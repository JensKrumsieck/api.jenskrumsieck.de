use std::{env, sync::Arc};

use api::{
    AppState,
    auth::{auth_server, refresh_token},
    get_instagram_media,
    media::Media,
    refresh_instagram_media_cache,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue},
    response::{IntoResponse, Response},
    routing::get,
};
use dotenvy::dotenv;
use reqwest::{StatusCode, header};
use tower::ServiceBuilder;
use tower_http::{services::ServeDir, set_header::SetResponseHeaderLayer};
use tracing::{error, info};
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

    let api_host = env::var("API_HOST").expect("No Host set");
    let state = Arc::new(AppState {
        http_client: reqwest::Client::builder()
            .user_agent(format!(
                "Mozilla/5.0 (compatible; API Proxy/1.0; +{api_host})"
            ))
            .build()
            .expect("HTTP client"),
    });

    let app = Router::new()
        .route("/instagram", get(instagram))
        .route("/openstreetmap/{s}/{z}/{x}/{y}", get(openstreetmap))
        .nest_service(
            "/media",
            ServiceBuilder::new()
                .layer(SetResponseHeaderLayer::overriding(
                    header::CACHE_CONTROL,
                    HeaderValue::from_static("public, max-age=315360000, immutable"),
                ))
                .service(ServeDir::new("./media")),
        )
        .with_state(state);
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

async fn openstreetmap(
    State(state): State<Arc<AppState>>,
    Path((s, z, x, y)): Path<(String, u32, u32, String)>,
) -> Response {
    let upstream_url = format!("https://{s}.tile.openstreetmap.de/{z}/{x}/{y}");
    let upstream_data = match state.http_client.get(upstream_url).send().await {
        Ok(resp) => resp,
        Err(e) => {
            error!("Failed to proxy request: {e}");
            return (StatusCode::BAD_GATEWAY, "Failed to fetch tile").into_response();
        }
    };

    let status = upstream_data.status();
    //get content type header (should be png anyways..)
    let content_type = upstream_data
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("image/png")
        .to_owned();

    let bytes = match upstream_data.bytes().await {
        Ok(b) => b,
        Err(e) => {
            error!("Failed to read tile body: {e}");
            return (StatusCode::BAD_GATEWAY, "Failed to read tile body").into_response();
        }
    };
    let mut response_headers = HeaderMap::new();
    response_headers.insert("content-type", content_type.parse().unwrap());
    response_headers.insert("cache-control", "public, max-age=864000".parse().unwrap());

    (
        StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::OK),
        response_headers,
        bytes,
    )
        .into_response()
}
