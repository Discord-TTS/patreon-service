#![warn(clippy::pedantic)]
#![allow(clippy::unused_async)]

use std::{borrow::Cow, str::FromStr, collections::HashMap};

use anyhow::Result;
use once_cell::sync::OnceCell;
use hmac::{Mac as _, digest::FixedOutput};
use subtle::ConstantTimeEq;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use axum::{http::HeaderValue, response::Response};


mod models;
mod macros;

type ResponseResult<T> = Result<T, Error>;

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
            }
        }
    }
}


#[derive(serde::Deserialize, serde::Serialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
struct DiscordUserId(u64);

#[derive(serde::Deserialize)]
struct Config {
    campaign_id: String,
    basic_tier_id: String,
    extra_tier_id: String,
    webhook_secret: String,
    bind_address: Option<String>,
    creator_access_token: String,
    #[serde(default)] preset_members: Vec<DiscordUserId>,
}


struct State {
    members: parking_lot::RwLock<HashMap<DiscordUserId, PatreonTierInfo>>,
    refresh_task: tokio::sync::mpsc::Sender<()>,
    reqwest: reqwest::Client,
    config: Config,
}

static STATE: OnceCell<State> = OnceCell::new();


fn check_md5(key: &[u8], untrusted_signature: &[u8], untrusted_data: &[u8]) -> Result<bool> {
    let mut mac = hmac::Hmac::<md5::Md5>::new_from_slice(key)?;
    mac.update(untrusted_data);

    let correct_sig = mac.finalize_fixed();
    Ok(correct_sig.ct_eq(untrusted_signature).into())
}




#[derive(serde::Deserialize)]
struct FetchMember {
    member_id: DiscordUserId
}

async fn fetch_member(axum::extract::Path(payload): axum::extract::Path<FetchMember>) -> impl axum::response::IntoResponse {
    let state = STATE.get().unwrap();
    axum::Json(state.members.read().get(&payload.member_id).copied())
}

async fn fetch_members() -> impl axum::response::IntoResponse {
    let state = STATE.get().unwrap();    
    axum::Json(state.members.read().clone())
}

async fn refresh_members() {
    let state = STATE.get().unwrap();
    state.refresh_task.send(()).await.unwrap();
}

async fn webhook_recv(
    headers: axum::http::HeaderMap,
    payload: String,
) -> ResponseResult<()> {
    if check_md5(
        STATE.get().unwrap().config.webhook_secret.as_bytes(),
        require!(headers.get("X-Patreon-Signature"), Ok(())).as_bytes(),
        payload.as_bytes()
    )? {
        return Err(Error::SignatureMismatch);
    };

    let event = require!(headers.get("X-Patreon-Event"), Ok(())).to_str()?;
    if matches!(event, "members:pledge:create" | "members:pledge:delete" | "members:pledge:update" | "members:create") {
        fill_members().await?; // Just refresh all the members
    } else {
        tracing::info!("Unknown event: {event}");
    }

    Ok(())
}


#[tokio::main]
async fn main() -> Result<()> {
    let fmt_layer = tracing_subscriber::fmt::layer();
    let filter = tracing_subscriber::filter::LevelFilter::from_str(
        &std::env::var("LOG_LEVEL")
        .unwrap_or_else(|_| String::from("INFO"))
    )?;

    tracing_subscriber::registry().with(fmt_layer).with(filter).init();

    let mut config: Config = toml::from_str(&std::fs::read_to_string("config.toml")?)?;
    let bind_address = config.bind_address.take().unwrap().parse()?;

    STATE.set(State {
        config,
        reqwest: reqwest::Client::new(),
        members: parking_lot::RwLock::new(HashMap::new()),
        refresh_task: {
            let (tx, mut rx) = tokio::sync::mpsc::channel(1);

            tokio::spawn(async move {loop {
                let res = tokio::time::timeout(
                    std::time::Duration::from_secs(60 * 60),
                    rx.recv()
                ).await;

                if res.as_ref().map(Option::is_none).unwrap_or(false) {
                    break
                }

                match fill_members().await {
                    Ok(len) => tracing::info!("Refreshed {len} members"),
                    Err(err) => tracing::error!("{err:?}"),
                }
            }});
            tx
        }
    }).is_err().then(|| unreachable!());

    fill_members().await?;

    let app = axum::Router::new()
        .route("/members/:member_id", axum::routing::get(fetch_member))
        .route("/refresh", axum::routing::post(refresh_members))
        .route("/patreon", axum::routing::post(webhook_recv))
        .route("/members", axum::routing::get(fetch_members));

    tracing::info!("Binding to {bind_address}!");
    axum::Server::bind(&bind_address)
        .serve(app.into_make_service())
        .with_graceful_shutdown(async {drop(tokio::signal::ctrl_c().await)})
        .await?;

    Ok(())
}



