#![warn(clippy::pedantic)]

use std::{collections::HashMap, fmt::Display, str::FromStr, sync::OnceLock, time::Duration};

use aformat::{aformat, astr};
use arrayvec::ArrayString;
use axum::{extract::Path, http::HeaderValue};
use models::{RawPatreonError, RawPatreonRateLimit};
use reqwest::StatusCode;
use serde_cow::CowStr;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod macros;
mod models;

#[derive(Clone, Copy)]
enum PatreonTier {
    Basic,
    Extra,
}

#[derive(serde::Serialize, Clone, Copy)]
struct PatreonTierInfo {
    tier: u8,
    entitled_servers: u8,
}

impl PatreonTierInfo {
    fn fake() -> Self {
        Self {
            tier: u8::MAX,
            entitled_servers: u8::MAX,
        }
    }
}

impl From<PatreonTier> for PatreonTierInfo {
    fn from(tier: PatreonTier) -> Self {
        Self {
            tier: 0, // Future tiers with higher privileges
            entitled_servers: match tier {
                PatreonTier::Basic => 2,
                PatreonTier::Extra => 5,
            },
        }
    }
}

#[derive(serde::Deserialize, serde::Serialize, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(transparent)]
struct DiscordUserId(u64);

#[derive(serde::Deserialize)]
struct Config {
    #[serde(deserialize_with = "deserialize_from_str")]
    campaign_id: u64,
    #[serde(deserialize_with = "deserialize_from_str")]
    basic_tier_id: u32,
    #[serde(deserialize_with = "deserialize_from_str")]
    extra_tier_id: u32,
    bind_address: Option<std::net::SocketAddr>,
    #[serde(default)]
    preset_members: Vec<DiscordUserId>,
    #[serde(deserialize_with = "add_bearer")]
    creator_access_token: HeaderValue,
}

fn deserialize_from_str<'de, D, V>(deserializer: D) -> Result<V, D::Error>
where
    D: serde::Deserializer<'de>,
    <V as FromStr>::Err: Display,
    V: FromStr,
{
    use serde::{de::Error, Deserialize};

    let value_str = <CowStr<'de>>::deserialize(deserializer)?;
    value_str.0.parse().map_err(D::Error::custom)
}

fn add_bearer<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<HeaderValue, D::Error> {
    let mut token: String = serde::Deserialize::deserialize(deserializer)?;
    token.insert_str(0, "Bearer ");

    HeaderValue::try_from(token).map_err(serde::de::Error::custom)
}

struct State {
    members: std::sync::RwLock<HashMap<DiscordUserId, PatreonTierInfo>>,
    refresh_task: tokio::sync::mpsc::Sender<()>,
    reqwest: reqwest::Client,
    config: Config,
}

static STATE: OnceLock<State> = OnceLock::new();

#[derive(serde::Deserialize)]
struct FetchMember {
    member_id: DiscordUserId,
}

async fn fetch_member(Path(payload): Path<FetchMember>) -> impl axum::response::IntoResponse {
    let state = STATE.get().unwrap();
    let members = state.members.read().expect("poison");

    axum::Json(members.get(&payload.member_id).copied())
}

async fn fetch_members() -> impl axum::response::IntoResponse {
    let state = STATE.get().unwrap();
    let members = state.members.read().expect("poison");

    axum::Json(members.clone())
}

async fn refresh_members() {
    let state = STATE.get().unwrap();
    state.refresh_task.send(()).await.unwrap();
}

#[tokio::main]
async fn main() -> Result<(), StartupError> {
    let fmt_layer = tracing_subscriber::fmt::layer();
    let filter = tracing_subscriber::filter::LevelFilter::from_str(
        &std::env::var("LOG_LEVEL").unwrap_or_else(|_| String::from("INFO")),
    )?;

    tracing_subscriber::registry()
        .with(fmt_layer)
        .with(filter)
        .init();

    let mut config: Config = toml::from_str(&std::fs::read_to_string("config.toml")?)?;
    let bind_address = config.bind_address.take().unwrap();

    let reqwest = reqwest::Client::builder()
        .user_agent("Discord-TTS/patreon-service")
        .build()?;

    let state = State {
        config,
        reqwest,
        members: std::sync::RwLock::new(HashMap::new()),
        refresh_task: {
            let (tx, mut rx) = tokio::sync::mpsc::channel(1);

            tokio::spawn(async move {
                loop {
                    let res = tokio::time::timeout(Duration::from_secs(60 * 60), rx.recv()).await;

                    if res.as_ref().map(Option::is_none).unwrap_or(false) {
                        break;
                    }

                    match fill_members().await {
                        Ok(len) => tracing::info!("Refreshed {len} members"),
                        Err(err) => tracing::error!("{err:?}"),
                    }
                }
            });
            tx
        },
    };

    STATE.set(state).is_err().then(|| unreachable!());

    fill_members().await?;

    let app = axum::Router::new()
        .route("/members/{member_id}", axum::routing::get(fetch_member))
        .route("/refresh", axum::routing::post(refresh_members))
        .route("/members", axum::routing::get(fetch_members));

    tracing::info!("Binding to {bind_address}!");

    let listener = tokio::net::TcpListener::bind(bind_address).await?;
    axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(async { drop(tokio::signal::ctrl_c().await) })
        .await?;

    Ok(())
}

