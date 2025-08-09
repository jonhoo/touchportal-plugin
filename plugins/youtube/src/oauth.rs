//! OAuth 2.0 management for YouTube API authentication.
//!
//! This module encapsulates all OAuth-related operations for authenticating with the YouTube API,
//! including initial user authorization, token refresh, and secure handling of authorization flows.

use eyre::Context;
use http_body_util::Full;
use hyper::body::Bytes;
use hyper::service::service_fn;
use hyper::{body, Request, Response};
use oauth2::basic::{BasicClient, BasicTokenResponse};
use oauth2::{reqwest, ClientSecret, RevocationUrl, TokenResponse};
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, CsrfToken, PkceCodeChallenge, RedirectUrl, Scope,
    TokenUrl,
};
use std::future::Future;

/// Google OAuth2 token endpoint URL used for both initial authentication and token refresh
const TOKEN_URL: &str = "https://www.googleapis.com/oauth2/v3/token";

/// Manages OAuth 2.0 authentication flows for YouTube API access.
///
/// The OAuthManager encapsulates all OAuth operations, providing a consistent interface
/// for both initial user authentication and token refresh operations. It maintains
/// the OAuth client configuration and handles the security aspects of the authorization flow.
#[derive(Debug, Clone)]
pub(crate) struct OAuthManager {
    client_id: &'static str,
    client_secret: &'static str,
    oauth_done_html: &'static str,
}

impl OAuthManager {
    /// Creates a new OAuth manager with the specified credentials.
    ///
    /// # Arguments
    ///
    /// * `client_id` - The OAuth client ID for the YouTube API application
    /// * `client_secret` - The OAuth client secret (embedded for installed applications)
    /// * `oauth_done_html` - HTML content to display after successful authorization
    pub(crate) fn new(
        client_id: &'static str,
        client_secret: &'static str,
        oauth_done_html: &'static str,
    ) -> Self {
        Self {
            client_id,
            client_secret,
            oauth_done_html,
        }
    }

    /// Performs a complete OAuth 2.0 authorization flow to obtain a new access token.
    ///
    /// This method initiates the full OAuth flow, including:
    /// 1. Opening the user's browser for authorization
    /// 2. Setting up a local HTTP server to receive the authorization callback
    /// 3. Exchanging the authorization code for an access token
    ///
    /// # Panics
    ///
    /// Panics if hardcoded OAuth endpoint URLs are malformed (this should never happen
    /// in practice as the URLs are static and validated).
    pub(crate) async fn authenticate(&self) -> eyre::Result<BasicTokenResponse> {
        let csrf = CsrfToken::new_random();
        let (redirect_url, eventually_authorization_code) = self
            .setup_redirect(csrf.clone())
            .await
            .context("set up redirect endpoint")?;

        let auth_url = AuthUrl::new("https://accounts.google.com/o/oauth2/v2/auth".to_string())
            .expect("Invalid authorization endpoint URL");
        let token_url = TokenUrl::new(TOKEN_URL.to_string()).expect("Invalid token endpoint URL");
        let revocation_url = RevocationUrl::new("https://oauth2.googleapis.com/revoke".to_string())
            .expect("Invalid revocation endpoint URL");
        let client = BasicClient::new(ClientId::new(self.client_id.to_string()))
            .set_client_secret(ClientSecret::new(self.client_secret.to_string()))
            .set_auth_uri(auth_url)
            .set_token_uri(token_url)
            .set_redirect_uri(redirect_url)
            .set_revocation_url(revocation_url);

        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
        let (auth_url, _csrf_token) = client
            // We never re-use the CSRF since we only go through the flow exactly once.
            .authorize_url(move || csrf.clone())
            .add_scope(Scope::new(
                "https://www.googleapis.com/auth/youtube".to_string(),
            ))
            .set_pkce_challenge(pkce_challenge)
            .url();

        tracing::info!(url = %auth_url, "asking user to follow OAuth flow");
        webbrowser::open(auth_url.as_ref()).context("open user's browser")?;
        let authorization_code = eventually_authorization_code
            .await
            .context("await user authorization code")?;

        let http_client = reqwest::ClientBuilder::new()
            // SSRF no thank you.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("building reqwest client should not fail");
        let token_result = client
            .exchange_code(authorization_code)
            .set_pkce_verifier(pkce_verifier)
            .request_async(&http_client)
            .await
            .context("exchange authorization code with access token")?;

        Ok(token_result)
    }

