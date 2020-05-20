use super::math;

#[derive(Clone)]
pub struct Client {
    client: reqwest::Client,
    count: std::cell::Cell<u32>,
}

impl Client {
    pub fn new() -> Client {
        Client {
            client: reqwest::Client::new(),
            count: std::cell::Cell::new(0),
        }
    }

    async fn send<T: serde::de::DeserializeOwned>(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<T, ()> {
        let n = self.count.get() + 1;
        self.count.set(n);
        let request = request
            .header(
                "User-Agent",
                "EveMapper-Development v0.0001: nickmass@nickmass.com",
            )
            .build()
            .unwrap();
        let path_hash = sha1::Sha1::from(request.url().as_str()).hexdigest();
        let path = std::path::PathBuf::from(format!("local-cache/{}", path_hash));
        if path.exists() {
            let bytes = tokio::fs::read(path).await.unwrap();
            return Ok(serde_json::from_slice(&bytes).unwrap());
        }

        let start = std::time::Instant::now();
        let mut retry_count = 0;
        while retry_count < 5 {
            let res = self
                .client
                .execute(request.try_clone().unwrap())
                .await
                .unwrap();
            let retry = res.status().is_server_error() || res.status().is_client_error();
            let limit = res.headers().get("X-Esi-Error-Limit-Reset");

            if let (Some(limit), true) = (limit, retry) {
                eprintln!("{:?}", res.headers());
                let dur = limit.to_str().unwrap().parse::<u64>().unwrap() * 1000;
                tokio::time::delay_for(std::time::Duration::from_millis(dur)).await;
            }

            if !retry {
                let bytes = res.bytes().await.unwrap();
                tokio::fs::write(path, &bytes).await.unwrap();
                println!("DURATION: {}", start.elapsed().as_millis());
                return Ok(serde_json::from_slice(&bytes).unwrap());
            }
            retry_count += 1;
        }

        panic!("ESI request failed {} times, aborting", retry_count)
    }
}

pub mod universe {
    use super::*;
    use serde::Deserialize;

    impl Client {
        pub async fn get_universe_systems(&self) -> Result<Vec<i32>, ()> {
            let url = format!("https://esi.evetech.net/latest/universe/systems/");
            let request = self.client.get(&url);
            let result = self.send(request).await.unwrap();
            Ok(result)
        }

        pub async fn get_universe_system(&self, system_id: i32) -> Result<GetUniverseSystem, ()> {
            let url = format!(
                "https://esi.evetech.net/latest/universe/systems/{}/",
                system_id
            );
            let request = self.client.get(&url);
            let result = self.send(request).await.unwrap();
            Ok(result)
        }

        pub async fn get_universe_stargate(
            &self,
            stargate_id: i32,
        ) -> Result<GetUniverseStargate, ()> {
            let url = format!(
                "https://esi.evetech.net/latest/universe/stargates/{}/",
                stargate_id
            );
            let request = self.client.get(&url);
            let result = self.send(request).await.unwrap();
            Ok(result)
        }
    }

    #[derive(Debug, Deserialize)]
    pub struct GetUniverseSystem {
        pub system_id: i32,
        pub name: String,
        pub position: Position,
        pub security_status: f64,
        pub constellation_id: i32,
        pub stargates: Option<Vec<i32>>,
    }

    impl std::fmt::Display for GetUniverseSystem {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            write!(f, "{}", self.name)
        }
    }

    #[derive(Debug, Deserialize)]
    pub struct GetUniverseStargate {
        pub stargate_id: i32,
        pub name: String,
        pub position: Position,
        pub destination: GetUniverseStargateDestination,
        pub system_id: i32,
    }

    impl std::fmt::Display for GetUniverseStargate {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            write!(f, "{}", self.name)
        }
    }

    #[derive(Debug, Deserialize)]
    pub struct GetUniverseStargateDestination {
        pub stargate_id: i32,
        pub system_id: i32,
    }

    #[derive(Debug, Deserialize)]
    pub struct Position {
        pub x: f64,
        pub y: f64,
        pub z: f64,
    }

    impl From<&Position> for math::V3<f64> {
        fn from(position: &Position) -> Self {
            math::V3 {
                x: position.x,
                y: position.y,
                z: position.z,
            } / 1e12
        }
    }
}
