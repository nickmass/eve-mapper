use async_std::sync::RwLock;
use async_std::task::sleep;
use futures_intrusive::sync::Semaphore;
use reqwest::{header, Method, Response, Url};
use serde::{Deserialize, Serialize};

use std::sync::Arc;

use crate::cache::{Cache, CacheError, CacheKind};
use crate::oauth::{self, Profile};
use crate::platform::time::{Instant, SystemTime};
use crate::platform::{parse_http_date, spawn, ESI_IMAGE_SERVER, USER_AGENT};

pub const ALWAYS_CACHE: bool = false;

#[derive(Copy, Clone, Debug)]
enum EsiEndpoint {
    Latest,
    Images,
}

impl EsiEndpoint {
    fn as_url_base(&self) -> Url {
        match *self {
            EsiEndpoint::Latest => Url::parse("https://esi.evetech.net/latest/").unwrap(),
            EsiEndpoint::Images => Url::parse(ESI_IMAGE_SERVER).unwrap(),
        }
    }
}

#[derive(Clone)]
pub struct Client {
    endpoint: EsiEndpoint,
    image_endpoint: EsiEndpoint,
    client: reqwest::Client,
    profile: Arc<RwLock<Profile>>,
    cache: Arc<Cache>,
    limiter: Arc<Semaphore>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("endpoint", &self.endpoint)
            .field("image_endpoint", &self.image_endpoint)
            .finish()
    }
}

#[derive(Debug)]
pub enum Error {
    InvalidUrlPath(String),
    Io(std::io::Error),
    ResponseDeserialize(serde_json::Error),
    CannotExecuteRequest(reqwest::Error),
    CannotRetrieveRequestBody(reqwest::Error),
    InvalidEsiLimitHeader(String),
    RetriesExhausted,
}

impl Client {
    pub async fn new(profile: Profile) -> Client {
        let cache = Arc::new(
            Cache::new("eve-static.dat", "eve-dynamic.dat", "eve-images.dat")
                .await
                .unwrap(),
        );

        let inner_cache = cache.clone();

        spawn(async move {
            loop {
                sleep(std::time::Duration::from_secs(120)).await;
                let save_res = inner_cache.save().await;
                match save_res {
                    Err(error) => log::error!("cache save error: {:?}", error),
                    _ => (),
                }
            }
        });
        Client {
            endpoint: EsiEndpoint::Latest,
            image_endpoint: EsiEndpoint::Images,
            client: reqwest::Client::new(),
            profile: Arc::new(RwLock::new(profile)),
            cache,
            limiter: Arc::new(Semaphore::new(true, 5)),
        }
    }

    async fn get<S: AsRef<str>, T: serde::de::DeserializeOwned + serde::Serialize>(
        &self,
        path: S,
    ) -> Result<T, Error> {
        self.execute(
            Method::GET,
            &self.endpoint,
            path,
            false,
            CacheKind::Static,
            |bytes| serde_json::from_slice(bytes).map_err(Error::ResponseDeserialize),
            |_, _| (),
        )
        .await
    }

    async fn get_no_cache<S: AsRef<str>, T: serde::de::DeserializeOwned + serde::Serialize>(
        &self,
        path: S,
    ) -> Result<T, Error> {
        self.execute(
            Method::GET,
            &self.endpoint,
            path,
            false,
            CacheKind::Dynamic,
            |bytes| serde_json::from_slice(bytes).map_err(Error::ResponseDeserialize),
            |_, _| (),
        )
        .await
    }

    async fn get_auth_no_cache<S: AsRef<str>, T: serde::de::DeserializeOwned + serde::Serialize>(
        &self,
        path: S,
    ) -> Result<T, Error> {
        {
            let mut profile = self.profile.write().await;
            if profile.token.expired() {
                if let Ok(new_profile) = oauth::refresh(profile.clone()).await {
                    *profile = new_profile;
                }
            }
        }
        self.execute(
            Method::GET,
            &self.endpoint,
            path,
            true,
            CacheKind::Dynamic,
            |bytes| serde_json::from_slice(bytes).map_err(Error::ResponseDeserialize),
            |_, _| (),
        )
        .await
    }

