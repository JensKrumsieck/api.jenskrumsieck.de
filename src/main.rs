use std::{env, path::PathBuf, sync::Arc};

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
use tokio::fs;
use tower::ServiceBuilder;
use tower_http::{services::ServeDir, set_header::SetResponseHeaderLayer};
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

const TILE_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(30 * 24 * 60 * 60);

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
            .timeout(std::time::Duration::from_secs(10))
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
    if !matches!(s.as_str(), "a" | "b" | "c") {
        return (StatusCode::BAD_REQUEST, "Invalid tile subdomain").into_response();
    }
    if z > 19 {
        return (StatusCode::BAD_REQUEST, "Invalid zoom level").into_response();
    }
    let max_tile = 1u32 << z;
    if x >= max_tile {
        return (StatusCode::BAD_REQUEST, "Invalid tile coordinate").into_response();
    }
    //only plain numeric tile.png names are valid, reject anything else to keep the cache path safe
    let valid_y = y
        .strip_suffix(".png")
        .filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|n| n.parse::<u32>().ok())
        .is_some_and(|n| n < max_tile);
    if !valid_y {
        return (StatusCode::BAD_REQUEST, "Invalid tile coordinate").into_response();
    }

    let cache_path = PathBuf::from("./tile-cache")
        .join(&s)
        .join(z.to_string())
        .join(x.to_string())
        .join(&y);

    if let Ok(meta) = fs::metadata(&cache_path).await
        && let Ok(modified) = meta.modified()
        && modified.elapsed().is_ok_and(|age| age < TILE_CACHE_TTL)
        && let Ok(bytes) = fs::read(&cache_path).await
    {
        return tile_response(StatusCode::OK, "image/png", bytes);
    }

    let upstream_url = format!("https://{s}.tile.openstreetmap.org/{z}/{x}/{y}");
    let upstream_data = match state.http_client.get(upstream_url).send().await {
        Ok(resp) => resp,
        Err(e) => {
            error!("Failed to proxy request: {e:?}");
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

    if status.is_success() {
        if let Some(parent) = cache_path.parent()
            && let Err(e) = fs::create_dir_all(parent).await
        {
            error!("Failed to create tile cache dir: {e}");
        }
        if let Err(e) = fs::write(&cache_path, &bytes).await {
            error!("Failed to write tile cache: {e}");
        }
    }

    tile_response(
        StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
        &content_type,
        bytes,
    )
}

fn tile_response(status: StatusCode, content_type: &str, bytes: impl Into<axum::body::Bytes>) -> Response {
    let mut response_headers = HeaderMap::new();
    if let Ok(value) = content_type.parse() {
        response_headers.insert("content-type", value);
    }
    response_headers.insert(
        "cache-control",
        HeaderValue::from_static(if status.is_success() {
            "public, max-age=31536000"
        } else {
            "no-store"
        }),
    );

    (status, response_headers, bytes.into()).into_response()
}
