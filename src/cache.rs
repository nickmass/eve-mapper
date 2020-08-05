use serde::{Deserialize, Serialize};

use tokio::sync::RwLock;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

trait Expiry {
    fn is_expired(expires: u64) -> bool;
}

struct NeverExpires;
impl Expiry for NeverExpires {
    fn is_expired(_expires: u64) -> bool {
        false
    }
}

struct CheckExpiry;
impl Expiry for CheckExpiry {
    fn is_expired(expires: u64) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(u64::MAX);
        now > expires
    }
}

struct MonthExpiry;
impl Expiry for MonthExpiry {
    fn is_expired(expires: u64) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(u64::MAX);
        now > (expires + (60 * 60 * 24 * 30))
    }
}

pub struct Cache {
    static_store: Store<NeverExpires>,
    dynamic_store: Store<CheckExpiry>,
    image_store: Store<MonthExpiry>,
}

struct Store<T: Expiry> {
    path: PathBuf,
    entries: RwLock<HashMap<String, Entry>>,
    dirty: RwLock<bool>,
    expiry: std::marker::PhantomData<T>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Entry {
    expires: u64,
    etag: Option<String>,
    #[serde(with = "serde_bytes")]
    data: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CacheKind {
    None,
    Static,
    Dynamic,
    Image,
}

#[derive(Debug, Clone)]
pub enum CacheError<T> {
    Expired(Option<String>, T),
    NonExistant,
}

#[derive(Debug)]
pub enum Error {
    Io(tokio::io::Error),
    Deserialize(flexbuffers::DeserializationError),
    Serialize(flexbuffers::SerializationError),
}

impl Cache {
    pub async fn new<S: AsRef<Path>, D: AsRef<Path>, I: AsRef<Path>>(
        static_store: S,
        dynamic_store: D,
        image_store: I,
    ) -> Result<Cache, Error> {
        let static_path = static_store.as_ref();
        let dynamic_path = dynamic_store.as_ref();
        let image_path = image_store.as_ref();

        let static_store = Store::load(static_path).await?;
        let dynamic_store = Store::load(dynamic_path).await?;
        let image_store = Store::load(image_path).await?;

        Ok(Cache {
            static_store,
            dynamic_store,
            image_store,
        })
    }

    pub async fn get<T: serde::de::DeserializeOwned, K: AsRef<str>>(
        &self,
        key: K,
        kind: CacheKind,
    ) -> Result<T, CacheError<T>> {
        match kind {
            CacheKind::Static => self.static_store.get(key).await,
            CacheKind::Dynamic => self.dynamic_store.get(key).await,
            CacheKind::Image => self.image_store.get(key).await,
            CacheKind::None => Err(CacheError::NonExistant),
        }
    }

    pub async fn store<T: serde::Serialize, K: AsRef<str>>(
        &self,
        key: K,
        kind: CacheKind,
        value: T,
        etag: Option<String>,
        expires: SystemTime,
    ) -> Result<(), Error> {
        let expires = expires
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        match kind {
            CacheKind::Static => self.static_store.store(key, value, etag, expires).await,
            CacheKind::Dynamic => self.dynamic_store.store(key, value, etag, expires).await,
            CacheKind::Image => self.image_store.store(key, value, etag, expires).await,
            CacheKind::None => Ok(()),
        }
    }

    pub async fn save(&self) -> Result<(), Error> {
        self.static_store.save().await?;
        self.dynamic_store.save().await?;
        self.image_store.save().await?;

        Ok(())
    }
}

impl<E: Expiry> Store<E> {
    async fn load<P: AsRef<Path>>(path: P) -> Result<Store<E>, Error> {
        let path = path.as_ref();
        let entries = if path.exists() {
            let bytes = tokio::fs::read(&path).await.map_err(Error::Io)?;
            flexbuffers::from_slice(&bytes).map_err(Error::Deserialize)?
        } else {
            HashMap::new()
        };

        log::info!("loaded cache {}, {} entries", path.display(), entries.len());

        Ok(Store {
            path: path.to_owned(),
            entries: RwLock::new(entries),
            dirty: RwLock::new(false),
            expiry: Default::default(),
        })
    }

    async fn get<T: serde::de::DeserializeOwned, K: AsRef<str>>(
        &self,
        key: K,
    ) -> Result<T, CacheError<T>> {
        let key = key.as_ref();
        let map = self.entries.read().await;
        if let Some(entry) = map.get(key) {
            let data = flexbuffers::from_slice(&entry.data);
            if let Ok(data) = data {
                if E::is_expired(entry.expires) {
                    Err(CacheError::Expired(entry.etag.clone(), data))
                } else {
                    Ok(data)
                }
            } else {
                Err(CacheError::NonExistant)
            }
        } else {
            Err(CacheError::NonExistant)
        }
    }

    async fn store<T: serde::Serialize, K: AsRef<str>>(
        &self,
        key: K,
        value: T,
        etag: Option<String>,
        expires: u64,
    ) -> Result<(), Error> {
        let key = key.as_ref().to_owned();
        let mut map = self.entries.write().await;
        let data = flexbuffers::to_vec(value).map_err(Error::Serialize)?;
        let entry = Entry {
            expires,
            data,
            etag,
        };
        map.insert(key.clone(), entry);
        *self.dirty.write().await = true;
        Ok(())
    }

    async fn save(&self) -> Result<(), Error> {
        if *self.dirty.read().await {
            log::info!("saving cache to {}", self.path.display());
            *self.dirty.write().await = false;
            let entries = self.entries.read().await;
            let data = flexbuffers::to_vec(&*entries).map_err(Error::Serialize)?;
            tokio::fs::write(&self.path, data)
                .await
                .map_err(Error::Io)?;
        }

        Ok(())
    }
}
