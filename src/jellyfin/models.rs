use serde::{Deserialize, Serialize};

pub type JellyfinItemId = String;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct JellyfinAuthResponse {
    pub user: JellyfinUser,
    pub access_token: String,
    #[serde(default)]
    pub server_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct JellyfinUser {
    pub id: JellyfinItemId,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
pub struct JellyfinItemsResponse<T> {
    #[serde(default)]
    pub items: Vec<T>,
    #[serde(default)]
    pub total_record_count: Option<i32>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct JellyfinTrack {
    pub id: JellyfinItemId,
    pub name: String,
    #[serde(default)]
    pub artists: Vec<String>,
    #[serde(default)]
    pub album_artist: Option<String>,
    #[serde(default)]
    pub album_artists: Vec<JellyfinNameId>,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub album_id: Option<JellyfinItemId>,
    #[serde(default)]
    pub index_number: Option<i32>,
    #[serde(default)]
    pub parent_index_number: Option<i32>,
    #[serde(default)]
    pub artist_items: Vec<JellyfinNameId>,
    #[serde(default)]
    pub run_time_ticks: Option<i64>,
    #[serde(default)]
    pub container: Option<String>,
    #[serde(default)]
    pub media_sources: Vec<JellyfinMediaSource>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct JellyfinAlbum {
    pub id: JellyfinItemId,
    pub name: String,
    #[serde(default)]
    pub album_artist: Option<String>,
    #[serde(default)]
    pub production_year: Option<i32>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct JellyfinArtist {
    pub id: JellyfinItemId,
    pub name: String,
    #[serde(default)]
    pub image_tags: JellyfinImageTags,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct JellyfinPlaylist {
    pub id: JellyfinItemId,
    pub name: String,
    #[serde(default)]
    pub child_count: Option<i32>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct JellyfinImageTags {
    #[serde(default)]
    pub primary: Option<String>,
    #[serde(default)]
    pub backdrop: Option<String>,
    #[serde(default)]
    pub logo: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct JellyfinNameId {
    pub id: JellyfinItemId,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct JellyfinMediaSource {
    pub id: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub container: Option<String>,
    #[serde(default)]
    pub bitrate: Option<i32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtistImageKind {
    Primary,
    Backdrop,
    Logo,
}

impl ArtistImageKind {
    pub fn as_jellyfin_name(self) -> &'static str {
        match self {
            Self::Primary => "Primary",
            Self::Backdrop => "Backdrop",
            Self::Logo => "Logo",
        }
    }
}
