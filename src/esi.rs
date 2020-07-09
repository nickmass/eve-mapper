use reqwest::Url;
use serde::Deserialize;
use tokio::sync::RwLock;

use std::sync::Arc;

use super::oauth::{self, Profile};

pub const ALWAYS_CACHE: bool = false;

#[derive(Copy, Clone, Debug)]
enum EsiEndpoint {
    Latest,
}

impl EsiEndpoint {
    fn as_url_base(&self) -> Url {
        match *self {
            EsiEndpoint::Latest => Url::parse("https://esi.evetech.net/latest/").unwrap(),
        }
    }
}

#[derive(Clone)]
pub struct Client {
    endpoint: EsiEndpoint,
    client: reqwest::Client,
    profile: Arc<RwLock<Profile>>,
}

#[derive(Debug)]
pub enum Error {
    InvalidUrlPath(String, Box<dyn std::error::Error>),
    InvalidRequest(reqwest::Error),
    Io(tokio::io::Error),
    ResponseDeserialize(serde_json::Error),
    CannotCloneRequest,
    CannotExecuteRequest(reqwest::Error),
    CannotRetrieveRequestBody(reqwest::Error),
    CannotParseAuthorizationHeader,
    InvalidEsiLimitHeader(String),
    RetriesExhausted,
}

impl Client {
    pub fn new(profile: Profile) -> Client {
        Client {
            endpoint: EsiEndpoint::Latest,
            client: reqwest::Client::new(),
            profile: Arc::new(RwLock::new(profile)),
        }
    }

    async fn get<S: AsRef<str>, T: serde::de::DeserializeOwned>(
        &self,
        path: S,
    ) -> Result<T, Error> {
        self.execute_get(path, false, true).await
    }

    async fn get_no_cache<S: AsRef<str>, T: serde::de::DeserializeOwned>(
        &self,
        path: S,
    ) -> Result<T, Error> {
        self.execute_get(path, false, false).await
    }

    async fn get_auth<S: AsRef<str>, T: serde::de::DeserializeOwned>(
        &self,
        path: S,
    ) -> Result<T, Error> {
        self.execute_get(path, true, true).await
    }

    async fn get_auth_no_cache<S: AsRef<str>, T: serde::de::DeserializeOwned>(
        &self,
        path: S,
    ) -> Result<T, Error> {
        self.execute_get(path, true, false).await
    }

    async fn execute_get<S: AsRef<str>, T: serde::de::DeserializeOwned>(
        &self,
        path: S,
        auth: bool,
        cache: bool,
    ) -> Result<T, Error> {
        let cache = ALWAYS_CACHE || cache;

        let uuid = uuid::Uuid::new_v4();
        let path = path.as_ref();
        let url = self
            .endpoint
            .as_url_base()
            .join(path)
            .map_err(|e| Error::InvalidUrlPath(path.to_string(), e.into()))?;
        log::info!("esi request {}: {}", uuid, url);

        use sha2::Digest;
        let path_hash = format!("{:x}", sha2::Sha256::digest(url.as_str().as_bytes()));
        let path = std::path::PathBuf::from(format!("local-cache/{}", path_hash));

        let mut request = self.client.get(url).header(
            "User-Agent",
            "EveMapper-Development v0.0001: nickmass@nickmass.com",
        );

        if auth {
            let auth = self.profile.read().await.token.authorization();
            request = request.header("Authorization", auth);
        }

        let mut request = request.build().map_err(|e| Error::InvalidRequest(e))?;

        if cache {
            if path.exists() {
                log::info!("esi request found in cache {}", uuid);
                match tokio::fs::read(&path).await {
                    Ok(bytes) => match serde_json::from_slice(&bytes) {
                        Ok(data) => return Ok(data),
                        Err(error) => log::error!(
                            "esi unable to deserialize from cache {}: {:?}",
                            uuid,
                            error
                        ),
                    },
                    Err(error) => {
                        log::error!("esi unable to read from cache {}: {:?}", uuid, error)
                    }
                }
            }
        }

        let mut retry_count: u32 = 0;

        while retry_count < 5 {
            let this_request = request.try_clone().ok_or(Error::CannotCloneRequest)?;
            let request_start = std::time::Instant::now();
            let response = self
                .client
                .execute(this_request)
                .await
                .map_err(Error::CannotExecuteRequest)?;

            let status_code = response.status().as_u16();
            log::info!(
                "esi response {}: {} after {}ms",
                status_code,
                uuid,
                request_start.elapsed().as_millis()
            );

            let reauth = auth && status_code == 401 || status_code == 403;
            let retry = response.status().is_server_error() || response.status().is_client_error();
            let limit = response.headers().get("X-Esi-Error-Limit-Reset");

            if reauth {
                log::info!("esi refreshing authentication token {}", uuid);
                let reauth_start = std::time::Instant::now();
                let mut profile = self.profile.write().await;
                if let Ok(new_profile) = oauth::refresh(profile.clone()).await {
                    *profile = new_profile;
                    log::info!(
                        "esi refreshed authentication token {} after {}ms",
                        uuid,
                        reauth_start.elapsed().as_millis()
                    );

                    let header_value = profile
                        .token
                        .authorization()
                        .parse()
                        .map_err(|_| Error::CannotParseAuthorizationHeader)?;

                    request
                        .headers_mut()
                        .get_mut("Authorization")
                        .map(|v| *v = header_value);
                }
            }

            if let (Some(limit), true) = (limit, retry) {
                let dur = limit
                    .to_str()
                    .map_err(|_| Error::InvalidEsiLimitHeader(format!("{:?}", limit.as_bytes())))?
                    .parse::<u64>()
                    .map_err(|_| Error::InvalidEsiLimitHeader(format!("{:?}", limit.as_bytes())))?
                    * 1000;
                log::warn!(
                    "esi error limit header found {} delaying for {}ms",
                    uuid,
                    dur
                );
                tokio::time::delay_for(std::time::Duration::from_millis(dur)).await;
            }

            if !retry {
                let bytes = response
                    .bytes()
                    .await
                    .map_err(Error::CannotRetrieveRequestBody)?;
                if cache {
                    log::info!("esi updating cache contents {}", uuid);
                    tokio::fs::write(path, &bytes).await.map_err(Error::Io)?;
                }
                return Ok(serde_json::from_slice(&bytes).map_err(Error::ResponseDeserialize)?);
            }
            retry_count += 1;
            log::error!(
                "esi request failed {} retrying attempt {}",
                uuid,
                retry_count
            );
        }

        log::error!("esi retries exahusted {}", uuid);
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
        self.get(&url).await
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
        self.get(&url).await
    }

