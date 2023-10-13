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
    pub data: RawPatreonId
}

#[derive(serde::Deserialize)]
pub struct RawPatreonId {
    pub id: String
}

#[derive(serde::Deserialize)]
pub struct RawPatreonTierRelationship {
    pub data: Vec<RawPatreonId>,
}

#[derive(serde::Deserialize)]
pub struct RawPatreonUser {
    pub id: String,
    pub attributes: RawPatreonUserAttributes
}

#[derive(serde::Deserialize)]
pub struct RawPatreonUserAttributes {
    pub social_connections: Option<RawPatreonSocialConnections>
}

#[derive(serde::Deserialize)]
pub struct RawPatreonSocialConnections {
    pub discord: Option<RawPatreonDiscordConnection>
}

#[derive(serde::Deserialize)]
pub struct RawPatreonDiscordConnection {
    pub user_id: Option<String>
}

#[derive(serde::Deserialize)]
pub struct RawPatreonMeta {
    pub pagination: RawPatreonPagination
}

#[derive(serde::Deserialize)]
pub struct RawPatreonPagination {
    pub cursors: Option<RawPatreonCursors>,
}

#[derive(serde::Deserialize)]
pub struct RawPatreonCursors {
    pub next: Option<String>
}
