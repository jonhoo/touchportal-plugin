#![allow(dead_code)]

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
use std::collections::HashMap;
use std::time::Duration;
use touchportal_sdk::protocol::{CreateNotificationCommand, InfoMessage};
use touchportal_sdk::ApiVersion;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;
use youtube_client::YouTubeClient;

mod youtube_client;

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

const OAUTH_DONE: &str = include_str!("../oauth_success.html");

#[derive(Debug)]
struct Channel {
    name: String,
    yt: YouTubeClient,
}

#[derive(Debug)]
struct Plugin {
    yt: HashMap<String, Channel>,
    tp: TouchPortalHandle,
    current_channel: Option<String>,
}

impl PluginCallbacks for Plugin {
    async fn on_ytl_live_stream_toggle(
        &mut self,
        _mode: protocol::ActionInteractionMode,
        _ytl_channel: ChoicesForYtlChannel,
        _ytl_stream: ChoicesForYtlStream,
    ) -> eyre::Result<()> {
        todo!()
    }

    async fn on_select_ytl_channel_in_ytl_live_stream_toggle(
        &mut self,
        instance: String,
        selected: ChoicesForYtlChannel,
    ) -> eyre::Result<()> {
        let ChoicesForYtlChannel::Dynamic(selected) = selected else {
            return Ok(());
        };
        let selected = selected
            .rsplit_once(" - ")
            .expect("all options are formatted this way")
            .1;
        self.current_channel = Some(selected.to_string());
        let Some(channel) = self.yt.get_mut(selected) else {
            eyre::bail!("user selected unknown channel '{selected}'");
        };

        let streams = channel
            .yt
            .list_live_streams()
            .await
            .context("list live streams")?
            .into_iter()
            .map(|stream| format!("{} - {}", stream.snippet.title, stream.id));
        self.tp
            .update_choices_in_specific_ytl_stream(instance, streams)
            .await;

        Ok(())
    }

    async fn on_select_ytl_stream_in_ytl_live_stream_toggle(
        &mut self,
        _instance: String,
        _selected: ChoicesForYtlStream,
    ) -> eyre::Result<()> {
        Ok(())
    }
}

impl Plugin {
    async fn new(
        settings: PluginSettings,
        mut outgoing: TouchPortalHandle,
        info: InfoMessage,
    ) -> eyre::Result<Self> {
        tracing::info!(version = info.tp_version_string, "paired with TouchPortal");
        tracing::debug!(settings = ?settings, "got settings");

        let (tokens, is_old) = if settings.you_tube_api_access_tokens.is_empty()
            || settings.you_tube_api_access_tokens == "[]"
        {
            outgoing
                .notify(
                    CreateNotificationCommand::builder()
                        .notification_id("ytl_auth")
                        .title("Check your browser")
                        .message(
                            "You need to authenticate to YouTube \
                            to give access to your channel.",
                        )
                        .build()
                        .unwrap(),
                )
                .await;

            let token = run_oauth_flow()
                .await
                .context("authorize user to YouTube")?;
            let tokens = vec![token];

            outgoing
                .set_you_tube_api_access_tokens(
                    serde_json::to_string(&tokens).expect("OAuth tokens always serialize"),
                )
                .await;

            (tokens, false)
        } else {
            (
                serde_json::from_str(&settings.you_tube_api_access_tokens)
                    .context("parse YouTube access token")?,
                true,
            )
        };

        // Test the existing token before using it
        let mut yt_clients = Vec::new();
        for token in tokens {
            let mut client = YouTubeClient::new(token);
            let is_valid = client
                .validate_token()
                .await
                .context("validate existing YouTube token")?;

            if is_valid {
                tracing::info!("YouTube token is valid, using it");
            } else if is_old {
                outgoing
                    .notify(
                        CreateNotificationCommand::builder()
                            .notification_id("ytl_reauth")
                            .title("Check your browser")
                            .message(
                                "You need to authenticate to YouTube \
                                to re-authorize access to your channel.",
                            )
                            .build()
                            .unwrap(),
                    )
                    .await;
                tracing::info!("existing YouTube token is invalid, getting new one");

                let new_token = run_oauth_flow()
                    .await
                    .context("authorize user to YouTube")?;

                client = YouTubeClient::new(new_token);
            } else {
                eyre::bail!("freshly minted YouTube token is invalid");
            }

            yt_clients.push(client);
        }

        let mut client_by_channel = HashMap::new();
        for client in yt_clients {
            let channels = client.list_my_channels().await.context("list channels")?;
            for channel in channels {
                client_by_channel.insert(
                    channel.id,
                    Channel {
                        name: channel.snippet.title,
                        yt: client.clone(),
                    },
                );
            }
        }

        // TODO: keep a state that reflects the current stream state for every known stream?

        // TODO: event when a stream becomes active or inactive

        // Now we actually know what channels the user can select between!
        outgoing
            .update_choices_in_ytl_channel(
                client_by_channel
                    .iter()
                    .map(|(id, c)| format!("{} - {id}", c.name)),
            )
            .await;

        for client in client_by_channel.values_mut() {
            dbg!(client.yt.list_live_streams().await.unwrap());
            dbg!(client.yt.list_live_broadcasts().await.unwrap());
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
            yt: client_by_channel,
            tp: outgoing,
            current_channel: None,
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
                    // TODO
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
        let mut tokens = String::new();
        if tokio::fs::try_exists("tokens.json").await.unwrap() {
            tokens = tokio::fs::read_to_string("tokens.json").await.unwrap();
        }
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let plugin = Plugin::new(
            PluginSettings {
                you_tube_api_access_tokens: tokens,
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
        let json =
            serde_json::to_string(&plugin.yt.values().map(|c| c.yt.token()).collect::<Vec<_>>())
                .unwrap();
        tokio::fs::write("tokens.json", &json).await.unwrap();
    }

    Ok(())
}
