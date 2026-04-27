use crate::instagram::{self, InstagramMediaData, get_instagram_posts};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::Path;
use tokio::{fs, io::AsyncWriteExt};
use tracing::debug;

#[derive(Debug, Serialize, Clone)]
pub enum MediaType {
    Gallery,
    Video,
    Image,
}

impl From<instagram::MediaType> for MediaType {
    fn from(value: instagram::MediaType) -> Self {
        match value {
            instagram::MediaType::Carousel => MediaType::Gallery,
            instagram::MediaType::Reel => MediaType::Video,
            instagram::MediaType::Image => MediaType::Image,
        }
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct Media {
    pub id: String,
    pub media_type: MediaType,
    pub permalink: Option<String>,
    pub image_url: String,
    pub caption: String,
    pub alt_text: String,
    pub timestamp: String,
}

impl From<InstagramMediaData> for Media {
    fn from(value: InstagramMediaData) -> Self {
        Self {
            id: value.id,
            media_type: value.media_type.into(),
            permalink: value.permalink,
            image_url: value.thumbnail_url.or(value.media_url).unwrap(),
            alt_text: value.alt_text.or(value.caption).unwrap(),
            caption: value.caption.unwrap_or_default(),
            timestamp: value.timestamp,
        }
    }
}

pub(crate) async fn rebuild_media_cache() -> anyhow::Result<Vec<Media>> {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (compatible; CacheApi/1.0)")
        .build()
        .expect("HTTP client");

    let posts = get_instagram_posts().await?;
    let mut media = posts
        .into_iter()
        .map(Into::<Media>::into)
        .collect::<Vec<_>>();

    for item in &mut media {
        let new_url =
            download_image(&client, &item.id, &item.image_url, Path::new("media")).await?;
        item.image_url = new_url;
    }

    debug!("Cache rebuilt!");
    Ok(media)
}

async fn download_image(
    client: &reqwest::Client,
    id: &str,
    url: &str,
    media_dir: &Path,
) -> anyhow::Result<String> {
    let hash = &Sha256::digest(id.as_bytes())[..16];
    let hash = hex::encode(hash);
    let ext = url
        .split('?')
        .next()
        .unwrap_or("")
        .rsplit('.')
        .next()
        .unwrap_or("jpg");

    let filename = format!("{}.{}", hash, ext);
    let dest_path = media_dir.join(&filename);

    if dest_path.exists() {
        debug!("Cache hit: {}", filename);
        return Ok(format!("media/{}", filename));
    }

    debug!("Downloading: {} -> {}", url, filename);

    let response = client.get(url).send().await?.error_for_status()?;
    let bytes = response.bytes().await?;

    fs::create_dir_all(media_dir).await?;
    let mut file = fs::File::create(&dest_path).await?;
    file.write_all(&bytes).await?;

    Ok(format!("media/{}", filename))
}
