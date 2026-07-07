use super::prelude::*;

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub(crate) struct UiTrack {
    pub(crate) item_id: Option<String>,
    #[serde(default)]
    pub(crate) date_last_saved: Option<String>,
    #[serde(default)]
    pub(crate) album_id: Option<String>,
    pub(crate) media_source_id: Option<String>,
    pub(crate) stream_url: Option<String>,
    #[serde(default)]
    pub(crate) fallback_stream_url: Option<String>,
    #[serde(skip)]
    pub(crate) stream_http_headers: Vec<(String, String)>,
    pub(crate) artwork_url: Option<String>,
    pub(crate) thumbnail_artwork_url: Option<String>,
    pub(crate) title: String,
    pub(crate) artist: String,
    #[serde(default)]
    pub(crate) album_artist: Option<String>,
    #[serde(default)]
    pub(crate) artist_images: Vec<UiArtistImage>,
    pub(crate) album: String,
    pub(crate) disc_number: Option<i32>,
    pub(crate) track_number: Option<i32>,
    #[serde(default)]
    pub(crate) album_position: Option<usize>,
    pub(crate) duration: String,
    pub(crate) quality: String,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub(crate) struct UiArtistImage {
    pub(crate) key: String,
    pub(crate) name: String,
    pub(crate) thumbnail_url: String,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub(crate) struct UiPlaylist {
    pub(crate) id: String,
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) date_last_saved: Option<String>,
    #[serde(default)]
    pub(crate) artwork_url: Option<String>,
    #[serde(default)]
    pub(crate) thumbnail_artwork_url: Option<String>,
    #[serde(default)]
    pub(crate) tracks: Vec<UiTrack>,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub(crate) struct RadioStation {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) url: String,
    pub(crate) source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) icon: Option<String>,
    #[serde(default)]
    pub(crate) built_in: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RadioSourceKind {
    Stream,
    YouTube,
    Twitch,
}

pub(crate) const RADIO_DEFAULT_ICON: &str = "\u{EFBC}";

impl RadioStation {
    pub(crate) fn built_in(name: &str, url: &str, icon: &str) -> Self {
        Self {
            id: format!("built-in:{name}"),
            name: name.to_string(),
            url: url.to_string(),
            source: "stream".to_string(),
            icon: Some(icon.to_string()),
            built_in: true,
        }
    }

    pub(crate) fn source_kind(&self) -> RadioSourceKind {
        radio_source_kind_from_station(&self.source, &self.url)
    }

    pub(crate) fn icon_glyph(&self) -> &str {
        self.icon
            .as_deref()
            .filter(|icon| !icon.trim().is_empty())
            .unwrap_or_else(|| default_radio_icon_for_kind(self.source_kind()))
    }

    pub(crate) fn source_label(&self) -> &'static str {
        self.source_kind().label()
    }

    pub(crate) fn mpris_source_label(&self) -> &'static str {
        self.source_kind().mpris_label()
    }
}

impl RadioSourceKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Stream => "stream",
            Self::YouTube => "youtube",
            Self::Twitch => "twitch",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Stream => "Stream",
            Self::YouTube => "YouTube Live",
            Self::Twitch => "Twitch Live",
        }
    }

    pub(crate) fn mpris_label(self) -> &'static str {
        match self {
            Self::Stream => "Radio Stream",
            Self::YouTube => "Youtube Stream",
            Self::Twitch => "Twitch Stream",
        }
    }

    pub(crate) fn external_source(self) -> Option<ExternalStreamSource> {
        match self {
            Self::Stream => None,
            Self::YouTube => Some(ExternalStreamSource::YouTube),
            Self::Twitch => Some(ExternalStreamSource::Twitch),
        }
    }
}

pub(crate) fn radio_source_kind_from_station(source: &str, raw_url: &str) -> RadioSourceKind {
    match source {
        "youtube" => RadioSourceKind::YouTube,
        "twitch" => RadioSourceKind::Twitch,
        _ => raw_url
            .parse::<url::Url>()
            .ok()
            .map(|url| radio_source_kind_for_url(&url))
            .unwrap_or(RadioSourceKind::Stream),
    }
}

pub(crate) fn radio_source_kind_for_url(url: &url::Url) -> RadioSourceKind {
    let Some(host) = url
        .host_str()
        .map(|host| host.trim_end_matches('.').to_ascii_lowercase())
    else {
        return RadioSourceKind::Stream;
    };
    let host = host.strip_prefix("www.").unwrap_or(host.as_str());

    if host == "youtu.be"
        || host == "youtube.com"
        || host.ends_with(".youtube.com")
        || host == "youtube-nocookie.com"
        || host.ends_with(".youtube-nocookie.com")
    {
        RadioSourceKind::YouTube
    } else if host == "twitch.tv" || host.ends_with(".twitch.tv") {
        RadioSourceKind::Twitch
    } else {
        RadioSourceKind::Stream
    }
}