    /// Attempts to refresh an existing OAuth token using its refresh token.
    ///
    /// This method provides a seamless way to extend token lifetime without requiring
    /// user interaction. It checks for the presence of a refresh token and attempts
    /// to exchange it for a new access token.
    ///
    /// # Arguments
    ///
    /// * `token` - The existing [`BasicTokenResponse`] containing the refresh token
    ///
    /// # Returns
    ///
    /// * `Ok(Some(new_token))` - Refresh succeeded, new token is available
    /// * `Ok(None)` - Refresh failed or no refresh token available
    /// * `Err(_)` - Network or other error occurred during refresh attempt
    ///
    /// # Token Lifecycle
    ///
    /// When refresh fails, the token should be considered invalid and the user
    /// should be prompted to re-authenticate using [`Self::authenticate`].
    ///
    /// # Panics
    ///
    /// Panics if hardcoded OAuth endpoint URLs are malformed or if the HTTP client
    /// cannot be built with the specified configuration (both should never happen
    /// in practice).
    pub(crate) async fn refresh_token(
        &self,
        token: BasicTokenResponse,
    ) -> eyre::Result<Option<BasicTokenResponse>> {
        let Some(refresh_token) = token.refresh_token() else {
            tracing::warn!("no refresh token available, cannot refresh");
            return Ok(None);
        };

        tracing::debug!("attempting to refresh OAuth token");

        // Create a minimal OAuth client for token refresh (no redirect URL needed)
        let client = BasicClient::new(ClientId::new(self.client_id.to_string()))
            .set_client_secret(ClientSecret::new(self.client_secret.to_string()))
            .set_token_uri(
                TokenUrl::new(TOKEN_URL.to_string()).expect("Invalid token endpoint URL"),
            );

        let http_client = reqwest::ClientBuilder::new()
            // SSRF no thank you.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("building reqwest client should not fail");

        match client
            .exchange_refresh_token(refresh_token)
            .request_async(&http_client)
            .await
        {
            Ok(new_token) => {
                tracing::debug!("successfully refreshed OAuth token");
                Ok(Some(new_token))
            }
            Err(ref e @ oauth2::RequestTokenError::ServerResponse(ref sr))
                if matches!(
                    sr.error(),
                    oauth2::basic::BasicErrorResponseType::InvalidGrant
                ) =>
            {
                tracing::warn!("OAuth refresh token considered invalid grant: {}", e);
                Ok(None)
            }
            Err(e) => Err(e).context("exchange refresh token"),
        }
    }

    /// Sets up a local HTTP server to receive the OAuth authorization callback.
    ///
    /// Creates a temporary HTTP server on a random local port to handle the OAuth
    /// redirect after user authorization. The server validates the CSRF token and
    /// extracts the authorization code from the callback.
    ///
    /// # Arguments
    ///
    /// * `csrf` - CSRF token for state validation
    ///
    /// # Returns
    ///
    /// A tuple containing:
    /// - The redirect URL to use in the OAuth flow
    /// - A future that resolves to the authorization code when the callback is received
    async fn setup_redirect(
        &self,
        csrf: CsrfToken,
    ) -> eyre::Result<(
        RedirectUrl,
        impl Future<Output = eyre::Result<AuthorizationCode>>,
    )> {
        // Once the user has been redirected to the redirect URL, you'll have access to the
        // authorization code. For security reasons, your code should verify that the `state`
        // parameter returned by the server matches `csrf_token`. `code` has the authorization code.
        let socket = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .context("bind to localhost")?;
        let addr = socket.local_addr().context("get local address")?;
        let url = RedirectUrl::new(format!("http://{}:{}", addr.ip(), addr.port()))
            .context("construct redirect url")?;
        let (tx, rx) = tokio::sync::oneshot::channel();
        let oauth_done = self.oauth_done_html;
        tokio::spawn(async move {
            let r = async move {
                let (conn, _) = socket.accept().await.context("accept")?;
                let conn = hyper_util::rt::TokioIo::new(conn);
                let (got, mut gotten) = tokio::sync::mpsc::channel(1);
                let service = service_fn(move |req: Request<body::Incoming>| {
                    let csrf = csrf.clone();
                    let got = got.clone();
                    async move {
                        let mut presented_state = None;
                        let mut presented_code = None;
                        // space-separated
                        let mut presented_scope = None;
                        for (k, v) in
                            form_urlencoded::parse(req.uri().query().unwrap_or("").as_bytes())
                        {
                            match &*k {
                                "state" => presented_state = Some(v),
                                "code" => presented_code = Some(v),
                                "scope" => presented_scope = Some(v),
                                _ => {}
                            }
                        }
                        // TODO: check that the user granted the scope(s) we requested
                        let _ = presented_scope;
                        if presented_state.as_deref() != Some(csrf.secret().as_str()) {
                            return Err("invalid csrf token");
                        }
                        let Some(code) = presented_code else {
                            return Err("no authorization code found");
                        };
                        let code = AuthorizationCode::new(code.into_owned());
                        got.send(code)
                            .await
                            .expect("channel won't be closed until server exit");
                        Ok(Response::new(Full::<Bytes>::from(oauth_done)))
                    }
                });
                let mut serve = std::pin::pin!(
                    hyper::server::conn::http1::Builder::new().serve_connection(conn, service)
                );

                tokio::select! {
                    exit = &mut serve => {
                        if let Err(e) = exit {
                            Err(e).context("redirect server got bad request")
                        } else {
                            eyre::bail!("redirect server exit prematurely");
                        }
                    }
                    code = gotten.recv() => {
                        serve
                            .graceful_shutdown();
                        let code = code.expect("channel won't be closed until service_fn is dropped");
                        Ok(code)
                    }
                }
            };
            let _ = tx.send(r.await);
        });
        Ok((url, async move {
            rx.await.context("redirect future dropped prematurely")?
        }))
    }
}
