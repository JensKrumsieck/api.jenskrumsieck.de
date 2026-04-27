use api::{
    auth::{auth_server, refresh_token},
    get_instagram_media,
    media::Media,
    refresh_instagram_media_cache,
};
use axum::{Json, Router, routing::get};
use dotenvy::dotenv;
use reqwest::StatusCode;
use tower_http::services::ServeDir;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() {
    dotenv().ok();
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                format!(
                    "{}=trace,tower_http=debug,axum::rejection=trace",
                    env!("CARGO_CRATE_NAME")
                )
                .into()
            }),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    //check authentication and start refresh coroutine
    auth_server().await;
    tokio::spawn(async {
        if let Err(e) = refresh_token().await {
            eprintln!("Token refresh failed: {e}");
        }
    });

    info!("Starting server...");
    let app = Router::new()
        .route("/instagram", get(instagram))
        .nest_service("/media", ServeDir::new("./media"));
    let listener = tokio::net::TcpListener::bind("0.0.0.0:1337").await.unwrap();

    tokio::spawn(async {
        if let Err(e) = refresh_instagram_media_cache().await {
            eprintln!("Media cache refresh failed: {e}");
        }
    });

    info!("Server runs at port 1337");
    axum::serve(listener, app)
        .await
        .expect("Could not start server");
}

async fn instagram() -> Result<Json<Vec<Media>>, StatusCode> {
    let media = get_instagram_media().await;
    Ok(Json(media))
}