    pub async fn get_alliance(&self, alliance_id: i32) -> Result<GetAlliance, Error> {
        let url = format!("alliances/{}/", alliance_id);
        self.get(&url).await
    }

    pub async fn get_alliance_contacts(
        &self,
        alliance_id: i32,
        page: i32,
    ) -> Result<Vec<GetAllianceContacts>, Error> {
        let url = format!("alliances/{}/contacts/?page={}", alliance_id, page);
        self.get_auth_no_cache(&url).await
    }

    pub async fn get_corporation_contacts(
        &self,
        corporation_id: i32,
        page: i32,
    ) -> Result<Vec<GetCorporationContacts>, Error> {
        let url = format!("corporations/{}/contacts/?page={}", corporation_id, page);
        self.get_auth_no_cache(&url).await
    }

    pub async fn get_character_contacts(
        &self,
        page: i32,
    ) -> Result<Vec<GetCharacterContacts>, Error> {
        let character = self.profile.read().await.character.character_id;
        let url = format!("characters/{}/contacts/?page={}", character, page);
        self.get_auth_no_cache(&url).await
    }

    pub async fn get_sovereignty_map(&self) -> Result<Vec<GetSovereigntyMap>, Error> {
        let url = format!("sovereignty/map/");
        self.get_no_cache(&url).await
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct GetUniverseSystem {
    pub system_id: i32,
    pub name: String,
    pub position: Position,
    pub security_status: f64,
    pub constellation_id: i32,
    pub stargates: Option<Vec<i32>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GetUniverseStargate {
    pub stargate_id: i32,
    pub name: String,
    pub position: Position,
    pub destination: GetUniverseStargateDestination,
    pub system_id: i32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GetUniverseStargateDestination {
    pub stargate_id: i32,
    pub system_id: i32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Position {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GetUniverseRegion {
    pub region_id: i32,
    pub name: String,
    pub description: Option<String>,
    pub constellations: Option<Vec<i32>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GetUniverseConstellation {
    pub constellation_id: i32,
    pub name: String,
    pub position: Position,
    pub region_id: i32,
    pub systems: Option<Vec<i32>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GetUniverseSystemKills {
    pub npc_kills: i32,
    pub pod_kills: i32,
    pub ship_kills: i32,
    pub system_id: i32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GetUniverseSystemJumps {
    pub ship_jumps: i32,
    pub system_id: i32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GetCharacterLocation {
    pub solar_system_id: i32,
    pub station_id: Option<i64>,
    pub structure_id: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GetAllianceContacts {
    pub contact_id: i32,
    pub contact_type: String,
    pub standing: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GetCorporationContacts {
    pub contact_id: i32,
    pub contact_type: String,
    pub standing: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GetCharacterContacts {
    pub contact_id: i32,
    pub contact_type: String,
    pub is_blocked: Option<bool>,
    pub is_watched: Option<bool>,
    pub standing: f64,
}

#[derive(Debug, Clone, Deserialize)]
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
#[derive(Debug, Clone, Deserialize)]
pub struct GetCorporation {
    pub alliance_id: Option<i32>,
    pub name: String,
    pub ticker: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GetAlliance {
    pub name: String,
    pub ticker: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GetSovereigntyMap {
    pub system_id: i32,
    pub alliance_id: Option<i32>,
    pub corporation_id: Option<i32>,
    pub faction_id: Option<i32>,
}