const BASE_URL: &str = "https://www.patreon.com/api/oauth2/v2";

fn get_member_tier(config: &Config, member: &models::RawPatreonMember, user: &models::RawPatreonUser) -> Option<(DiscordUserId, Option<PatreonTier>)> {
    user.attributes.social_connections.as_ref().and_then(|socials| socials.discord.as_ref()).map(|discord_info| {
        let check_tier = |tier_id| member.relationships.currently_entitled_tiers.data.iter().any(|tier| tier_id == &tier.id);

        (
            DiscordUserId(discord_info.user_id.parse().unwrap()),
            if check_tier(&config.extra_tier_id) {
                Some(PatreonTier::Extra)
            } else if check_tier(&config.basic_tier_id) {
                Some(PatreonTier::Basic)
            } else {
                None
            }
        )
    })
}

async fn fill_members() -> Result<usize> {
    let state = STATE.get().unwrap();

    let mut url = reqwest::Url::parse(&format!("{BASE_URL}/campaigns/{}/members", state.config.campaign_id))?;
    url.query_pairs_mut()
        .append_pair("fields[user]", "social_connections")
        .append_pair("include", "user,currently_entitled_tiers")
        .finish();

    let mut cursor = Cow::Borrowed("");
    let headers = reqwest::header::HeaderMap::from_iter([
        (reqwest::header::AUTHORIZATION, HeaderValue::from_str(&format!("Bearer {}", state.config.creator_access_token))?)
    ]);

    let mut members = HashMap::with_capacity(state.members.read().len());

    loop {
        let mut url = url.clone();
        url.query_pairs_mut().append_pair("page[cursor]", &cursor);

        let resp: models::RawPatreonResponse = state.reqwest.get(url).headers(headers.clone()).send().await?.json().await?;

        members.extend(resp.data.into_iter().filter_map(|member| {
            let user_id = &member.relationships.user.data.id;
            let user = resp.included.iter().find(|u| &u.id == user_id).unwrap();

            get_member_tier(&state.config, &member, user).and_then(|(discord_id, tier)| {
                tier.map(|tier| (discord_id, PatreonTierInfo::from(tier)))
            })
        }));

        if let Some(cursors) = resp.meta.pagination.cursors {
            cursor = Cow::Owned(cursors.next);
        } else {
            members.extend(state.config.preset_members.iter().map(|id| (*id, PatreonTierInfo::fake())));
            members.shrink_to_fit();

            let len = members.len();
            *state.members.write() = members;
            break Ok(len)
        }
    }
}


#[derive(Debug)]
enum Error {
    SignatureMismatch,
    Unknown(anyhow::Error),
}

impl<E: Into<anyhow::Error>> From<E> for Error {
    fn from(e: E) -> Self {
        Self::Unknown(e.into())
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SignatureMismatch => write!(f, "Signature mismatch!"),
            Self::Unknown(e) => write!(f, "Unknown error: {e}"),
        }
    }
}

impl axum::response::IntoResponse for Error {
    fn into_response(self) -> Response {
        tracing::error!("{self:?}");
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            self.to_string()
        ).into_response()
    }
}