fn base_url() -> ArrayString<37> {
    astr!("https://www.patreon.com/api/oauth2/v2")
}

fn check_tier(member: &models::RawPatreonMember, tier_id: u32) -> bool {
    member
        .relationships
        .currently_entitled_tiers
        .data
        .iter()
        .any(|tier| tier.id == tier_id)
}

fn get_member_tier(
    config: &Config,
    member: &models::RawPatreonMember,
    user: &models::RawPatreonUser,
) -> Option<(DiscordUserId, Option<PatreonTier>)> {
    let socials = user.attributes.social_connections.as_ref()?;
    let user_id = socials.discord.as_ref()?.user_id.as_ref()?;

    let tier = if check_tier(member, config.extra_tier_id) {
        Some(PatreonTier::Extra)
    } else if check_tier(member, config.basic_tier_id) {
        Some(PatreonTier::Basic)
    } else {
        None
    };

    Some((DiscordUserId(user_id.0), tier))
}

async fn handle_rate_limit(resp: reqwest::Response) -> Result<(), FillError> {
    let resp_text = resp.text().await?;
    let [RawPatreonRateLimit {
        retry_after_seconds,
    }] = serde_json::from_str::<RawPatreonError>(&resp_text)?.errors;

    tracing::info!("Rate limited, waiting for {retry_after_seconds} seconds");
    tokio::time::sleep(Duration::from_secs(retry_after_seconds.into())).await;
    Ok(())
}

async fn fill_members() -> Result<usize, FillError> {
    let state = STATE.get().unwrap();

    let reqwest = &state.reqwest;
    let mut url = reqwest::Url::parse(&aformat!(
        "{}/campaigns/{}/members",
        base_url(),
        state.config.campaign_id
    ))?;

    url.query_pairs_mut()
        .append_pair("fields[user]", "social_connections")
        .append_pair("include", "user,currently_entitled_tiers")
        .finish();

    let mut next_cursor = Some(ArrayString::new());
    let headers = reqwest::header::HeaderMap::from_iter([(
        reqwest::header::AUTHORIZATION,
        state.config.creator_access_token.clone(),
    )]);

    let mut members = {
        let members = state.members.read().expect("poison");
        HashMap::with_capacity(members.len())
    };

    while let Some(cursor) = next_cursor {
        let resp = loop {
            let mut url = url.clone();
            url.query_pairs_mut().append_pair("page[cursor]", &cursor);

            let resp = reqwest.get(url).headers(headers.clone()).send().await?;
            match resp.status() {
                StatusCode::OK => break resp.text().await?,
                StatusCode::TOO_MANY_REQUESTS => handle_rate_limit(resp).await?,
                _ => return Err(resp.error_for_status().unwrap_err().into()),
            }
        };

        let resp: models::RawPatreonResponse = serde_json::from_str(&resp)?;

        members.extend(resp.data.into_iter().filter_map(|member| {
            let user_id = &member.relationships.user.data.id;
            let user = resp.included.iter().find(|u| &u.id == user_id).unwrap();

            get_member_tier(&state.config, &member, user).and_then(|(discord_id, tier)| {
                tier.map(|tier| (discord_id, PatreonTierInfo::from(tier)))
            })
        }));

        next_cursor = resp
            .meta
            .pagination
            .cursors
            .and_then(|cursors| cursors.next);
    }

    let preset_members = state.config.preset_members.iter();
    members.extend(preset_members.map(|id| (*id, PatreonTierInfo::fake())));
    members.shrink_to_fit();

    let len = members.len();
    *state.members.write().expect("poison") = members;
    Ok(len)
}

#[derive(Debug, thiserror::Error)]
enum FillError {
    #[error("JSON Error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("Reqwest Error: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("URL Error: {0}")]
    Url(#[from] url::ParseError),
}

#[derive(thiserror::Error)]
enum StartupError {
    #[error("Initial Fill Error: {0}")]
    InitialFill(#[from] FillError),
    #[error("Reqwest Init Error: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("Port Binding error: {0}")]
    Bind(#[from] std::io::Error),
    #[error("Axum Error: {0}")]
    Axum(#[from] axum::Error),
    #[error("Config Parse Error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("Log Level Parse Error: {0}")]
    Tracing(#[from] tracing::metadata::ParseLevelFilterError),
}

impl std::fmt::Debug for StartupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        <Self as std::fmt::Display>::fmt(self, f)
    }
}
