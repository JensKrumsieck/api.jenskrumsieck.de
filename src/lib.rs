pub mod auth;
mod instagram;
pub mod media;

use once_cell::sync::Lazy;
use securestore::{KeySource, SecretsManager};
use std::{
    env, fs,
    sync::{Mutex, OnceLock},
};
use tokio::sync::RwLock;

use crate::media::Media;

pub const INSTAGRAM_GRAPH_ENDPOINT: &str = "https://graph.instagram.com/v25.0";

static SECRETS: Lazy<Mutex<SecretsManager>> = Lazy::new(|| {
    let key = env::var("APP_SECRET").expect("No secure key set");
    if !fs::exists("secrets.json").unwrap() {
        let manager = SecretsManager::new(KeySource::Password(&key))
            .expect("Could not create new SecretsManager");
        manager
            .save_as("secrets.json")
            .expect("Could not save SecretsManager");
    }
    if let Ok(manager) = SecretsManager::load("secrets.json", KeySource::Password(&key)) {
        Mutex::new(manager)
    } else {
        let manager = SecretsManager::new(KeySource::Password(&key))
            .expect("Could not create new SecretsManager");
        manager
            .save_as("secrets.json")
            .expect("Could not save SecretsManager");
        Mutex::new(manager)
    }
});

static INSTAGRAM_CACHE: OnceLock<RwLock<Vec<Media>>> = OnceLock::new();
pub fn instagram_cache() -> &'static RwLock<Vec<Media>> {
    INSTAGRAM_CACHE.get_or_init(|| RwLock::new(Vec::new()))
}

pub async fn get_instagram_media() -> Vec<Media> {
    instagram_cache().read().await.clone()
}

pub async fn refresh_instagram_media_cache() -> anyhow::Result<()> {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(24 * 60 * 60));
    loop {
        interval.tick().await;
        match media::rebuild_media_cache().await {
            Ok(media) => *instagram_cache().write().await = media,
            Err(e) => tracing::error!("Cache refresh failed: {e:#}"),
        }
    }
}
