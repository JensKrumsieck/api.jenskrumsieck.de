use crate::{
    SECRETS,
    auth::instagram::{InstagramClient, instagram_login},
};
use axum::{
    Router,
    extract::{Query, State},
    response::Html,
    routing::get,
};
use oauth2::{AccessToken, CsrfToken, reqwest};
use reqwest::StatusCode;
use serde::Deserialize;
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;
use tracing::info;
pub mod instagram;

struct AuthAppState {
    instagram: InstagramClient,
    csrf: CsrfToken,
    shutdown_tx: Mutex<Option<oneshot::Sender<()>>>,
}

pub async fn auth_server() {
    if get_token("instagram_token").is_err()
        || !instagram::token_is_valid()
            .await
            .expect("Could not check validity")
    {
        let (instagram, csrf) = instagram_login();
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

        let state = Arc::new(AuthAppState {
            instagram,
            csrf,
            shutdown_tx: Mutex::new(Some(shutdown_tx)),
        });

        let app = Router::new()
            .route("/auth/instagram", get(auth_callback))
            .with_state(state);
        let listener = tokio::net::TcpListener::bind("0.0.0.0:1337").await.unwrap();
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                shutdown_rx.await.ok();
            })
            .await
            .unwrap();
    } else {
        info!("Authentication is still valid, skipping auth server")
    }
}

#[derive(Deserialize, Debug)]
struct AuthQuery {
    code: String,
    state: String,
}

async fn auth_callback(
    State(state): State<Arc<AuthAppState>>,
    Query(auth): Query<AuthQuery>,
) -> Result<Html<String>, StatusCode> {
    if auth.state != *state.csrf.secret() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let token = instagram::get_access_token(&state.instagram, auth.code)
        .await
        .map_err(|e| {
            info!("{e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    save_token("instagram_token", &token).map_err(|e| {
        info!("{e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if let Ok(mut guard) = state.shutdown_tx.lock()
        && let Some(tx) = guard.take()
    {
        let _ = tx.send(());
    }

    Ok(Html(
        "<h1>Auth complete! You can close this tab.</h1>".to_string(),
    ))
}

pub(crate) fn save_token(store: &str, token: &AccessToken) -> anyhow::Result<()> {
    let mut secrets = SECRETS.lock().expect("Failed to lock secrets");
    secrets.set(store, token.clone().into_secret());
    secrets.save()?;
    Ok(())
}

pub fn get_token(store: &str) -> anyhow::Result<String> {
    let secrets = SECRETS.lock().expect("Failed to lock secrets");
    let token = secrets.get(store)?;
    Ok(token)
}

pub async fn refresh_token() -> anyhow::Result<()> {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(20 * 24 * 60 * 60));
    loop {
        interval.tick().await;
        let current_token = get_token("instagram_token")?;
        instagram::refresh_access_token(current_token).await?;
    }
}