    async fn get_auth_no_cache_with_headers<
        S: AsRef<str>,
        T: serde::de::DeserializeOwned + serde::Serialize,
        FH: Fn(&mut T, &header::HeaderMap),
    >(
        &self,
        path: S,
        map_headers: FH,
    ) -> Result<T, Error> {
        {
            let mut profile = self.profile.write().await;
            if profile.token.expired() {
                if let Ok(new_profile) = oauth::refresh(profile.clone()).await {
                    *profile = new_profile;
                }
            }
        }
        self.execute(
            Method::GET,
            &self.endpoint,
            path,
            true,
            CacheKind::Dynamic,
            |bytes| serde_json::from_slice(bytes).map_err(Error::ResponseDeserialize),
            map_headers,
        )
        .await
    }

    async fn post_auth<S: AsRef<str>>(&self, path: S) -> Result<(), Error> {
        {
            let mut profile = self.profile.write().await;
            if profile.token.expired() {
                if let Ok(new_profile) = oauth::refresh(profile.clone()).await {
                    *profile = new_profile;
                }
            }
        }
        self.execute(
            Method::POST,
            &self.endpoint,
            path,
            true,
            CacheKind::None,
            |_| Ok(()),
            |_, _| (),
        )
        .await
    }

    async fn get_image<S: AsRef<str>>(&self, path: S) -> Result<Vec<u8>, Error> {
        let logo = self
            .execute(
                Method::GET,
                &self.image_endpoint,
                path,
                true,
                CacheKind::Image,
                |bytes| Ok(serde_bytes::ByteBuf::from(bytes)),
                |_, _| (),
            )
            .await;
        logo.map(serde_bytes::ByteBuf::into_vec)
    }

