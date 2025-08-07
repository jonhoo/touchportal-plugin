use eyre::Context;
use http_body_util::Full;
use hyper::body::Bytes;
use hyper::service::service_fn;
use hyper::{body, Request, Response};
use oauth2::basic::{BasicClient, BasicTokenResponse};
use oauth2::{reqwest, ClientSecret, RevocationUrl};
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, CsrfToken, PkceCodeChallenge, RedirectUrl, Scope,
    TokenUrl,
};
use std::time::Duration;
use touchportal_sdk::protocol::InfoMessage;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;

mod youtube_client;
use touchportal_sdk::ApiVersion;
use youtube_client::YouTubeClient;

// You can look at the generated code for a plugin using this command:
//
// ```bash
// cat "$(dirname "$(cargo check --message-format=json | jq -r 'select(.reason == "build-script-executed") | select(.package_id | contains("#touchportal-")).out_dir')")"
// ```
include!(concat!(env!("OUT_DIR"), "/entry.rs"));

const OAUTH_CLIENT_ID: &str =
    "392239669497-in1s6h0alvakffbb5bjbqjegn2m5aram.apps.googleusercontent.com";

// As per <https://developers.google.com/identity/protocols/oauth2#installed>, for an installed
// desktop application using PKCE, it's expected that the secret gets embedded, and it is _not_
// considered secret.
const OAUTH_SECRET: &str = "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";

const OAUTH_DONE: &str = "
<!DOCTYPE html>
<title>YouTube Live now authorized</title>
<h1>YouTube Live plugin authorized 🎉</h1
<p>You can now close this window.</p>
";

#[derive(Debug)]
struct Plugin {
    yt: YouTubeClient,
    tp: TouchPortalHandle,
}

impl PluginCallbacks for Plugin {}

impl Plugin {
    async fn new(
        settings: PluginSettings,
        mut outgoing: TouchPortalHandle,
        info: InfoMessage,
    ) -> eyre::Result<Self> {
        tracing::info!(version = info.tp_version_string, "paired with TouchPortal");
        tracing::debug!(settings = ?settings, "got settings");

        let (token, old) = if settings.you_tube_api_access_token.is_empty() {
            // TODO: notify

            let token = run_oauth_flow()
                .await
                .context("authorize user to YouTube")?;

            outgoing
                .set_you_tube_api_access_token(
                    serde_json::to_string(&token).expect("OAuth tokens always serialize"),
                )
                .await;

            (token, false)
        } else {
            (
                serde_json::from_str(&settings.you_tube_api_access_token)
                    .context("parse YouTube access token")?,
                true,
            )
        };

        // Test the existing token before using it
        let mut yt_client = YouTubeClient::new(token);
        let is_valid = yt_client
            .validate_token()
            .await
            .context("validate existing YouTube token")?;

        if is_valid {
            tracing::info!("YouTube token is valid, using it");
        } else {
            // TODO: notify
            tracing::info!("existing YouTube token is invalid, getting new one");

            let new_token = run_oauth_flow()
                .await
                .context("authorize user to YouTube")?;

            outgoing
                .set_you_tube_api_access_token(
                    serde_json::to_string(&new_token).expect("OAuth tokens always serialize"),
                )
                .await;

            yt_client = YouTubeClient::new(new_token);
        }

        let handle = outgoing.clone();
        tokio::spawn(async move {
            let _ = handle;
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;
                // TODO: refresh latest live video + view count?
            }
        });

        Ok(Self {
            yt: yt_client,
            tp: outgoing,
        })
    }
}

async fn run_oauth_flow() -> eyre::Result<BasicTokenResponse> {
    let csrf = CsrfToken::new_random();
    let (redirect_url, eventually_authorization_code) = setup_redirect(csrf.clone())
        .await
        .context("set up redirect endpoint")?;

    let auth_url = AuthUrl::new("https://accounts.google.com/o/oauth2/v2/auth".to_string())
        .expect("Invalid authorization endpoint URL");
    let token_url = TokenUrl::new("https://www.googleapis.com/oauth2/v3/token".to_string())
        .expect("Invalid token endpoint URL");
    let revocation_url = RevocationUrl::new("https://oauth2.googleapis.com/revoke".to_string())
        .expect("Invalid revocation endpoint URL");
    let client = BasicClient::new(ClientId::new(OAUTH_CLIENT_ID.to_string()))
        .set_client_secret(ClientSecret::new(OAUTH_SECRET.to_string()))
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

async fn setup_redirect(
    csrf: CsrfToken,
) -> eyre::Result<(
    RedirectUrl,
    impl Future<Output = eyre::Result<AuthorizationCode>>,
)> {
    // Once the user has been redirected to the redirect URL, you'll have access to the
    // authorization code. For security reasons, your code should verify that the `state`
    // parameter returned by the server matches `csrf_token`. `code` has the authorization code.
    // RedirectUrl::new("http://redirect".to_string())?
    let socket = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .context("bind to localhost")?;
    let addr = socket.local_addr().context("get local address")?;
    let url = RedirectUrl::new(format!("http://{}:{}", addr.ip(), addr.port()))
        .context("construct redirect url")?;
    let (tx, rx) = tokio::sync::oneshot::channel();
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
                    for (k, v) in form_urlencoded::parse(req.uri().query().unwrap_or("").as_bytes())
                    {
                        match &*k {
                            "state" => presented_state = Some(v),
                            "code" => presented_code = Some(v),
                            "scope" => presented_scope = Some(v),
                            _ => {}
                        }
                    }
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
                    Ok(Response::new(Full::<Bytes>::from(OAUTH_DONE)))
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

#[tokio::main]
async fn main() -> eyre::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::TRACE.into())
                .from_env_lossy(),
        )
        .without_time() // done by TouchPortal's logs
        .with_ansi(false)
        .init();

    // when run without arguments, we're running as a plugin
    if std::env::args().len() == 1 {
        Plugin::run_dynamic("127.0.0.1:12136").await?;
    } else {
        let mut token = String::new();
        if tokio::fs::try_exists("token.json").await.unwrap() {
            token = tokio::fs::read_to_string("token.json").await.unwrap();
        }
        let (tx, rx) = tokio::sync::mpsc::channel(100);
        let plugin = Plugin::new(
            PluginSettings {
                you_tube_api_access_token: token,
            },
            TouchPortalHandle(tx),
            serde_json::from_value(serde_json::json!({
                "sdkVersion": ApiVersion::V4_3,
                "tpVersionString": "4.4",
                "tpVersionCode": 4044,
                "pluginVersion": 1,
            }))
            .context("fake InfoMessage")?,
        )
        .await?;
        let json = serde_json::to_string(&plugin.yt.into_token()).unwrap();
        tokio::fs::write("token.json", &json).await.unwrap();
    }

    Ok(())
}
