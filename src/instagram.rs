use crate::{INSTAGRAM_GRAPH_ENDPOINT, auth::get_token};
use serde::{Deserialize, Serialize};

pub(crate) async fn get_instagram_posts() -> anyhow::Result<Vec<InstagramMediaData>> {
    let http_client = reqwest::ClientBuilder::new()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let token = get_token("instagram_token")?;
    let media_data: InstagramMediaResponse = http_client
        .get(format!("{INSTAGRAM_GRAPH_ENDPOINT}/me/media"))
        .query(&[
            (
                "fields",
                "caption,media_type,media_url,permalink,thumbnail_url,timestamp,alt_text",
            ),
            ("limit", "12"),
            ("access_token", &token),
        ])
        .send()
        .await?
        .json()
        .await?;
    Ok(media_data.data)
}

#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct InstagramMediaResponse {
    pub data: Vec<InstagramMediaData>,
    next: Option<String>,
    previous: Option<String>,
}
#[derive(Serialize, Deserialize, Debug)]
pub(crate) enum MediaType {
    #[serde(rename = "CAROUSEL_ALBUM")]
    Carousel,
    #[serde(rename = "VIDEO")]
    Reel,
    #[serde(rename = "IMAGE")]
    Image,
}

#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct InstagramMediaData {
    pub id: String,
    pub media_type: MediaType,
    pub permalink: Option<String>,
    pub media_url: Option<String>,
    pub thumbnail_url: Option<String>,
    pub caption: Option<String>,
    pub alt_text: Option<String>,
    pub timestamp: String,
}