impl UiTrack {
    pub(crate) fn from_jellyfin(track: JellyfinTrack, client: &JellyfinClient) -> Self {
        let artist_items = track.artist_items.clone();
        let album_artist_items = track.album_artists.clone();
        let artist = if !track.artists.is_empty() {
            track.artists.join(", ")
        } else if !artist_items.is_empty() {
            artist_items
                .iter()
                .map(|artist| artist.name.clone())
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            "Unknown Artist".to_string()
        };
        let album_artist = track.album_artist.clone();
        let artist_images =
            artist_image_urls(album_artist_items.iter().chain(artist_items.iter()), client);

        let quality = track
            .container
            .or_else(|| {
                track
                    .media_sources
                    .first()
                    .and_then(|source| source.container.clone())
            })
            .unwrap_or_else(|| "stream".to_string())
            .split(',')
            .next()
            .unwrap_or("stream")
            .trim()
            .to_uppercase();
        let stream_url = client
            .item_direct_stream_url(&track.id)
            .ok()
            .map(|url| url.to_string());
        let fallback_stream_url = client
            .item_transcode_stream_url(&track.id)
            .ok()
            .map(|url| url.to_string());
        let stream_http_headers = client.stream_http_headers();
        let media_source_id = track
            .media_sources
            .first()
            .map(|source| source.id.clone())
            .unwrap_or_else(|| track.id.clone());
        let artwork_item_id = track.album_id.as_deref().unwrap_or(&track.id);
        let artwork_url = client
            .item_image_url(artwork_item_id, "Primary")
            .ok()
            .map(|url| url.to_string());
        let thumbnail_artwork_url = client
            .item_image_url_with_size(
                artwork_item_id,
                "Primary",
                Some(SIDEBAR_COVER_ART_IMAGE_SIZE),
            )
            .ok()
            .map(|url| url.to_string());

        Self {
            item_id: Some(track.id),
            date_last_saved: track.date_last_saved,
            album_id: track.album_id,
            media_source_id: Some(media_source_id),
            stream_url,
            fallback_stream_url,
            stream_http_headers,
            artwork_url,
            thumbnail_artwork_url,
            title: track.name,
            artist,
            album_artist,
            artist_images,
            album: track.album.unwrap_or_else(|| "Unknown Album".to_string()),
            disc_number: track.parent_index_number,
            track_number: track.index_number,
            album_position: None,
            duration: format_runtime(track.run_time_ticks),
            quality,
        }
    }

    pub(crate) fn artist_thumbnail_url_for(&self, artist: &str) -> Option<String> {
        let key = artist_key(artist);
        self.artist_images
            .iter()
            .find(|image| image.key == key)
            .or_else(|| {
                if artist == self.artist && self.artist_images.len() == 1 {
                    self.artist_images.first()
                } else {
                    None
                }
            })
            .map(|image| image.thumbnail_url.clone())
    }
}

impl UiPlaylist {
    pub(crate) fn from_jellyfin(
        playlist: JellyfinPlaylist,
        tracks: Vec<UiTrack>,
        client: &JellyfinClient,
    ) -> Self {
        let artwork_url = client
            .item_image_url(&playlist.id, "Primary")
            .ok()
            .map(|url| url.to_string());
        let thumbnail_artwork_url = client
            .item_image_url_with_size(&playlist.id, "Primary", Some(160))
            .ok()
            .map(|url| url.to_string())
            .or_else(|| {
                tracks
                    .iter()
                    .find_map(|track| track.thumbnail_artwork_url.clone())
            });

        Self {
            id: playlist.id,
            name: playlist.name,
            date_last_saved: playlist.date_last_saved,
            artwork_url,
            thumbnail_artwork_url,
            tracks,
        }
    }
}

pub(crate) fn artist_image_urls<'a>(
    artists: impl Iterator<Item = &'a crate::jellyfin::JellyfinNameId>,
    client: &JellyfinClient,
) -> Vec<UiArtistImage> {
    let mut images = Vec::new();
    for artist in artists {
        let key = artist_key(&artist.name);
        if images.iter().any(|image: &UiArtistImage| image.key == key) {
            continue;
        }
        if let Some(thumbnail_url) = client
            .item_image_url_with_size(&artist.id, "Primary", Some(160))
            .ok()
            .map(|url| url.to_string())
        {
            images.push(UiArtistImage {
                key,
                name: artist.name.clone(),
                thumbnail_url,
            });
        }
    }
    images
}

pub(crate) fn format_runtime(run_time_ticks: Option<i64>) -> String {
    let Some(ticks) = run_time_ticks else {
        return "--:--".to_string();
    };
    let total_seconds = (ticks / 10_000_000).max(0);
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{minutes}:{seconds:02}")
}