    async fn execute<
        S: AsRef<str>,
        F: Fn(&[u8]) -> Result<T, Error>,
        FH: Fn(&mut T, &header::HeaderMap),
        T: serde::de::DeserializeOwned + serde::Serialize,
    >(
        &self,
        method: Method,
        endpoint: &EsiEndpoint,
        path: S,
        auth: bool,
        cache_kind: CacheKind,
        map_value: F,
        map_headers: FH,
    ) -> Result<T, Error> {
        let uuid = uuid::Uuid::new_v4();
        let mut retry_count: u32 = 0;
        while retry_count < 5 {
            let path = path.as_ref();
            let url = endpoint
                .as_url_base()
                .join(path)
                .map_err(|_e| Error::InvalidUrlPath(path.to_string()))?;

            use sha2::Digest;
            let path_hash = format!("{:x}", sha2::Sha256::digest(url.as_str().as_bytes()));

            let mut request = self.client.request(method.clone(), url.clone());

            if let Some(user_agent) = USER_AGENT {
                request = request.header(header::USER_AGENT, user_agent);
            }

            let (response, request_start, cached_value) = {
                let _permit = self.limiter.acquire(1).await;

                if auth {
                    let auth = self.profile.read().await.token.authorization();
                    request = request.header(header::AUTHORIZATION, auth);
                }

                log::debug!("looking up url in cache: {}", &url);
                let (etag, cached_value) =
                    match (cache_kind, self.cache.get(&path_hash, cache_kind).await) {
                        (CacheKind::None, _) => (None, None),
                        (_, Ok(value)) => return Ok(value),
                        (_, Err(CacheError::Expired(_, value))) if ALWAYS_CACHE => {
                            log::info!("returning expired data: {}", &url);
                            return Ok(value);
                        }
                        (_, Err(CacheError::Expired(etag, value))) => (etag, Some(value)),
                        (_, Err(_)) => (None, None),
                    };

                if let Some(etag) = etag {
                    request = request.header(header::IF_NONE_MATCH, etag)
                }

                log::info!("request {}: {}", uuid, url);
                let start = Instant::now();
                let response = request.send().await.map_err(Error::CannotExecuteRequest)?;
                (response, start, cached_value)
            };

            let status_code = response.status().as_u16();
            log::info!(
                "response {}: {} after {}ms",
                status_code,
                uuid,
                request_start.elapsed().as_millis()
            );

            let warning = response
                .headers()
                .get(header::WARNING)
                .and_then(|s| s.to_str().ok());
            if let Some(warning) = warning {
                log::warn!("warning in header {}: {}", uuid, warning);
            }

            let reauth = auth && status_code == 401 || status_code == 403;
            let retry = response.status().is_server_error() || response.status().is_client_error();
            let limit = response.headers().get("X-Esi-Error-Limit-Reset");
            let expires = response.headers().get(header::EXPIRES).cloned();

            if reauth {
                log::info!("refreshing authentication token {}", uuid);
                let reauth_start = Instant::now();
                let mut profile = self.profile.write().await;
                if let Ok(new_profile) = oauth::refresh(profile.clone()).await {
                    *profile = new_profile;
                    log::info!(
                        "refreshed authentication token {} after {}ms",
                        uuid,
                        reauth_start.elapsed().as_millis()
                    );
                }
            }

            if let (Some(limit), true) = (limit, retry) {
                let dur = limit
                    .to_str()
                    .map_err(|_| Error::InvalidEsiLimitHeader(format!("{:?}", limit.as_bytes())))?
                    .parse::<u64>()
                    .map_err(|_| Error::InvalidEsiLimitHeader(format!("{:?}", limit.as_bytes())))?
                    * 1000;
                log::warn!("error limit header found {} delaying for {}ms", uuid, dur);
                sleep(std::time::Duration::from_millis(dur)).await;
            }

            if !retry {
                let parsed_expires = expires
                    .as_ref()
                    .and_then(|v| v.to_str().map(String::from).ok())
                    .and_then(|v| parse_http_date(&v))
                    .or_else(|| parse_cache_control(&response));

                let etag = parse_etag(&response);

                let headers = response.headers().clone();

                let mut value = if let (Some(value), true) = (
                    cached_value,
                    response.status() == reqwest::StatusCode::NOT_MODIFIED,
                ) {
                    value
                } else {
                    let bytes = response
                        .bytes()
                        .await
                        .map_err(Error::CannotRetrieveRequestBody)?;
                    map_value(&bytes)?
                };

                map_headers(&mut value, &headers);

                if cache_kind != CacheKind::None {
                    if let Some(expires) = parsed_expires {
                        let cache_res = self
                            .cache
                            .store(&path_hash, cache_kind, &value, etag, expires)
                            .await;
                        match cache_res {
                            Err(error) => {
                                log::error!("unable to store in cache {}: {:?}", uuid, error)
                            }
                            _ => (),
                        }
                    } else {
                        log::warn!(
                            "invalid or missing expires header, skipping cache {}: {:?}",
                            uuid,
                            expires
                        );
                    }
                }

                return Ok(value);
            }
            retry_count += 1;
            log::error!("request failed {} retrying attempt {}", uuid, retry_count);
        }

        log::error!("retries exahusted {}", uuid);
        Err(Error::RetriesExhausted)
    }
}

impl Client {
    pub async fn get_universe_systems(&self) -> Result<Vec<i32>, Error> {
        let url = format!("universe/systems/");
        self.get(&url).await
    }

    pub async fn get_universe_system(&self, system_id: i32) -> Result<GetUniverseSystem, Error> {
        let url = format!("universe/systems/{}/", system_id);
        self.get(&url).await
    }

    pub async fn get_universe_stargate(
        &self,
        stargate_id: i32,
    ) -> Result<GetUniverseStargate, Error> {
        let url = format!("universe/stargates/{}/", stargate_id);
        self.get(&url).await
    }

    pub async fn get_universe_regions(&self) -> Result<Vec<i32>, Error> {
        let url = format!("universe/regions/");
        self.get(&url).await
    }

    pub async fn get_universe_region(&self, region_id: i32) -> Result<GetUniverseRegion, Error> {
        let url = format!("universe/regions/{}/", region_id);
        self.get(&url).await
    }

    pub async fn get_universe_constellations(&self) -> Result<Vec<i32>, Error> {
        let url = format!("universe/constellations/");
        self.get(&url).await
    }

    pub async fn get_universe_constellation(
        &self,
        constellation_id: i32,
    ) -> Result<GetUniverseConstellation, Error> {
        let url = format!("universe/constellations/{}/", constellation_id);
        self.get(&url).await
    }

