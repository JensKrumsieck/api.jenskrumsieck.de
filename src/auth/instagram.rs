use crate::{
    INSTAGRAM_GRAPH_ENDPOINT,
    auth::{get_token, save_token},
};
use oauth2::{
    AccessToken, AuthType, AuthUrl, AuthorizationCode, Client, ClientId, ClientSecret, CsrfToken,
    EmptyExtraTokenFields, EndpointNotSet, EndpointSet, ErrorResponse, ErrorResponseType,
    ExtraTokenFields, RedirectUrl, RefreshToken, Scope, StandardRevocableToken, TokenResponse,
    TokenType, TokenUrl,
    basic::{
        BasicErrorResponseType, BasicRevocationErrorResponse, BasicTokenIntrospectionResponse,
        BasicTokenResponse,
    },
};
use serde::{Deserialize, Serialize};
use std::{
    env,
    fmt::{Display, Formatter},
    time::Duration,
};
use tracing::{debug, info};

pub(crate) type InstagramClient<
    HasAuthUrl = EndpointSet,
    HasDeviceAuthUrl = EndpointNotSet,
    HasIntrospectiveUrl = EndpointNotSet,
    HasRevocationUrl = EndpointNotSet,
    HasTokenUrl = EndpointSet,
> = Client<
    InstagramErrorResponse,
    InstagramTokenResponse,
    BasicTokenIntrospectionResponse,
    StandardRevocableToken,
    BasicRevocationErrorResponse,
    HasAuthUrl,
    HasDeviceAuthUrl,
    HasIntrospectiveUrl,
    HasRevocationUrl,
    HasTokenUrl,
>;

pub(crate) fn instagram_login() -> (InstagramClient, CsrfToken) {
    let client = instagram_client().expect("Could not load instagram oauth2 client");
    let (authorize_url, csrf_state) = client
        .authorize_url(CsrfToken::new_random)
        .add_scope(Scope::new("instagram_business_basic".to_string()))
        .url();

    info!("To use the Instagram-Login please follow this URL: {authorize_url}");
    (client, csrf_state)
}

pub(crate) fn instagram_client() -> anyhow::Result<InstagramClient> {
    let client_id = env::var("INSTAGRAM_APP_ID")?;
    let client_secret = env::var("INSTAGRAM_APP_SECRET")?;
    let redirect_url = env::var("INSTAGRAM_REDIRECT_URI")?;
    Ok(InstagramClient::new(ClientId::new(client_id))
        .set_client_secret(ClientSecret::new(client_secret))
        .set_auth_uri(AuthUrl::new(
            "https://www.instagram.com/oauth/authorize".to_string(),
        )?)
        .set_token_uri(TokenUrl::new(
            "https://api.instagram.com/oauth/access_token".to_string(),
        )?)
        .set_redirect_uri(RedirectUrl::new(redirect_url)?)
        .set_auth_type(AuthType::RequestBody))
}

