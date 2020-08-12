use serde::{Deserialize, Serialize};

use std::collections::HashMap;

use crate::error::*;
use crate::platform::{read_file, write_file};

const PORT: u16 = 13536;
const CLIENT_ID: &str = "8abed7fc8c3343098e8c619ed7338fad";
const SCOPES: [&str; 14] = [
    "publicData",
    "esi-location.read_location.v1",
    "esi-location.read_ship_type.v1",
    "esi-skills.read_skills.v1",
    "esi-search.search_structures.v1",
    "esi-characters.read_contacts.v1",
    "esi-fleets.read_fleet.v1",
    "esi-ui.write_waypoint.v1",
    "esi-characters.read_standings.v1",
    "esi-location.read_online.v1",
    "esi-characters.read_fatigue.v1",
    "esi-corporations.read_contacts.v1",
    "esi-corporations.read_standings.v1",
    "esi-alliances.read_contacts.v1",
];
const OAUTH_AUTHORIZE: &str = "https://login.eveonline.com/v2/oauth/authorize/";
const OAUTH_TOKEN: &str = "https://login.eveonline.com/v2/oauth/token/";
const OAUTH_VERIFY: &str = "https://login.eveonline.com/oauth/verify/";

pub async fn load_or_authorize() -> Result<Profile, Error> {
    let profile: Option<Profile> = read_file("eve-profile.json")
        .await
        .ok()
        .and_then(|p| serde_json::from_slice(&p).ok());

    if let Some(profile) = profile {
        if crate::esi::ALWAYS_CACHE {
            return Ok(profile);
        }
        if profile.token.expired() {
            log::info!("oauth token expired, refreshing");
            if let Ok(profile) = refresh(profile).await {
                Ok(profile)
            } else {
                log::info!("oauth token invalid, authorizing");
                auth::authorize().await
            }
        } else if let Ok(_) = verify(&profile.token).await {
            log::info!("using existing oauth profile");
            Ok(profile)
        } else {
            log::info!("oauth token expired, refreshing");
            if let Ok(profile) = refresh(profile).await {
                Ok(profile)
            } else {
                log::info!("oauth token invalid, authorizing");
                auth::authorize().await
            }
        }
    } else {
        log::info!("no oauth profile found, authorizing");
        auth::authorize().await
    }
}

pub async fn refresh(mut profile: Profile) -> Result<Profile, Error> {
    log::info!("refreshing oauth credentials");
    let mut request_body = HashMap::new();
    request_body.insert("grant_type", "refresh_token".to_string());
    request_body.insert("refresh_token", profile.token.refresh_token.to_string());
    request_body.insert("client_id", CLIENT_ID.to_string());

    let client = reqwest::Client::new();
    let token_request = client.post(OAUTH_TOKEN).form(&request_body);
    let token_response = token_request.send().await;
    let token: AccessToken = token_response.unwrap().json().await.unwrap();

    profile.token = token;

    let json = serde_json::to_vec(&profile).unwrap();
    write_file("eve-profile.json", json).await.unwrap();

    Ok(profile)
}

