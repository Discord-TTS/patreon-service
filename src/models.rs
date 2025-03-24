use arrayvec::ArrayString;
use smallvec::SmallVec;

use crate::serde_as_str;

#[derive(serde::Deserialize)]
pub struct RawPatreonResponse {
    pub data: Vec<RawPatreonMember>,
    pub included: Vec<RawPatreonUser>,
    pub meta: RawPatreonMeta,
}

#[derive(serde::Deserialize)]
pub struct RawPatreonMember {
    pub relationships: RawPatreonRelationships,
}

#[derive(serde::Deserialize)]
pub struct RawPatreonRelationships {
    pub user: RawPatreonIdData,
    pub currently_entitled_tiers: RawPatreonTierRelationship,
}

#[derive(serde::Deserialize)]
pub struct RawPatreonIdData {
    pub data: RawPatreonId,
}

#[derive(serde::Deserialize)]
pub struct RawPatreonId {
    #[serde(with = "serde_as_str")]
    pub id: u32,
}

#[derive(serde::Deserialize)]
pub struct RawPatreonTierRelationship {
    pub data: SmallVec<[RawPatreonId; 4]>,
}

#[derive(serde::Deserialize)]
pub struct RawPatreonUser {
    #[serde(with = "serde_as_str")]
    pub id: u32,
    pub attributes: RawPatreonUserAttributes,
}

#[derive(serde::Deserialize)]
pub struct RawPatreonUserAttributes {
    pub social_connections: Option<RawPatreonSocialConnections>,
}

#[derive(serde::Deserialize)]
pub struct RawPatreonSocialConnections {
    pub discord: Option<RawPatreonDiscordConnection>,
}

#[derive(serde::Deserialize)]
pub struct RawPatreonDiscordConnection {
    pub user_id: Option<RawPatreonDiscordUserId>,
}

#[derive(serde::Deserialize)]
pub struct RawPatreonDiscordUserId(#[serde(with = "serde_as_str")] pub u64);

#[derive(serde::Deserialize)]
pub struct RawPatreonMeta {
    pub pagination: RawPatreonPagination,
}

#[derive(serde::Deserialize)]
pub struct RawPatreonPagination {
    pub cursors: Option<RawPatreonCursors>,
}

#[derive(serde::Deserialize)]
pub struct RawPatreonCursors {
    pub next: Option<ArrayString<128>>,
}

#[derive(serde::Deserialize)]
pub struct RawPatreonOAuth2Response {
    pub access_token: Box<str>,
    pub refresh_token: Box<str>,
}

#[derive(serde::Deserialize)]
pub struct RawPatreonError {
    pub errors: [RawPatreonRateLimit; 1],
}

#[derive(serde::Deserialize)]
pub struct RawPatreonRateLimit {
    pub retry_after_seconds: u16,
}