pub(crate) async fn get_access_token(
    client: &InstagramClient,
    code: String,
) -> anyhow::Result<AccessToken> {
    let http_client = oauth2::reqwest::ClientBuilder::new()
        .redirect(oauth2::reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let token: InstagramTokenResponse = client
        .exchange_code(AuthorizationCode::new(code))
        .request_async(&http_client)
        .await?;
    debug!("Got instgram short lived token");

    let token = token.access_token();
    get_long_lived_access_token(token).await
}

async fn get_long_lived_access_token(token: &AccessToken) -> anyhow::Result<AccessToken> {
    let client_secret = env::var("INSTAGRAM_APP_SECRET")?;
    let http_client = reqwest::ClientBuilder::new()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let token_response: BasicTokenResponse = http_client
        .get("https://graph.instagram.com/access_token")
        .query(&[
            ("grant_type", "ig_exchange_token"),
            ("client_secret", client_secret.as_str()),
            ("access_token", token.secret()),
        ])
        .send()
        .await?
        .json()
        .await?;

    debug!("Got instagram long lived token");
    Ok(token_response.access_token().clone())
}

pub(crate) async fn refresh_access_token(token: String) -> anyhow::Result<()> {
    let client_secret = env::var("INSTAGRAM_APP_SECRET")?;
    let http_client = reqwest::ClientBuilder::new()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let token_response = http_client
        .get("https://graph.instagram.com/refresh_access_token")
        .query(&[
            ("grant_type", "ig_refresh_token"),
            ("client_secret", client_secret.as_str()),
            ("access_token", &token),
        ])
        .send()
        .await?;
    let token_response: BasicTokenResponse = token_response.json().await?;

    debug!("refreshed the token");
    save_token("instagram_token", token_response.access_token())?;
    Ok(())
}

pub(crate) async fn token_is_valid() -> anyhow::Result<bool> {
    let token = get_token("instagram_token")?;

    let http_client = reqwest::ClientBuilder::new()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    let response = http_client
        .get(format!("{INSTAGRAM_GRAPH_ENDPOINT}/me"))
        .query(&[("fields", "user_id,username"), ("access_token", &token)])
        .send()
        .await?;

    let data: serde_json::Value = response.json().await?;
    if let Some(data) = data.get("user_id") {
        debug!("Login data valid: {data}");
        return Ok(true);
    }
    debug!("{data}");
    Ok(false)
}

type InstagramTokenResponse =
    InstagramTokenCustomResponse<EmptyExtraTokenFields, InstagramTokenType>;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct InstagramTokenCustomResponse<EF, TT>
where
    EF: ExtraTokenFields,
    TT: TokenType,
{
    access_token: AccessToken,
    #[serde(bound = "TT: TokenType")]
    #[serde(default)]
    #[serde(deserialize_with = "oauth2::helpers::deserialize_untagged_enum_case_insensitive")]
    token_type: Option<TT>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expires_in: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    refresh_token: Option<RefreshToken>,
    #[serde(rename = "permissions")]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    scopes: Option<Vec<Scope>>,

    #[serde(bound = "EF: ExtraTokenFields")]
    #[serde(flatten)]
    extra_fields: EF,
}

impl<EF> TokenResponse for InstagramTokenCustomResponse<EF, InstagramTokenType>
where
    EF: ExtraTokenFields,
{
    type TokenType = InstagramTokenType;

    fn access_token(&self) -> &AccessToken {
        &self.access_token
    }

    fn token_type(&self) -> &Self::TokenType {
        &InstagramTokenType::Default
    }

    fn expires_in(&self) -> Option<std::time::Duration> {
        self.expires_in.map(Duration::from_secs)
    }

    fn refresh_token(&self) -> Option<&RefreshToken> {
        self.refresh_token.as_ref()
    }

    fn scopes(&self) -> Option<&Vec<Scope>> {
        self.scopes.as_ref()
    }
}

type InstagramErrorResponse = InstagramErrorCustomResponse<BasicErrorResponseType>;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct InstagramErrorCustomResponse<T: ErrorResponseType> {
    #[serde(bound = "T: ErrorResponseType")]
    pub(crate) error_type: T,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) code: Option<u32>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error_message: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error_uri: Option<String>,
}

impl<T: ErrorResponseType> InstagramErrorCustomResponse<T> {
    pub fn error(&self) -> &T {
        &self.error_type
    }
    pub fn error_description(&self) -> Option<&String> {
        self.error_message.as_ref()
    }
    pub fn error_uri(&self) -> Option<&String> {
        self.error_uri.as_ref()
    }
}

impl<T> ErrorResponse for InstagramErrorCustomResponse<T> where
    T: ErrorResponseType + Display + 'static
{
}

impl<TE> Display for InstagramErrorCustomResponse<TE>
where
    TE: ErrorResponseType + Display,
{
    fn fmt(&self, f: &mut Formatter) -> Result<(), std::fmt::Error> {
        let mut formatted = self.error().to_string();

        if let Some(error_description) = self.error_description() {
            formatted.push_str(": ");
            formatted.push_str(error_description);
        }

        if let Some(error_uri) = self.error_uri() {
            formatted.push_str(" (see ");
            formatted.push_str(error_uri);
            formatted.push(')');
        }

        write!(f, "{formatted}")
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum InstagramTokenType {
    #[default]
    Default,
}

impl TokenType for InstagramTokenType {}