async fn verify(token: &AccessToken) -> Result<Character, Error> {
    let client = reqwest::Client::new();
    let token_request = client
        .get(OAUTH_VERIFY)
        .header("Authorization", token.authorization());
    let verify_response = token_request.send().await.map_err(|_| Error)?;
    if verify_response.status().is_server_error() || verify_response.status().is_client_error() {
        Err(Error)
    } else {
        let character: Character = verify_response.json().await.unwrap();
        Ok(character)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Profile {
    pub character: Character,
    pub token: AccessToken,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Character {
    #[serde(rename = "CharacterID")]
    pub character_id: i32,
    #[serde(rename = "CharacterName")]
    pub character_name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AccessToken {
    access_token: String,
    expires_in: u64,
    token_type: String,
    refresh_token: String,
    #[serde(default = "AccessToken::now")]
    created_at: u64,
}

impl AccessToken {
    pub fn authorization(&self) -> String {
        format!("Bearer {}", self.access_token)
    }

    pub fn now() -> u64 {
        /*
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_else(|err| err.duration())
            .as_secs()
        */
        0
    }

    pub fn expired(&self) -> bool {
        Self::now() > self.created_at + self.expires_in
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod auth {
    use async_std::sync::Mutex;
    use async_std::task::spawn;
    use futures::channel::mpsc::{channel as mpsc, Sender};
    use futures::channel::oneshot::channel as oneshot;
    use futures::SinkExt;
    use futures::StreamExt;
    use hyper::service::make_service_fn;
    use hyper::{Body, Method, Request, Response, Server};
    use reqwest::Url;

    use std::convert::Infallible;
    use std::net::SocketAddr;
    use std::sync::Arc;

    use super::*;

    pub async fn authorize() -> Result<Profile, Error> {
        let (start_tx, start_rx) = oneshot();
        let (end_tx, end_rx) = oneshot();

        let (profile_tx, mut profile_rx) = mpsc(1);
        let server = spawn({
            let profile_tx = profile_tx.clone();
            async move {
                let oauth_state = Arc::new(Mutex::new(HashMap::new()));
                let profile_tx = profile_tx.clone();
                let addr = SocketAddr::from(([127, 0, 0, 1], PORT));

                let make_service = make_service_fn(|_| {
                    let oauth_state = oauth_state.clone();
                    let profile_tx = profile_tx.clone();
                    async move {
                        let oauth_state = oauth_state.clone();
                        let profile_tx = profile_tx.clone();
                        Ok::<_, Infallible>(OauthService {
                            oauth_state,
                            profile_tx,
                        })
                    }
                });

                let server = Server::bind(&addr)
                    .serve(make_service)
                    .with_graceful_shutdown(async { end_rx.await.unwrap() });

                start_tx.send(()).unwrap();
                if let Err(e) = server.await {
                    log::error!("oauth server error: {}", e);
                }
            }
        });

        let _ = start_rx.await.unwrap();
        let auth_url = format!("http://localhost:{}/esi-redirect/", PORT);
        log::info!(
            "opening authorization page in default web browser: {}",
            auth_url
        );

        std::thread::spawn(move || {
            let browser_err = webbrowser::open(&auth_url);
            if let Err(error) = browser_err {
                log::error!("unable to open browser: {:?}", error);
            }
        });

        let profile = profile_rx.next().await.unwrap();
        end_tx.send(()).unwrap();
        server.await;

        let json = serde_json::to_vec(&profile).unwrap();
        write_file("eve-profile.json", json).await.unwrap();

        Ok(profile)
    }

    struct OauthService {
        oauth_state: Arc<Mutex<HashMap<String, String>>>,
        profile_tx: Sender<Profile>,
    }

    impl hyper::service::Service<Request<Body>> for OauthService {
        type Response = Response<Body>;
        type Error = Infallible;

        type Future = std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
        >;

        fn poll_ready(
            &mut self,
            _ctx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Ready(Ok(()))
        }

        fn call(&mut self, request: Request<Body>) -> Self::Future {
            let oauth_state = self.oauth_state.clone();
            let mut profile_tx = self.profile_tx.clone();
            let fut = async move {
                match (request.method(), request.uri().path()) {
                    (&Method::GET, "/esi-redirect") | (&Method::GET, "/esi-redirect/") => {
                        let state: String = base64::encode_config(
                            rand::random::<[u8; 32]>(),
                            base64::URL_SAFE_NO_PAD,
                        );
                        let secret: String = base64::encode_config(
                            rand::random::<[u8; 32]>(),
                            base64::URL_SAFE_NO_PAD,
                        );

                        {
                            oauth_state
                                .lock()
                                .await
                                .insert(state.clone(), secret.clone());
                        }

                        use sha2::Digest;
                        let hash = sha2::Sha256::digest(secret.as_bytes());
                        let url_secret = base64::encode_config(hash, base64::URL_SAFE_NO_PAD);

                        let mut authorize = Url::parse(OAUTH_AUTHORIZE).unwrap();
                        authorize
                            .query_pairs_mut()
                            .append_pair("response_type", "code")
                            .append_pair(
                                "redirect_uri",
                                &format!("http://localhost:{}/esi-callback/", PORT),
                            )
                            .append_pair("client_id", CLIENT_ID)
                            .append_pair("scope", &SCOPES[..].join(" "))
                            .append_pair("code_challenge", &url_secret)
                            .append_pair("code_challenge_method", "S256")
                            .append_pair("state", &state);

                        let response = Response::builder()
                            .header("Location", authorize.as_str())
                            .header("Cache-Control", "no-cache")
                            .status(307)
                            .body(Body::empty())
                            .unwrap();
                        Ok(response)
                    }
                    (&Method::GET, "/esi-callback") | (&Method::GET, "/esi-callback/") => {
                        let url = Url::parse(&format!(
                            "http://localhost:{}{}",
                            PORT,
                            request.uri().to_string()
                        ))
                        .unwrap();

                        let mut code = None;
                        let mut request_state = None;
                        for (key, value) in url.query_pairs() {
                            let key = key.into_owned();
                            match key.as_str() {
                                "code" => code = Some(value.into_owned()),
                                "state" => request_state = Some(value.into_owned()),
                                _ => (),
                            }
                        }

                        let (code, request_state) = match (code, request_state) {
                            (None, _) | (_, None) => {
                                let response = Response::builder()
                                    .status(400)
                                    .body(Body::from("'code' and 'state' parameters are required."))
                                    .unwrap();
                                return Ok(response);
                            }
                            (Some(code), Some(request_state)) => (code, request_state),
                        };

                        let secret = { oauth_state.lock().await.get(&request_state).cloned() };

                        let secret = match secret {
                            None => {
                                let response = Response::builder()
                                    .status(400)
                                    .body(Body::from("'state' is invalid."))
                                    .unwrap();
                                return Ok(response);
                            }
                            Some(secret) => secret,
                        };

                        let mut request_body = HashMap::new();
                        request_body.insert("grant_type", "authorization_code".to_string());
                        request_body.insert("code", code);
                        request_body.insert("client_id", CLIENT_ID.to_string());
                        request_body.insert("code_verifier", secret);

                        let client = reqwest::Client::new();
                        let token_request = client.post(OAUTH_TOKEN).form(&request_body);
                        let token_response = token_request.send().await;
                        let token: AccessToken = token_response.unwrap().json().await.unwrap();

                        let character = verify(&token).await.unwrap();

                        profile_tx
                            .send(Profile {
                                character: character.clone(),
                                token,
                            })
                            .await
                            .unwrap();

                        let response = Response::builder()
                            .status(200)
                            .body(Body::from(format!(
                                "Hello, {}! You may now close this browser window",
                                character.character_name
                            )))
                            .unwrap();
                        Ok(response)
                    }
                    (m, p) => {
                        log::warn!("unexpected oauth request: {} {}", m, p);
                        let response = Response::builder().status(404).body(Body::empty()).unwrap();
                        Ok(response)
                    }
                }
            };

            Box::pin(fut)
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod auth {
    use super::*;

    pub async fn authorize() -> Result<Profile, Error> {
        Err(Error)
    }
}