    pub async fn get_universe_system_jumps(&self) -> Result<Vec<GetUniverseSystemJumps>, Error> {
        let url = format!("universe/system_jumps/");
        self.get_no_cache(&url).await
    }

    pub async fn get_universe_system_kills(&self) -> Result<Vec<GetUniverseSystemKills>, Error> {
        let url = format!("universe/system_kills/");
        self.get_no_cache(&url).await
    }

    pub async fn get_character_location(&self) -> Result<GetCharacterLocation, Error> {
        let character = self.profile.read().await.character.character_id;
        let url = format!("characters/{}/location/", character);
        self.get_auth_no_cache(&url).await
    }

    pub async fn get_character_self(&self) -> Result<GetCharacter, Error> {
        let character = self.profile.read().await.character.character_id;
        let url = format!("characters/{}/", character);
        self.get_no_cache(&url).await
    }

    pub async fn get_corporation(&self, corporation_id: i32) -> Result<GetCorporation, Error> {
        let url = format!("corporations/{}/", corporation_id);
        let mut res: Result<GetCorporation, _> = self.get(&url).await;
        if let Ok(res) = res.as_mut() {
            res.corporation_id = corporation_id;
        }
        res
    }

    pub async fn get_alliance(&self, alliance_id: i32) -> Result<GetAlliance, Error> {
        let url = format!("alliances/{}/", alliance_id);
        let mut res: Result<GetAlliance, _> = self.get(&url).await;
        if let Ok(res) = res.as_mut() {
            res.alliance_id = alliance_id;
        }
        res
    }

    pub async fn get_alliance_contacts(
        &self,
        alliance_id: i32,
        page: i32,
    ) -> Result<GetAllianceContacts, Error> {
        let url = format!("alliances/{}/contacts/?page={}", alliance_id, page);
        self.get_auth_no_cache_with_headers(&url, |contacts: &mut GetAllianceContacts, headers| {
            let pages = headers
                .get("x-pages")
                .and_then(|n| n.to_str().ok())
                .and_then(|n| n.parse().ok());
            contacts.pages = pages.or(contacts.pages);
        })
        .await
    }

    pub async fn get_corporation_contacts(
        &self,
        corporation_id: i32,
        page: i32,
    ) -> Result<GetCorporationContacts, Error> {
        let url = format!("corporations/{}/contacts/?page={}", corporation_id, page);
        self.get_auth_no_cache_with_headers(
            &url,
            |contacts: &mut GetCorporationContacts, headers| {
                let pages = headers
                    .get("x-pages")
                    .and_then(|n| n.to_str().ok())
                    .and_then(|n| n.parse().ok());
                contacts.pages = pages.or(contacts.pages);
            },
        )
        .await
    }

    pub async fn get_character_contacts(&self, page: i32) -> Result<GetCharacterContacts, Error> {
        let character = self.profile.read().await.character.character_id;
        let url = format!("characters/{}/contacts/?page={}", character, page);
        self.get_auth_no_cache_with_headers(&url, |contacts: &mut GetCharacterContacts, headers| {
            let pages = headers
                .get("x-pages")
                .and_then(|n| n.to_str().ok())
                .and_then(|n| n.parse().ok());
            contacts.pages = pages.or(contacts.pages);
        })
        .await
    }

    pub async fn get_sovereignty_map(&self) -> Result<Vec<GetSovereigntyMap>, Error> {
        let url = format!("sovereignty/map/");
        self.get_no_cache(&url).await
    }

    pub async fn get_alliance_logo(&self, alliance_id: i32, size: u32) -> Result<Vec<u8>, Error> {
        let url = format!("alliances/{}/logo?size={}", alliance_id, size);
        self.get_image(&url).await
    }

    pub async fn get_character_online(&self) -> Result<GetCharacterOnline, Error> {
        let character = self.profile.read().await.character.character_id;
        let url = format!("characters/{}/online/", character);
        self.get_auth_no_cache(&url).await
    }

    pub async fn post_waypoint(
        &self,
        add_to_beginning: bool,
        clear_other_waypoints: bool,
        destination_id: i32,
    ) -> Result<(), Error> {
        let url = format!(
            "ui/autopilot/waypoint/?add_to_beginning={}&clear_other_waypoints={}&destination_id={}",
            add_to_beginning, clear_other_waypoints, destination_id
        );

        self.post_auth(&url).await
    }
}