pub(crate) fn parse_duration_str(s: &str) -> f64 {
    let parts: Vec<f64> = s.split(':').filter_map(|p| p.parse().ok()).collect();
    match parts.len() {
        3 => parts[0] * 3600.0 + parts[1] * 60.0 + parts[2],
        2 => parts[0] * 60.0 + parts[1],
        1 => parts[0],
        _ => 0.0,
    }
}

pub(crate) fn album_key(track: &UiTrack) -> String {
    track
        .album_id
        .clone()
        .unwrap_or_else(|| normalized_key(&track.album))
}

pub(crate) fn artist_key(artist: &str) -> String {
    normalized_artist_key(artist)
}

pub(crate) fn normalized_key(value: &str) -> String {
    normalized_text_key(value)
}

pub(crate) fn normalized_text_key(value: &str) -> String {
    let mut key = String::new();
    for character in value.trim().chars() {
        for character in character.to_lowercase() {
            if character.is_whitespace() {
                push_key_separator(&mut key);
            } else {
                key.push(character);
            }
        }
    }
    key.trim_end().to_string()
}

pub(crate) fn normalized_artist_key(value: &str) -> String {
    let mut key = String::new();
    for character in value.trim().chars() {
        for character in character.to_lowercase() {
            if character.is_alphanumeric() {
                key.push(character);
            } else if is_ignored_artist_key_character(character) {
                continue;
            } else {
                push_key_separator(&mut key);
            }
        }
    }
    key.trim_end().to_string()
}

pub(crate) fn normalized_artist_display_key(value: &str) -> String {
    let mut key = String::new();
    for character in value.trim().chars() {
        for character in character.to_lowercase() {
            if character.is_whitespace() {
                push_key_separator(&mut key);
            } else if is_dash_character(character) {
                key.push('-');
            } else {
                key.push(character);
            }
        }
    }
    key.trim_end().to_string()
}

pub(crate) fn push_key_separator(key: &mut String) {
    if !key.is_empty() && !key.ends_with(' ') {
        key.push(' ');
    }
}

pub(crate) fn is_dash_character(character: char) -> bool {
    matches!(
        character,
        '-' | '\u{2010}'
            | '\u{2011}'
            | '\u{2012}'
            | '\u{2013}'
            | '\u{2014}'
            | '\u{2015}'
            | '\u{2212}'
            | '\u{FE58}'
            | '\u{FE63}'
            | '\u{FF0D}'
    )
}

pub(crate) fn is_ignored_artist_key_character(character: char) -> bool {
    matches!(
        character,
        '\'' | '"'
            | '`'
            | '\u{2018}'
            | '\u{2019}'
            | '\u{201C}'
            | '\u{201D}'
            | '\u{200B}'
            | '\u{200C}'
            | '\u{200D}'
            | '\u{FEFF}'
    )
}

pub(crate) fn default_radio_icon_for_kind(kind: RadioSourceKind) -> &'static str {
    match kind {
        RadioSourceKind::Stream => RADIO_DEFAULT_ICON,
        RadioSourceKind::YouTube => RADIO_DEFAULT_ICON,
        RadioSourceKind::Twitch => RADIO_DEFAULT_ICON,
    }
}

pub(crate) fn track_matches_query(track: &UiTrack, query: &str) -> bool {
    track.title.to_lowercase().contains(query)
        || track.artist.to_lowercase().contains(query)
        || track.album.to_lowercase().contains(query)
}

pub(crate) fn track_key(track: &UiTrack) -> String {
    track
        .item_id
        .clone()
        .unwrap_or_else(|| format!("{}\u{1f}{}\u{1f}{}", track.title, track.artist, track.album))
}

pub(crate) fn track_has_key(track: &UiTrack, key: &str) -> bool {
    match track.item_id.as_deref() {
        Some(item_id) => item_id == key,
        None => track_key(track) == key,
    }
}

pub(crate) fn same_track(left: &UiTrack, right: &UiTrack) -> bool {
    match (left.item_id.as_deref(), right.item_id.as_deref()) {
        (Some(left_id), Some(right_id)) => left_id == right_id,
        (None, None) => {
            left.title == right.title && left.artist == right.artist && left.album == right.album
        }
        _ => false,
    }
}

pub(crate) fn track_key_if_same_album(track: &UiTrack, album_key_value: &str) -> Option<String> {
    (album_key(track) == album_key_value).then(|| track_key(track))
}

pub(crate) fn duration_seconds(duration: &str) -> i32 {
    let mut total = 0;
    for part in duration.split(':') {
        let Ok(value) = part.parse::<i32>() else {
            return 0;
        };
        total = total * 60 + value;
    }
    total
}