fn parse_cache_control(response: &Response) -> Option<SystemTime> {
    response
        .headers()
        .get(header::CACHE_CONTROL)
        .and_then(|s| s.to_str().ok())
        .map(str::trim)
        .and_then(|s| {
            if s.starts_with("max-age=") {
                let index = "max-age=".len();
                let seconds = s[index..].trim().parse::<u64>().ok()?;
                Some(SystemTime::now() + std::time::Duration::from_secs(seconds))
            } else if s == "no-cache" || s == "no-store" {
                Some(SystemTime::now() - std::time::Duration::from_secs(1))
            } else {
                None
            }
        })
}

fn parse_etag(response: &Response) -> Option<String> {
    response
        .headers()
        .get(header::ETAG)
        .and_then(|s| header::HeaderValue::to_str(s).ok())
        .map(str::trim)
        .and_then(|s| match s.as_bytes() {
            [b'"', s @ .., b'"'] => String::from_utf8(s.to_owned()).ok(),
            _ => None,
        })
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GetUniverseSystem {
    pub system_id: i32,
    pub name: String,
    pub position: Position,
    pub security_status: f64,
    pub constellation_id: i32,
    pub stargates: Option<Vec<i32>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GetUniverseStargate {
    pub stargate_id: i32,
    pub name: String,
    pub position: Position,
    pub destination: GetUniverseStargateDestination,
    pub system_id: i32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GetUniverseStargateDestination {
    pub stargate_id: i32,
    pub system_id: i32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Position {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GetUniverseRegion {
    pub region_id: i32,
    pub name: String,
    pub description: Option<String>,
    pub constellations: Option<Vec<i32>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GetUniverseConstellation {
    pub constellation_id: i32,
    pub name: String,
    pub position: Position,
    pub region_id: i32,
    pub systems: Option<Vec<i32>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GetUniverseSystemKills {
    pub npc_kills: i32,
    pub pod_kills: i32,
    pub ship_kills: i32,
    pub system_id: i32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GetUniverseSystemJumps {
    pub ship_jumps: i32,
    pub system_id: i32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GetCharacterLocation {
    pub solar_system_id: i32,
    pub station_id: Option<i64>,
    pub structure_id: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GetAllianceContact {
    pub contact_id: i32,
    pub contact_type: String,
    pub standing: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(transparent)]
pub struct GetAllianceContacts {
    pub contacts: Vec<GetAllianceContact>,
    #[serde(skip)]
    pub pages: Option<i32>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GetCorporationContact {
    pub contact_id: i32,
    pub contact_type: String,
    pub standing: f64,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(transparent)]
pub struct GetCorporationContacts {
    pub contacts: Vec<GetCorporationContact>,
    #[serde(skip)]
    pub pages: Option<i32>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GetCharacterContact {
    pub contact_id: i32,
    pub contact_type: String,
    pub is_blocked: Option<bool>,
    pub is_watched: Option<bool>,
    pub standing: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(transparent)]
pub struct GetCharacterContacts {
    pub contacts: Vec<GetCharacterContact>,
    #[serde(skip)]
    pub pages: Option<i32>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GetCharacter {
    pub alliance_id: Option<i32>,
    pub ancestry_id: Option<i32>,
    pub birthday: String,
    pub bloodline_id: i32,
    pub corporation_id: i32,
    pub description: Option<String>,
    pub faction_id: Option<i32>,
    pub gender: String,
    pub name: String,
    pub race_id: i32,
    pub security_status: Option<f64>,
    pub title: Option<String>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GetCorporation {
    #[serde(default)]
    pub corporation_id: i32,
    pub alliance_id: Option<i32>,
    pub name: String,
    pub ticker: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GetAlliance {
    #[serde(default)]
    pub alliance_id: i32,
    pub name: String,
    pub ticker: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GetSovereigntyMap {
    pub system_id: i32,
    pub alliance_id: Option<i32>,
    pub corporation_id: Option<i32>,
    pub faction_id: Option<i32>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GetCharacterOnline {
    pub last_login: Option<String>,
    pub last_logout: Option<String>,
    pub logins: Option<i32>,
    pub online: bool,
}
