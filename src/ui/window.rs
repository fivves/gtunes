use adw::prelude::*;
use gtk::glib::object::IsA;
use gtk::{Align, Orientation};
use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, PlatformConfig};
use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Mutex, mpsc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::cache::{CacheDatabase, JellyfinSession};
use crate::config;
use crate::discord::{
    DiscordPresence, PresenceActivity, PresencePlaybackState, artwork_cache_path,
};
use crate::jellyfin::{
    JellyfinClient, JellyfinClientError, JellyfinItemSummary, JellyfinPlaylist, JellyfinTrack,
    stream_http_headers_for_token,
};
use crate::playback::{
    ExternalStreamSource, PlaybackEngine, PlaybackEvent, PlaybackRequest, PlaybackState,
    PlaybackStreamKind, resolve_external_stream_url, session,
};
use crate::waveform::{WaveformKey, WaveformSummary};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
struct UiTrack {
    item_id: Option<String>,
    #[serde(default)]
    date_last_saved: Option<String>,
    #[serde(default)]
    album_id: Option<String>,
    media_source_id: Option<String>,
    stream_url: Option<String>,
    #[serde(default)]
    fallback_stream_url: Option<String>,
    #[serde(skip)]
    stream_http_headers: Vec<(String, String)>,
    artwork_url: Option<String>,
    thumbnail_artwork_url: Option<String>,
    title: String,
    artist: String,
    #[serde(default)]
    album_artist: Option<String>,
    #[serde(default)]
    artist_images: Vec<UiArtistImage>,
    album: String,
    disc_number: Option<i32>,
    track_number: Option<i32>,
    #[serde(default)]
    album_position: Option<usize>,
    duration: String,
    quality: String,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
struct UiArtistImage {
    key: String,
    name: String,
    thumbnail_url: String,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
struct UiPlaylist {
    id: String,
    name: String,
    #[serde(default)]
    date_last_saved: Option<String>,
    #[serde(default)]
    artwork_url: Option<String>,
    #[serde(default)]
    thumbnail_artwork_url: Option<String>,
    #[serde(default)]
    tracks: Vec<UiTrack>,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
struct RadioStation {
    id: String,
    name: String,
    url: String,
    source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    icon: Option<String>,
    #[serde(default)]
    built_in: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RadioSourceKind {
    Stream,
    YouTube,
    Twitch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
enum SortColumn {
    Title,
    Artist,
    Album,
    Duration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LibraryPage {
    Tracks,
    Albums,
    Artists,
    Playlists,
    Radio,
    NextUp,
}

#[derive(Debug)]
enum ImageFetchError {
    Missing,
    HttpStatus(reqwest::StatusCode),
    Request(&'static str),
    Io(std::io::Error),
}

impl fmt::Display for ImageFetchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => formatter.write_str("image not found"),
            Self::HttpStatus(status) => write!(formatter, "HTTP status {status}"),
            Self::Request(message) => formatter.write_str(message),
            Self::Io(error) => write!(formatter, "io error: {error}"),
        }
    }
}

#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize)]
struct LibraryViewSettings {
    sort_column: SortColumn,
    sort_ascending: bool,
}

impl Default for LibraryViewSettings {
    fn default() -> Self {
        Self {
            sort_column: SortColumn::Title,
            sort_ascending: true,
        }
    }
}

struct UiState {
    all_tracks: Vec<UiTrack>,
    tracks: Vec<UiTrack>,
    playlists: Vec<UiPlaylist>,
    radio_stations: Vec<RadioStation>,
    track_filter_signature: TrackFilterSignature,
    library_albums: Vec<AlbumSummary>,
    library_artists: Vec<ArtistSummary>,
    collection_render_generation: u64,
    active_page: LibraryPage,
    album_filter: Option<String>,
    artist_filter: Option<String>,
    playlist_filter: Option<String>,
    collection_detail_title: Option<String>,
    collection_detail_subtitle: Option<String>,
    collection_detail_parent_search_query: Option<String>,
    collection_return_target: Option<CollectionReturnTarget>,
    collection_parent_return_target: Option<CollectionReturnTarget>,
    selected_index: usize,
    search_query: String,
    connection_generation: u64,
    jellyfin_connected: bool,
    sort_column: SortColumn,
    sort_ascending: bool,
    keep_playing_while_closed: bool,
    animations_enabled: bool,
    font_mono: bool,
    playback_session: session::PlaybackSession<UiTrack>,
    track_indicators: Vec<(String, gtk::Image)>,
    last_playback_snapshot_at: Option<Instant>,
    library_stack: Option<gtk::Stack>,
    album_grid: Option<gtk::FlowBox>,
    artist_grid: Option<gtk::FlowBox>,
    playlist_grid: Option<gtk::FlowBox>,
    album_grid_scroll_value: f64,
    artist_grid_scroll_value: f64,
    playlist_grid_scroll_value: f64,
    radio_grid: Option<gtk::FlowBox>,
    detail_header: Option<gtk::Box>,
    detail_title_label: Option<gtk::Label>,
    detail_subtitle_label: Option<gtk::Label>,
    nav_list: Option<gtk::ListBox>,
    nav_track_count: Option<gtk::Label>,
    nav_album_count: Option<gtk::Label>,
    nav_artist_count: Option<gtk::Label>,
    nav_playlist_count: Option<gtk::Label>,
    nav_radio_count: Option<gtk::Label>,
    track_model: gtk::StringList,
    track_selection: Option<gtk::SingleSelection>,
    track_stack: Option<gtk::Stack>,
    track_empty: Option<gtk::Label>,
    track_empty_detail: Option<gtk::Label>,
    now_title: gtk::Label,
    now_meta: gtk::Label,
    playback_status: gtk::Label,
    page_summary: gtk::Label,
    connection_status: gtk::Label,
    connection_detail: gtk::Label,
    sync_spinner: Option<gtk::Spinner>,
    connection_card: Option<gtk::Box>,
    connection_form_status: Option<gtk::Label>,
    connection_server_entry: Option<gtk::Entry>,
    connection_username_entry: Option<gtk::Entry>,
    connection_password_entry: Option<gtk::PasswordEntry>,
    radio_name_entry: Option<gtk::Entry>,
    radio_url_entry: Option<gtk::Entry>,
    radio_icon_entry: Option<gtk::Entry>,
    search_entry: Option<gtk::SearchEntry>,
    cover_art: Option<gtk::Image>,
    play_button: Option<gtk::Button>,
    shuffle_button: Option<gtk::Button>,
    refresh_button: Option<gtk::Button>,
    reconnect_button: Option<gtk::Button>,
    queue_view: Option<Rc<QueueView>>,
    sidebar_queue_card: Option<gtk::Box>,
    next_up_view: Option<Rc<NextUpPageView>>,
    wave_area: Option<gtk::DrawingArea>,
    elapsed_label: gtk::Label,
    remaining_label: gtk::Label,
    waveform_status: gtk::Label,
    waveform: Rc<RefCell<WaveformVisual>>,
    playback: Option<PlaybackEngine>,
    loading_spinner: Option<gtk::Spinner>,
    mpris: Option<MediaControls>,
    discord_presence: Option<DiscordPresence>,
}

#[derive(Debug)]
struct InvisibleSearchState {
    query: String,
    last_input_at: Instant,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TrackFilterSignature {
    album_filter: Option<String>,
    artist_filter: Option<String>,
    playlist_filter: Option<String>,
    search_query: String,
    sort_column: SortColumn,
    sort_ascending: bool,
}

impl UiState {
    fn is_track_list_visible(&self) -> bool {
        self.active_page == LibraryPage::Tracks
            || self.album_filter.is_some()
            || self.playlist_filter.is_some()
    }

    fn current_track_filter_signature(&self) -> TrackFilterSignature {
        TrackFilterSignature {
            album_filter: self.album_filter.clone(),
            artist_filter: self.artist_filter.clone(),
            playlist_filter: self.playlist_filter.clone(),
            search_query: self.search_query.clone(),
            sort_column: self.sort_column,
            sort_ascending: self.sort_ascending,
        }
    }

    fn track_filter_is_current(&self) -> bool {
        self.track_filter_signature == self.current_track_filter_signature()
    }
}

#[derive(Clone, Debug)]
struct WaveformVisual {
    peaks: Vec<f32>,
    progress: f64,
    loaded_key: Option<WaveformKey>,
    loading_key: Option<WaveformKey>,
}

#[derive(Clone, Debug)]
struct ConnectionPayload {
    session: JellyfinSession,
    tracks: Vec<UiTrack>,
    playlists: Vec<UiPlaylist>,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
struct CachedLibrary {
    tracks: Vec<UiTrack>,
    #[serde(default)]
    playlists: Vec<UiPlaylist>,
}

enum ConnectionMessage {
    Authenticated(JellyfinSession),
    Status(String),
    Progress { loaded: usize, total: Option<usize> },
    Finished(Result<ConnectionPayload, String>),
}

const PLAYER_ACTION_EDGE_INSET: i32 = 0;
const LEFT_SIDEBAR_CONTENT_WIDTH: i32 = 220;
const SIDEBAR_COVER_ART_IMAGE_SIZE: u32 = LEFT_SIDEBAR_CONTENT_WIDTH as u32;
const LEFT_SIDEBAR_WIDTH: i32 = LEFT_SIDEBAR_CONTENT_WIDTH + 20;
const ACTION_PANEL_WIDTH: i32 = 130;
const ALBUM_ART_SIZE: i32 = 168;
const COLLECTION_TILE_WIDTH: i32 = 184;
const ARTIST_ART_SIZE: i32 = 148;
const RADIO_CARD_CONTENT_WIDTH: i32 = 154;
const RADIO_GRID_COLUMN_GAP: i32 = 14;
const RADIO_DEFAULT_ICON: &str = "\u{EFBC}";
const COLLECTION_ARTWORK_INITIAL_DELAY_MS: u64 = 24;
const COLLECTION_ARTWORK_STAGGER_MS: u64 = 8;
const COLLECTION_ARTWORK_MAX_STAGGERED_ITEMS: usize = 160;
const COLLECTION_TILE_INITIAL_BATCH: usize = 24;
const COLLECTION_TILE_IDLE_BATCH: usize = 24;
const COLLECTION_RETURN_HIGHLIGHT_MS: u64 = 850;
const INVISIBLE_SEARCH_TIMEOUT: Duration = Duration::from_millis(1_200);
const NEXT_UP_PAGE_LIMIT: usize = 50;
const RADIO_STATIONS_KEY: &str = "radio.stations";
static CONNECTION_GENERATION: AtomicU64 = AtomicU64::new(0);
static CACHE_RESET_LOCK: Mutex<()> = Mutex::new(());

struct QueueView {
    empty: gtk::Label,
    rows: Vec<QueueRow>,
}

struct QueueRow {
    button: gtk::Button,
    art: gtk::Image,
    title: gtk::Label,
    artist: gtk::Label,
    track_index: Rc<RefCell<Option<usize>>>,
    artwork_url: Rc<RefCell<Option<String>>>,
}

struct NextUpPageView {
    empty: gtk::Box,
    list: gtk::Box,
    rows: Rc<RefCell<Vec<gtk::Button>>>,
}

impl RadioStation {
    fn built_in(name: &str, url: &str, icon: &str) -> Self {
        Self {
            id: format!("built-in:{name}"),
            name: name.to_string(),
            url: url.to_string(),
            source: "stream".to_string(),
            icon: Some(icon.to_string()),
            built_in: true,
        }
    }

    fn source_kind(&self) -> RadioSourceKind {
        radio_source_kind_from_station(&self.source, &self.url)
    }

    fn icon_glyph(&self) -> &str {
        self.icon
            .as_deref()
            .filter(|icon| !icon.trim().is_empty())
            .unwrap_or_else(|| default_radio_icon_for_kind(self.source_kind()))
    }

    fn source_label(&self) -> &'static str {
        self.source_kind().label()
    }

    fn mpris_source_label(&self) -> &'static str {
        self.source_kind().mpris_label()
    }
}

impl RadioSourceKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Stream => "stream",
            Self::YouTube => "youtube",
            Self::Twitch => "twitch",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Stream => "Stream",
            Self::YouTube => "YouTube Live",
            Self::Twitch => "Twitch Live",
        }
    }

    fn mpris_label(self) -> &'static str {
        match self {
            Self::Stream => "Radio Stream",
            Self::YouTube => "Youtube Stream",
            Self::Twitch => "Twitch Stream",
        }
    }

    fn external_source(self) -> Option<ExternalStreamSource> {
        match self {
            Self::Stream => None,
            Self::YouTube => Some(ExternalStreamSource::YouTube),
            Self::Twitch => Some(ExternalStreamSource::Twitch),
        }
    }
}

fn radio_source_kind_from_station(source: &str, raw_url: &str) -> RadioSourceKind {
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

fn radio_source_kind_for_url(url: &url::Url) -> RadioSourceKind {
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
    fn from_jellyfin(track: JellyfinTrack, client: &JellyfinClient) -> Self {
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

    fn artist_thumbnail_url_for(&self, artist: &str) -> Option<String> {
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
    fn from_jellyfin(
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

fn artist_image_urls<'a>(
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

fn format_runtime(run_time_ticks: Option<i64>) -> String {
    let Some(ticks) = run_time_ticks else {
        return "--:--".to_string();
    };
    let total_seconds = (ticks / 10_000_000).max(0);
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{minutes}:{seconds:02}")
}

pub fn build(app: &adw::Application) -> adw::ApplicationWindow {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title(config::APP_NAME)
        .default_width(1240)
        .default_height(760)
        .width_request(360)
        .height_request(420)
        .build();

    let root = gtk::Box::new(Orientation::Vertical, 0);
    root.add_css_class("app-root");
    window.set_content(Some(&root));

    let view_settings = load_library_view_settings();
    let keep_playing_while_closed = load_keep_playing_while_closed();
    let animations_enabled = load_animations_enabled();
    if !animations_enabled {
        window.add_css_class("no-animations");
        if let Some(gtk_settings) = gtk::Settings::default() {
            gtk_settings.set_gtk_enable_animations(false);
        }
    }
    let font_mono = load_font_mono();
    if font_mono {
        window.add_css_class("font-mono");
    }
    let state = Rc::new(RefCell::new(UiState {
        all_tracks: Vec::new(),
        tracks: Vec::new(),
        playlists: Vec::new(),
        radio_stations: load_radio_stations(),
        track_filter_signature: TrackFilterSignature {
            album_filter: None,
            artist_filter: None,
            playlist_filter: None,
            search_query: String::new(),
            sort_column: view_settings.sort_column,
            sort_ascending: view_settings.sort_ascending,
        },
        library_albums: Vec::new(),
        library_artists: Vec::new(),
        collection_render_generation: 0,
        active_page: LibraryPage::Tracks,
        album_filter: None,
        artist_filter: None,
        playlist_filter: None,
        collection_detail_title: None,
        collection_detail_subtitle: None,
        collection_detail_parent_search_query: None,
        collection_return_target: None,
        collection_parent_return_target: None,
        selected_index: 0,
        search_query: String::new(),
        connection_generation: CONNECTION_GENERATION.load(AtomicOrdering::SeqCst),
        jellyfin_connected: false,
        sort_column: view_settings.sort_column,
        sort_ascending: view_settings.sort_ascending,
        keep_playing_while_closed,
        animations_enabled,
        font_mono,
        playback_session: session::PlaybackSession::default(),
        track_indicators: Vec::new(),
        last_playback_snapshot_at: None,
        library_stack: None,
        album_grid: None,
        artist_grid: None,
        playlist_grid: None,
        album_grid_scroll_value: 0.0,
        artist_grid_scroll_value: 0.0,
        playlist_grid_scroll_value: 0.0,
        radio_grid: None,
        detail_header: None,
        detail_title_label: None,
        detail_subtitle_label: None,
        nav_list: None,
        nav_track_count: None,
        nav_album_count: None,
        nav_artist_count: None,
        nav_playlist_count: None,
        nav_radio_count: None,
        track_model: gtk::StringList::new(&[]),
        track_selection: None,
        track_stack: None,
        track_empty: None,
        track_empty_detail: None,
        now_title: label("No track selected", "now-title"),
        now_meta: label("Connect to Jellyfin to load music", "meta"),
        playback_status: label("Jellyfin stream | Not playing", "meta"),
        page_summary: label("Jellyfin music library | Not connected", "meta"),
        connection_status: label("Not connected", "meta"),
        connection_detail: label("Connect to Jellyfin to sync tracks", "meta"),
        sync_spinner: None,
        connection_card: None,
        connection_form_status: None,
        connection_server_entry: None,
        connection_username_entry: None,
        connection_password_entry: None,
        radio_name_entry: None,
        radio_url_entry: None,
        radio_icon_entry: None,
        search_entry: None,
        cover_art: None,
        play_button: None,
        shuffle_button: None,
        refresh_button: None,
        reconnect_button: None,
        queue_view: None,
        sidebar_queue_card: None,
        next_up_view: None,
        wave_area: None,
        elapsed_label: label("0:00", "mono"),
        remaining_label: label("--:--", "mono"),
        waveform_status: label("Select a Jellyfin track", "wave-marker"),
        waveform: Rc::new(RefCell::new(WaveformVisual {
            peaks: Vec::new(),
            progress: 0.0,
            loaded_key: None,
            loading_key: None,
        })),
        playback: PlaybackEngine::new().ok(),
        loading_spinner: None,
        mpris: None,
        discord_presence: DiscordPresence::from_env(),
    }));

    setup_mpris(state.clone());
    root.append(&build_player_bar(state.clone()));
    root.append(&build_body(state.clone()));
    root.append(&build_bottom_bar(state.clone()));
    connect_app_shortcuts(&root, state.clone());
    connect_window_close_request(&window, state.clone());
    load_selected_waveform(&state);
    start_playback_timer(&state);

    window
}

fn connect_window_close_request(window: &adw::ApplicationWindow, state: Rc<RefCell<UiState>>) {
    window.connect_close_request(move |window| {
        if state.borrow().keep_playing_while_closed {
            window.set_visible(false);
            gtk::glib::Propagation::Stop
        } else {
            let mut ui = state.borrow_mut();
            save_playback_snapshot_now(&mut ui);
            stop_playback(&mut ui);
            gtk::glib::Propagation::Proceed
        }
    });
}

fn connect_app_shortcuts(root: &gtk::Box, state: Rc<RefCell<UiState>>) {
    let controller = gtk::EventControllerKey::new();
    controller.set_propagation_phase(gtk::PropagationPhase::Capture);
    let invisible_search = Rc::new(RefCell::new(InvisibleSearchState {
        query: String::new(),
        last_input_at: Instant::now(),
    }));
    controller.connect_key_pressed(move |_, key, _, modifiers| {
        if modifiers.contains(gtk::gdk::ModifierType::CONTROL_MASK) {
            match key {
                gtk::gdk::Key::_1 => set_library_page(&state, LibraryPage::Tracks),
                gtk::gdk::Key::_2 => set_library_page(&state, LibraryPage::Albums),
                gtk::gdk::Key::_3 => set_library_page(&state, LibraryPage::Artists),
                gtk::gdk::Key::_4 => set_library_page(&state, LibraryPage::Playlists),
                gtk::gdk::Key::_5 => set_library_page(&state, LibraryPage::Radio),
                gtk::gdk::Key::s | gtk::gdk::Key::S => toggle_shuffle(&state),
                gtk::gdk::Key::f | gtk::gdk::Key::F => {
                    if let Some(search) = state.borrow().search_entry.as_ref() {
                        search.grab_focus();
                    }
                }
                _ => return gtk::glib::Propagation::Proceed,
            }

            return gtk::glib::Propagation::Stop;
        }

        if modifiers.intersects(
            gtk::gdk::ModifierType::ALT_MASK
                | gtk::gdk::ModifierType::SUPER_MASK
                | gtk::gdk::ModifierType::META_MASK,
        ) {
            return gtk::glib::Propagation::Proceed;
        }

        match key {
            gtk::gdk::Key::Escape => {
                invisible_search.borrow_mut().query.clear();
                if search_entry_has_focus(&state) {
                    clear_search_entry(&state);
                    focus_active_content(&state);
                    gtk::glib::Propagation::Stop
                } else if text_input_has_focus(&state) {
                    gtk::glib::Propagation::Proceed
                } else {
                    escape_back_or_top(&state);
                    gtk::glib::Propagation::Stop
                }
            }
            gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter => {
                if text_input_has_focus(&state) {
                    invisible_search.borrow_mut().query.clear();
                    return gtk::glib::Propagation::Proceed;
                }
                if let Some(query) = active_invisible_search_query(&invisible_search) {
                    if state.borrow().is_track_list_visible() {
                        play_track_at_selected_index(&state);
                    } else {
                        activate_invisible_collection_match(&state, &query);
                    }
                    gtk::glib::Propagation::Stop
                } else if activate_focused_collection_item(&state) {
                    gtk::glib::Propagation::Stop
                } else {
                    gtk::glib::Propagation::Proceed
                }
            }
            gtk::gdk::Key::space => {
                if text_input_has_focus(&state) {
                    invisible_search.borrow_mut().query.clear();
                    return gtk::glib::Propagation::Proceed;
                }
                let query = {
                    let mut search = invisible_search.borrow_mut();
                    let now = Instant::now();
                    if now.duration_since(search.last_input_at) > INVISIBLE_SEARCH_TIMEOUT {
                        search.query.clear();
                    }
                    if search.query.is_empty() {
                        return gtk::glib::Propagation::Proceed;
                    }
                    search.query.push(' ');
                    search.last_input_at = now;
                    search.query.clone()
                };
                navigate_invisible_search(&state, &query);
                gtk::glib::Propagation::Stop
            }
            gtk::gdk::Key::BackSpace => {
                if text_input_has_focus(&state) {
                    invisible_search.borrow_mut().query.clear();
                    return gtk::glib::Propagation::Proceed;
                }
                let query = {
                    let mut search = invisible_search.borrow_mut();
                    search.query.pop();
                    search.last_input_at = Instant::now();
                    search.query.clone()
                };
                if query.is_empty() || !navigate_invisible_search(&state, &query) {
                    gtk::glib::Propagation::Proceed
                } else {
                    gtk::glib::Propagation::Stop
                }
            }
            _ => {
                if text_input_has_focus(&state) {
                    invisible_search.borrow_mut().query.clear();
                    return gtk::glib::Propagation::Proceed;
                }
                let Some(character) = key.to_unicode().filter(|character| !character.is_control())
                else {
                    return gtk::glib::Propagation::Proceed;
                };

                let query = {
                    let mut search = invisible_search.borrow_mut();
                    let now = Instant::now();
                    if now.duration_since(search.last_input_at) > INVISIBLE_SEARCH_TIMEOUT {
                        search.query.clear();
                    }
                    search.query.push(character);
                    search.last_input_at = now;
                    search.query.clone()
                };

                if navigate_invisible_search(&state, &query) {
                    gtk::glib::Propagation::Stop
                } else {
                    gtk::glib::Propagation::Proceed
                }
            }
        }
    });
    root.add_controller(controller);
}

fn active_invisible_search_query(search: &Rc<RefCell<InvisibleSearchState>>) -> Option<String> {
    let search = search.borrow();
    if search.query.is_empty()
        || Instant::now().duration_since(search.last_input_at) > INVISIBLE_SEARCH_TIMEOUT
    {
        return None;
    }

    Some(search.query.clone())
}

fn search_entry_has_focus(state: &Rc<RefCell<UiState>>) -> bool {
    state
        .borrow()
        .search_entry
        .as_ref()
        .is_some_and(widget_has_focus_within)
}

fn clear_search_entry(state: &Rc<RefCell<UiState>>) {
    let entry = state.borrow().search_entry.clone();
    if let Some(entry) = entry {
        entry.set_text("");
    }
}

fn text_input_has_focus(state: &Rc<RefCell<UiState>>) -> bool {
    let ui = state.borrow();
    ui.search_entry
        .as_ref()
        .is_some_and(widget_has_focus_within)
        || ui
            .connection_server_entry
            .as_ref()
            .is_some_and(widget_has_focus_within)
        || ui
            .connection_username_entry
            .as_ref()
            .is_some_and(widget_has_focus_within)
        || ui
            .connection_password_entry
            .as_ref()
            .is_some_and(widget_has_focus_within)
        || ui
            .radio_name_entry
            .as_ref()
            .is_some_and(widget_has_focus_within)
        || ui
            .radio_url_entry
            .as_ref()
            .is_some_and(widget_has_focus_within)
}

fn widget_has_focus_within(widget: &impl IsA<gtk::Widget>) -> bool {
    let widget = widget.as_ref();
    widget.has_focus()
        || widget.is_focus()
        || widget
            .focus_child()
            .is_some_and(|child| widget_has_focus_within(&child))
}

fn navigate_invisible_search(state: &Rc<RefCell<UiState>>, query: &str) -> bool {
    let normalized_query = query.trim().to_lowercase();
    if normalized_query.is_empty() {
        return false;
    }

    let (visible_content, target) = {
        let ui = state.borrow();
        let visible_content = visible_library_content(&ui);
        let target = match visible_content {
            VisibleLibraryContent::Tracks => ui
                .tracks
                .iter()
                .enumerate()
                .filter_map(|(index, track)| {
                    invisible_track_search_rank(track, &normalized_query).map(|rank| (index, rank))
                })
                .min_by(|(_, left), (_, right)| left.cmp(right))
                .map(|(index, _)| index),
            VisibleLibraryContent::Albums => visible_album_summaries(&ui)
                .iter()
                .enumerate()
                .filter_map(|(index, album)| {
                    invisible_search_rank(
                        [album.name.as_str(), album.artist.as_str()],
                        &normalized_query,
                    )
                    .map(|rank| (index, rank))
                })
                .min_by(|(_, left), (_, right)| left.cmp(right))
                .map(|(index, _)| index),
            VisibleLibraryContent::Artists => {
                filter_artist_summaries(&ui.library_artists, &ui.search_query)
                    .iter()
                    .enumerate()
                    .filter_map(|(index, artist)| {
                        invisible_search_rank([artist.name.as_str()], &normalized_query)
                            .map(|rank| (index, rank))
                    })
                    .min_by(|(_, left), (_, right)| left.cmp(right))
                    .map(|(index, _)| index)
            }
            VisibleLibraryContent::Playlists => filter_playlists(&ui.playlists, &ui.search_query)
                .iter()
                .enumerate()
                .filter_map(|(index, playlist)| {
                    invisible_search_rank([playlist.name.as_str()], &normalized_query)
                        .map(|rank| (index, rank))
                })
                .min_by(|(_, left), (_, right)| left.cmp(right))
                .map(|(index, _)| index),
            VisibleLibraryContent::Radio | VisibleLibraryContent::NextUp => None,
        };
        (visible_content, target)
    };

    let Some(index) = target else {
        return false;
    };

    match visible_content {
        VisibleLibraryContent::Tracks => select_track_for_navigation(state, index),
        VisibleLibraryContent::Albums
        | VisibleLibraryContent::Artists
        | VisibleLibraryContent::Playlists
        | VisibleLibraryContent::Radio
        | VisibleLibraryContent::NextUp => focus_collection_item(state, index),
    }

    true
}

fn activate_invisible_collection_match(state: &Rc<RefCell<UiState>>, query: &str) -> bool {
    let normalized_query = query.trim().to_lowercase();
    if normalized_query.is_empty() {
        return false;
    }

    enum CollectionMatch {
        Album(AlbumSummary),
        Artist(ArtistSummary),
    }

    let target = {
        let ui = state.borrow();
        match visible_library_content(&ui) {
            VisibleLibraryContent::Albums => visible_album_summaries(&ui)
                .into_iter()
                .filter_map(|album| {
                    invisible_search_rank(
                        [album.name.as_str(), album.artist.as_str()],
                        &normalized_query,
                    )
                    .map(|rank| (album, rank))
                })
                .min_by(|(_, left), (_, right)| left.cmp(right))
                .map(|(album, _)| CollectionMatch::Album(album)),
            VisibleLibraryContent::Artists => {
                filter_artist_summaries(&ui.library_artists, &ui.search_query)
                    .into_iter()
                    .filter_map(|artist| {
                        invisible_search_rank([artist.name.as_str()], &normalized_query)
                            .map(|rank| (artist, rank))
                    })
                    .min_by(|(_, left), (_, right)| left.cmp(right))
                    .map(|(artist, _)| CollectionMatch::Artist(artist))
            }
            VisibleLibraryContent::Tracks
            | VisibleLibraryContent::Playlists
            | VisibleLibraryContent::NextUp => None,
            VisibleLibraryContent::Radio => None,
        }
    };

    match target {
        Some(CollectionMatch::Album(album)) => {
            show_album_tracks(state, &album);
            true
        }
        Some(CollectionMatch::Artist(artist)) => {
            show_artist_albums(state, &artist);
            true
        }
        None => false,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VisibleLibraryContent {
    Tracks,
    Albums,
    Artists,
    Playlists,
    Radio,
    NextUp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NavDirection {
    DrillForward,
    DrillBackward,
    PageForward,
    PageBackward,
}

#[derive(Clone, Debug)]
struct CollectionReturnTarget {
    content: VisibleLibraryContent,
    key: String,
}

fn visible_library_content(ui: &UiState) -> VisibleLibraryContent {
    let show_detail =
        ui.album_filter.is_some() || ui.artist_filter.is_some() || ui.playlist_filter.is_some();
    match (ui.active_page, show_detail) {
        (LibraryPage::Tracks, _) => VisibleLibraryContent::Tracks,
        (LibraryPage::Albums, false) => VisibleLibraryContent::Albums,
        (LibraryPage::Albums, true) => VisibleLibraryContent::Tracks,
        (LibraryPage::Artists, false) => VisibleLibraryContent::Artists,
        (LibraryPage::Artists, true) if ui.album_filter.is_none() => VisibleLibraryContent::Albums,
        (LibraryPage::Artists, true) => VisibleLibraryContent::Tracks,
        (LibraryPage::Playlists, false) => VisibleLibraryContent::Playlists,
        (LibraryPage::Playlists, true) => VisibleLibraryContent::Tracks,
        (LibraryPage::Radio, _) => VisibleLibraryContent::Radio,
        (LibraryPage::NextUp, _) => VisibleLibraryContent::NextUp,
    }
}

fn visible_album_summaries(ui: &UiState) -> Vec<AlbumSummary> {
    if ui.active_page == LibraryPage::Artists {
        ui.artist_filter
            .as_deref()
            .map(|selected_artist_key| {
                album_summaries_for_artist_from(
                    &ui.library_albums,
                    selected_artist_key,
                    &ui.search_query,
                )
            })
            .unwrap_or_else(|| filter_album_summaries(&ui.library_albums, &ui.search_query))
    } else {
        filter_album_summaries(&ui.library_albums, &ui.search_query)
    }
}

fn invisible_track_search_rank(track: &UiTrack, query: &str) -> Option<(u8, u8, usize)> {
    [
        (track.title.as_str(), 0),
        (track.artist.as_str(), 1),
        (track.album.as_str(), 2),
    ]
    .into_iter()
    .filter_map(|(text, field_rank)| {
        invisible_text_search_rank(text, query)
            .map(|(match_rank, match_index)| (match_rank, field_rank, match_index))
    })
    .min()
}

fn invisible_search_rank<'a>(
    texts: impl IntoIterator<Item = &'a str>,
    query: &str,
) -> Option<(u8, u8, usize)> {
    texts
        .into_iter()
        .enumerate()
        .filter_map(|(field_rank, text)| {
            invisible_text_search_rank(text, query)
                .map(|(match_rank, match_index)| (match_rank, field_rank as u8, match_index))
        })
        .min()
}

fn invisible_text_search_rank(text: &str, query: &str) -> Option<(u8, usize)> {
    let normalized_text = text.to_lowercase();
    let full_match_index = normalized_text.find(query)?;
    let word_match_index = normalized_text
        .match_indices(query)
        .find_map(|(index, _)| is_word_boundary(&normalized_text, index).then_some(index));

    let rank = if normalized_text.starts_with(query) {
        0
    } else if let Some(index) = word_match_index {
        return Some((1, index));
    } else {
        2
    };

    Some((rank, full_match_index))
}

fn is_word_boundary(text: &str, index: usize) -> bool {
    if index == 0 {
        return true;
    }

    text[..index]
        .chars()
        .next_back()
        .is_none_or(|character| !character.is_alphanumeric())
}

fn select_track_for_navigation(state: &Rc<RefCell<UiState>>, index: usize) {
    let selected_index = {
        let mut ui = state.borrow_mut();
        if ui.tracks.is_empty() {
            return;
        }
        ui.selected_index = index.min(ui.tracks.len() - 1);
        let selected_index = ui.selected_index;
        if ui.playback_session.queue_tracks.is_empty() {
            rebuild_playback_order(&mut ui, selected_index);
        }
        update_now_playing_labels(&ui);
        update_play_button(&ui);
        selected_index
    };

    select_track_model_row(state, selected_index);
    scroll_track_list_to_index(state, selected_index);
    rebuild_queue_list(state);
    load_selected_cover_art(state);
    load_selected_waveform(state);
}

fn scroll_track_list_to_index(state: &Rc<RefCell<UiState>>, index: usize) {
    let (stack, track_count) = {
        let ui = state.borrow();
        (ui.track_stack.clone(), ui.tracks.len())
    };
    if track_count == 0 {
        return;
    }

    let scroll = stack
        .as_ref()
        .and_then(|s| s.child_by_name("list"))
        .and_then(|c| c.downcast::<gtk::ScrolledWindow>().ok());

    if let Some(scroll) = scroll {
        let adj = scroll.vadjustment();
        let row_height = adj.upper() / track_count as f64;
        let target = index as f64 * row_height;
        adj.set_value(target.clamp(0.0, adj.upper() - adj.page_size()));
    }
}

fn focus_collection_item(state: &Rc<RefCell<UiState>>, index: usize) {
    let (grid, visible_content) = {
        let ui = state.borrow();
        let grid = match visible_library_content(&ui) {
            VisibleLibraryContent::Albums => ui.album_grid.clone(),
            VisibleLibraryContent::Artists => ui.artist_grid.clone(),
            VisibleLibraryContent::Playlists => ui.playlist_grid.clone(),
            VisibleLibraryContent::Radio => None,
            VisibleLibraryContent::Tracks | VisibleLibraryContent::NextUp => None,
        };
        (grid, visible_library_content(&ui))
    };
    if matches!(
        visible_content,
        VisibleLibraryContent::Tracks | VisibleLibraryContent::NextUp
    ) {
        select_track_for_navigation(state, index);
        return;
    }

    let Some(grid) = grid else {
        return;
    };
    let Some(child) = grid.child_at_index(index as i32) else {
        return;
    };

    if let Some(button) = collection_child_button(&child) {
        button.grab_focus();
    } else {
        child.grab_focus();
    }
}

fn escape_back_or_top(state: &Rc<RefCell<UiState>>) {
    if has_collection_detail_open(state) {
        return_to_collection_grid(state);
    } else {
        focus_active_content_top(state);
    }
}

fn has_collection_detail_open(state: &Rc<RefCell<UiState>>) -> bool {
    let ui = state.borrow();
    ui.album_filter.is_some() || ui.artist_filter.is_some() || ui.playlist_filter.is_some()
}

fn focus_active_content(state: &Rc<RefCell<UiState>>) {
    if state.borrow().is_track_list_visible() {
        focus_track_list(state);
    } else {
        focus_active_collection_grid(state);
    }
}

fn focus_active_content_top(state: &Rc<RefCell<UiState>>) {
    if state.borrow().is_track_list_visible() {
        let has_tracks = !state.borrow().tracks.is_empty();
        if has_tracks {
            select_track_for_navigation(state, 0);
        } else {
            scroll_track_list_to_top(state);
            focus_track_list(state);
        }
    } else {
        scroll_active_collection_grid_to_top(state);
        focus_active_collection_grid(state);
    }
}

fn focus_track_list(state: &Rc<RefCell<UiState>>) {
    let Some(scroll) = track_list_scroll(state) else {
        return;
    };

    if let Some(list) = scroll
        .child()
        .and_then(|widget| widget.downcast::<gtk::ColumnView>().ok())
    {
        list.grab_focus();
    } else {
        scroll.grab_focus();
    }
}

fn scroll_track_list_to_top(state: &Rc<RefCell<UiState>>) {
    if let Some(scroll) = track_list_scroll(state) {
        scroll.vadjustment().set_value(0.0);
    }
}

fn track_list_scroll(state: &Rc<RefCell<UiState>>) -> Option<gtk::ScrolledWindow> {
    state
        .borrow()
        .track_stack
        .as_ref()
        .and_then(|stack| stack.child_by_name("list"))
        .and_then(|child| child.downcast::<gtk::ScrolledWindow>().ok())
}

fn scroll_active_collection_grid_to_top(state: &Rc<RefCell<UiState>>) {
    let Some(grid) = active_collection_grid(state) else {
        return;
    };

    if let Some(scroll) = collection_grid_scroll(&grid) {
        scroll.vadjustment().set_value(0.0);
    }
}

fn collection_grid_scroll(grid: &gtk::FlowBox) -> Option<gtk::ScrolledWindow> {
    let mut parent = grid.parent();
    while let Some(widget) = parent {
        if let Ok(scroll) = widget.clone().downcast::<gtk::ScrolledWindow>() {
            return Some(scroll);
        }
        parent = widget.parent();
    }
    None
}

fn activate_focused_collection_item(state: &Rc<RefCell<UiState>>) -> bool {
    let Some(grid) = active_collection_grid(state) else {
        return false;
    };

    let mut child = grid.first_child();
    while let Some(widget) = child {
        if widget_has_focus_within(&widget)
            && let Some(button) = widget
                .clone()
                .downcast::<gtk::FlowBoxChild>()
                .ok()
                .and_then(|child| collection_child_button(&child))
                .or_else(|| widget.clone().downcast::<gtk::Button>().ok())
        {
            button.emit_clicked();
            return true;
        }
        child = widget.next_sibling();
    }

    false
}

fn collection_child_button(child: &gtk::FlowBoxChild) -> Option<gtk::Button> {
    child
        .first_child()
        .and_then(|widget| widget.downcast::<gtk::Button>().ok())
}

fn active_collection_grid(state: &Rc<RefCell<UiState>>) -> Option<gtk::FlowBox> {
    let ui = state.borrow();
    match visible_library_content(&ui) {
        VisibleLibraryContent::Albums => ui.album_grid.clone(),
        VisibleLibraryContent::Artists => ui.artist_grid.clone(),
        VisibleLibraryContent::Playlists => ui.playlist_grid.clone(),
        VisibleLibraryContent::Radio => None,
        VisibleLibraryContent::Tracks | VisibleLibraryContent::NextUp => None,
    }
}

fn active_collection_grid_and_content(
    state: &Rc<RefCell<UiState>>,
) -> Option<(VisibleLibraryContent, gtk::FlowBox)> {
    let ui = state.borrow();
    let content = visible_library_content(&ui);
    let grid = match content {
        VisibleLibraryContent::Albums => ui.album_grid.clone(),
        VisibleLibraryContent::Artists => ui.artist_grid.clone(),
        VisibleLibraryContent::Playlists => ui.playlist_grid.clone(),
        VisibleLibraryContent::Radio => None,
        VisibleLibraryContent::Tracks | VisibleLibraryContent::NextUp => None,
    }?;
    Some((content, grid))
}

fn collection_return_target_for_key(
    state: &Rc<RefCell<UiState>>,
    key: String,
) -> Option<CollectionReturnTarget> {
    let (content, _) = active_collection_grid_and_content(state)?;
    Some(CollectionReturnTarget { content, key })
}

fn save_active_collection_scroll_position(state: &Rc<RefCell<UiState>>) {
    let Some((content, grid)) = active_collection_grid_and_content(state) else {
        return;
    };
    let Some(scroll) = collection_grid_scroll(&grid) else {
        return;
    };
    let value = scroll.vadjustment().value();
    let mut ui = state.borrow_mut();
    set_collection_scroll_value(&mut ui, content, value);
}

fn restore_active_collection_scroll_position(state: &Rc<RefCell<UiState>>) {
    let Some((content, grid)) = active_collection_grid_and_content(state) else {
        return;
    };
    let Some(scroll) = collection_grid_scroll(&grid) else {
        return;
    };
    let value = {
        let ui = state.borrow();
        collection_scroll_value(&ui, content)
    };
    restore_collection_scroll(scroll, value);
}

fn set_collection_scroll_value(ui: &mut UiState, content: VisibleLibraryContent, value: f64) {
    match content {
        VisibleLibraryContent::Albums => ui.album_grid_scroll_value = value,
        VisibleLibraryContent::Artists => ui.artist_grid_scroll_value = value,
        VisibleLibraryContent::Playlists => ui.playlist_grid_scroll_value = value,
        VisibleLibraryContent::Radio
        | VisibleLibraryContent::Tracks
        | VisibleLibraryContent::NextUp => {}
    }
}

fn collection_scroll_value(ui: &UiState, content: VisibleLibraryContent) -> f64 {
    match content {
        VisibleLibraryContent::Albums => ui.album_grid_scroll_value,
        VisibleLibraryContent::Artists => ui.artist_grid_scroll_value,
        VisibleLibraryContent::Playlists => ui.playlist_grid_scroll_value,
        VisibleLibraryContent::Radio
        | VisibleLibraryContent::Tracks
        | VisibleLibraryContent::NextUp => 0.0,
    }
}

fn restore_collection_scroll(scroll: gtk::ScrolledWindow, value: f64) {
    let attempts = Rc::new(Cell::new(0usize));
    gtk::glib::idle_add_local(move || {
        let adj = scroll.vadjustment();
        let max = (adj.upper() - adj.page_size()).max(0.0);
        adj.set_value(value.clamp(0.0, max));

        let attempt = attempts.get() + 1;
        attempts.set(attempt);
        if value <= max || attempt >= 60 {
            gtk::glib::ControlFlow::Break
        } else {
            gtk::glib::ControlFlow::Continue
        }
    });
}

fn pulse_collection_return_target(
    state: &Rc<RefCell<UiState>>,
    target: Option<CollectionReturnTarget>,
) {
    let Some(target) = target else {
        return;
    };

    let state = state.clone();
    let attempts = Rc::new(Cell::new(0usize));
    gtk::glib::idle_add_local(move || {
        if let Some(button) = collection_return_target_button(&state, &target) {
            button.add_css_class("return-highlight");
            gtk::glib::timeout_add_local_once(
                Duration::from_millis(COLLECTION_RETURN_HIGHLIGHT_MS),
                move || {
                    button.remove_css_class("return-highlight");
                },
            );
            return gtk::glib::ControlFlow::Break;
        }

        let attempt = attempts.get() + 1;
        attempts.set(attempt);
        if attempt >= 60 {
            gtk::glib::ControlFlow::Break
        } else {
            gtk::glib::ControlFlow::Continue
        }
    });
}

fn collection_return_target_button(
    state: &Rc<RefCell<UiState>>,
    target: &CollectionReturnTarget,
) -> Option<gtk::Button> {
    let (grid, index) = {
        let ui = state.borrow();
        if visible_library_content(&ui) != target.content {
            return None;
        }
        let grid = collection_grid_for_content(&ui, target.content)?;
        let index = collection_return_target_index(&ui, target)?;
        (grid, index)
    };

    grid.child_at_index(index as i32)
        .and_then(|child| collection_child_button(&child))
}

fn collection_grid_for_content(
    ui: &UiState,
    content: VisibleLibraryContent,
) -> Option<gtk::FlowBox> {
    match content {
        VisibleLibraryContent::Albums => ui.album_grid.clone(),
        VisibleLibraryContent::Artists => ui.artist_grid.clone(),
        VisibleLibraryContent::Playlists => ui.playlist_grid.clone(),
        VisibleLibraryContent::Radio
        | VisibleLibraryContent::Tracks
        | VisibleLibraryContent::NextUp => None,
    }
}

fn collection_return_target_index(ui: &UiState, target: &CollectionReturnTarget) -> Option<usize> {
    match target.content {
        VisibleLibraryContent::Albums => visible_album_summaries(ui)
            .iter()
            .position(|album| album.key == target.key),
        VisibleLibraryContent::Artists => {
            filter_artist_summaries(&ui.library_artists, &ui.search_query)
                .iter()
                .position(|artist| artist.key == target.key)
        }
        VisibleLibraryContent::Playlists => filter_playlists(&ui.playlists, &ui.search_query)
            .iter()
            .position(|playlist| playlist.id == target.key),
        VisibleLibraryContent::Radio
        | VisibleLibraryContent::Tracks
        | VisibleLibraryContent::NextUp => None,
    }
}

fn setup_mpris(state: Rc<RefCell<UiState>>) {
    let config = PlatformConfig {
        dbus_name: "org.mpris.MediaPlayer2.gtunes",
        display_name: "gTunes",
        hwnd: None,
    };

    let mut controls = match MediaControls::new(config) {
        Ok(controls) => controls,
        Err(error) => {
            tracing::warn!(%error, "failed to initialize MPRIS controls");
            return;
        }
    };

    let (sender, receiver) = mpsc::channel::<MediaControlEvent>();
    poll_mpris_events(receiver, state.clone());

    let result = controls.attach(move |event| {
        let _ = sender.send(event);
    });

    if let Err(error) = result {
        tracing::warn!(%error, "failed to attach MPRIS event handler");
    } else {
        state.borrow_mut().mpris = Some(controls);
    }
}

fn poll_mpris_events(receiver: mpsc::Receiver<MediaControlEvent>, state: Rc<RefCell<UiState>>) {
    gtk::glib::timeout_add_local(Duration::from_millis(100), move || {
        loop {
            match receiver.try_recv() {
                Ok(event) => handle_mpris_event(&state, event),
                Err(mpsc::TryRecvError::Empty) => return gtk::glib::ControlFlow::Continue,
                Err(mpsc::TryRecvError::Disconnected) => return gtk::glib::ControlFlow::Break,
            }
        }
    });
}

fn handle_mpris_event(state: &Rc<RefCell<UiState>>, event: MediaControlEvent) {
    match event {
        MediaControlEvent::Play => resume_playback(state),
        MediaControlEvent::Pause => pause_playback(state),
        MediaControlEvent::Toggle => toggle_play_pause(state),
        MediaControlEvent::Next if !state.borrow().playback_session.mode.is_radio() => {
            play_next_track(state);
        }
        MediaControlEvent::Previous if !state.borrow().playback_session.mode.is_radio() => {
            play_previous_track(state);
        }
        MediaControlEvent::Next | MediaControlEvent::Previous => {}
        MediaControlEvent::Stop => {
            let mut ui = state.borrow_mut();
            stop_playback(&mut ui);
            update_now_playing_labels(&ui);
            update_play_button(&ui);
        }
        MediaControlEvent::Seek(direction) => {
            let mut ui = state.borrow_mut();
            let pos = ui
                .playback
                .as_ref()
                .and_then(|playback| playback.position());
            if let (Some(playback), Some(pos)) = (ui.playback.as_mut(), pos) {
                let offset = Duration::from_secs(10);
                let new_pos = match direction {
                    souvlaki::SeekDirection::Forward => pos + offset,
                    souvlaki::SeekDirection::Backward => pos.saturating_sub(offset),
                };
                let _ = playback.seek(new_pos);
            }
        }
        MediaControlEvent::SeekBy(direction, offset) => {
            let mut ui = state.borrow_mut();
            let pos = ui
                .playback
                .as_ref()
                .and_then(|playback| playback.position());
            if let (Some(playback), Some(pos)) = (ui.playback.as_mut(), pos) {
                let new_pos = match direction {
                    souvlaki::SeekDirection::Forward => pos + offset,
                    souvlaki::SeekDirection::Backward => pos.saturating_sub(offset),
                };
                let _ = playback.seek(new_pos);
            }
        }
        MediaControlEvent::SetPosition(pos) => {
            let mut ui = state.borrow_mut();
            if let Some(playback) = ui.playback.as_mut() {
                let _ = playback.seek(pos.0);
            }
        }
        _ => {}
    }
}

fn update_mpris_status(ui: &mut UiState) {
    let Some(mpris) = ui.mpris.as_mut() else {
        return;
    };

    let progress = ui
        .playback
        .as_ref()
        .and_then(PlaybackEngine::position)
        .map(souvlaki::MediaPosition);

    let status = match ui.playback.as_ref().map(PlaybackEngine::state) {
        Some(PlaybackState::Playing) => MediaPlayback::Playing { progress },
        Some(PlaybackState::Paused) => MediaPlayback::Paused { progress },
        _ => MediaPlayback::Stopped,
    };

    if let Err(error) = mpris.set_playback(status) {
        tracing::warn!(%error, "failed to update MPRIS playback status");
    }
}

fn update_mpris_metadata(ui: &mut UiState) {
    if let Some(station) = current_radio_station(ui) {
        let artist = station.mpris_source_label().to_string();
        let title = station.name;
        let album = "Radio".to_string();

        let Some(mpris) = ui.mpris.as_mut() else {
            return;
        };

        let metadata = MediaMetadata {
            title: Some(&title),
            artist: Some(&artist),
            album: Some(&album),
            duration: None,
            cover_url: None,
        };

        if let Err(error) = mpris.set_metadata(metadata) {
            tracing::warn!(%error, "failed to update MPRIS metadata");
        }
        return;
    }

    let Some(track) = current_display_track(ui) else {
        return;
    };
    let title = track.title.clone();
    let artist = track.artist.clone();
    let album = track.album.clone();
    let artwork_url = track.artwork_url.clone();

    let Some(mpris) = ui.mpris.as_mut() else {
        return;
    };

    let duration = ui.playback.as_ref().and_then(PlaybackEngine::duration);
    let cover_url = artwork_url.as_deref().map(|url| {
        let path = artwork_cache_path(url);
        format!("file://{}", path.to_string_lossy())
    });

    let metadata = MediaMetadata {
        title: Some(&title),
        artist: Some(&artist),
        album: Some(&album),
        duration,
        cover_url: cover_url.as_deref(),
    };

    if let Err(error) = mpris.set_metadata(metadata) {
        tracing::warn!(%error, "failed to update MPRIS metadata");
    }
}

fn sync_external_playback_status(ui: &mut UiState) {
    sync_discord_presence(ui);
    update_mpris_status(ui);
}

fn sync_external_playback_metadata(ui: &mut UiState) {
    sync_discord_presence(ui);
    update_mpris_metadata(ui);
}

fn sync_external_playback(ui: &mut UiState) {
    sync_discord_presence(ui);
    update_mpris_metadata(ui);
    update_mpris_status(ui);
}

fn sync_discord_presence(ui: &UiState) {
    let Some(discord) = ui.discord_presence.as_ref() else {
        return;
    };

    let playback_state = match ui.playback.as_ref().map(PlaybackEngine::state) {
        Some(PlaybackState::Playing) => PresencePlaybackState::Playing,
        Some(PlaybackState::Paused) => PresencePlaybackState::Paused,
        _ => {
            discord.clear_activity();
            return;
        }
    };

    if let Some(station) = current_radio_station(ui) {
        let title = station.name.clone();
        let artist = station.source_label().to_string();
        discord.set_activity(PresenceActivity {
            title,
            artist,
            album: Some("Radio".to_string()),
            artwork_source_url: None,
            playback_state,
            position: None,
            duration: None,
        });
        return;
    }

    let Some(track) = current_display_track(ui) else {
        discord.clear_activity();
        return;
    };

    discord.set_activity(PresenceActivity {
        title: track.title.clone(),
        artist: track.artist.clone(),
        album: Some(track.album.clone()),
        artwork_source_url: track
            .thumbnail_artwork_url
            .as_deref()
            .or(track.artwork_url.as_deref())
            .map(str::to_string),
        playback_state,
        position: ui.playback.as_ref().and_then(PlaybackEngine::position),
        duration: ui.playback.as_ref().and_then(PlaybackEngine::duration),
    });
}

fn build_player_bar(state: Rc<RefCell<UiState>>) -> gtk::Box {
    let player = gtk::Box::new(Orientation::Horizontal, 14);
    player.add_css_class("player-bar");
    player.set_valign(Align::Start);

    let transport = gtk::Box::new(Orientation::Horizontal, 2);
    transport.add_css_class("transport");
    transport.set_valign(Align::Center);
    let previous = icon_button("media-skip-backward-symbolic", "Previous track");
    {
        let state = state.clone();
        previous.connect_clicked(move |_| {
            play_previous_track(&state);
        });
    }
    transport.append(&previous);
    let play = icon_button("media-playback-start-symbolic", "Play");
    play.add_css_class("play-button");
    state.borrow_mut().play_button = Some(play.clone());
    {
        let state = state.clone();
        play.connect_clicked(move |_| {
            toggle_play_pause(&state);
        });
    }
    let play_overlay = gtk::Overlay::new();
    play_overlay.set_child(Some(&play));
    let loading_spinner = gtk::Spinner::new();
    loading_spinner.add_css_class("play-loading-spinner");
    loading_spinner.set_halign(Align::Center);
    loading_spinner.set_valign(Align::Center);
    loading_spinner.set_visible(false);
    play_overlay.add_overlay(&loading_spinner);
    state.borrow_mut().loading_spinner = Some(loading_spinner);
    transport.append(&play_overlay);
    let next = icon_button("media-skip-forward-symbolic", "Next track");
    {
        let state = state.clone();
        next.connect_clicked(move |_| {
            play_next_track(&state);
        });
    }
    transport.append(&next);
    player.append(&transport);

    let wave = gtk::Box::new(Orientation::Vertical, 7);
    wave.add_css_class("wave-card");
    wave.set_hexpand(true);
    wave.set_halign(Align::Fill);

    let wave_track = gtk::Box::new(Orientation::Vertical, 2);
    wave_track.set_halign(Align::Center);
    wave_track.set_hexpand(true);
    state.borrow().now_title.set_xalign(0.5);
    state.borrow().now_title.set_halign(Align::Center);
    state.borrow().now_title.set_single_line_mode(true);
    state.borrow().now_title.set_lines(1);
    state.borrow().now_meta.set_xalign(0.5);
    state.borrow().now_meta.set_halign(Align::Center);
    state.borrow().now_meta.set_single_line_mode(true);
    state.borrow().now_meta.set_lines(1);
    state
        .borrow()
        .now_meta
        .set_cursor_from_name(Some("pointer"));
    state.borrow().playback_status.set_xalign(0.5);
    state.borrow().playback_status.set_halign(Align::Center);
    state.borrow().playback_status.set_single_line_mode(true);
    state.borrow().playback_status.set_lines(1);
    {
        let state_click = state.clone();
        let click = gtk::GestureClick::new();
        click.connect_pressed(move |_, _, _, _| {
            if state_click.borrow().playback_session.mode.is_radio() {
                set_library_page(&state_click, LibraryPage::Radio);
            } else {
                scroll_to_now_playing(&state_click);
            }
        });
        state.borrow().now_title.add_controller(click);
        state
            .borrow()
            .now_title
            .set_cursor_from_name(Some("pointer"));
    }
    {
        let state_click = state.clone();
        state
            .borrow()
            .now_meta
            .connect_activate_link(move |_, uri| {
                match uri {
                    "gtunes:artist" => navigate_to_now_playing_artist(&state_click),
                    "gtunes:album" => navigate_to_now_playing_album(&state_click),
                    _ => {}
                }
                gtk::glib::Propagation::Stop
            });
    }
    wave_track.append(&state.borrow().now_title);
    wave_track.append(&state.borrow().now_meta);
    wave_track.append(&state.borrow().playback_status);
    wave.append(&wave_track);

    let wave_area = waveform_widget(state.clone());
    state.borrow_mut().wave_area = Some(wave_area.clone());
    wave.append(&wave_area);

    let wave_footer = gtk::Box::new(Orientation::Horizontal, 8);
    wave_footer.append(&state.borrow().elapsed_label);
    let wave_footer_spacer = gtk::Box::new(Orientation::Horizontal, 0);
    wave_footer_spacer.set_hexpand(true);
    wave_footer.append(&wave_footer_spacer);
    state.borrow().remaining_label.set_xalign(1.0);
    wave_footer.append(&state.borrow().remaining_label);
    wave.append(&wave_footer);
    player.append(&wave);

    let actions = gtk::Overlay::new();
    actions.set_valign(Align::Fill);
    actions.set_halign(Align::End);
    actions.set_hexpand(false);
    actions.set_size_request(ACTION_PANEL_WIDTH, -1);

    let search_centerer = gtk::Box::new(Orientation::Vertical, 0);
    search_centerer.set_valign(Align::Center);

    let search = gtk::SearchEntry::new();
    search.add_css_class("search");
    search.set_hexpand(true);
    search.set_halign(Align::Fill);
    search.set_size_request(0, -1);
    search.set_placeholder_text(Some("Search library"));
    {
        let state = state.clone();
        search.connect_search_changed(move |entry| {
            set_search_query(&state, entry.text().trim());
        });
    }
    state.borrow_mut().search_entry = Some(search.clone());
    search_centerer.append(&search);
    actions.set_child(Some(&search_centerer));

    let utility_row = gtk::Box::new(Orientation::Horizontal, 4);
    utility_row.set_halign(Align::End);
    utility_row.set_valign(Align::Start);
    utility_row.set_margin_end(PLAYER_ACTION_EDGE_INSET);

    let shuffle = icon_button("media-playlist-shuffle-symbolic", "Shuffle");
    shuffle.add_css_class("toolbar-button");
    shuffle.add_css_class("shuffle-toggle");
    shuffle.add_css_class("shuffle-off");
    {
        let state = state.clone();
        shuffle.connect_clicked(move |_| {
            toggle_shuffle(&state);
        });
    }
    state.borrow_mut().shuffle_button = Some(shuffle.clone());
    utility_row.append(&shuffle);

    let settings = settings_menu_button(state.clone());
    utility_row.append(&settings);

    actions.add_overlay(&utility_row);

    player.append(&actions);
    connect_player_bar_responsive_layout(&player, &actions, &state.borrow().playback_status);

    player
}

fn settings_menu_button(state: Rc<RefCell<UiState>>) -> gtk::MenuButton {
    let settings = gtk::MenuButton::builder()
        .icon_name("emblem-system-symbolic")
        .tooltip_text("Settings")
        .build();
    settings.add_css_class("icon-button");
    settings.add_css_class("toolbar-button");
    settings.add_css_class("settings-menu-button");

    let popover = gtk::Popover::new();
    popover.add_css_class("settings-popover");
    let menu = gtk::Box::new(Orientation::Vertical, 4);
    menu.add_css_class("settings-popover-menu");
    menu.set_margin_top(10);
    menu.set_margin_bottom(10);
    menu.set_margin_start(10);
    menu.set_margin_end(10);
    menu.set_width_request(280);

    let keep_row = gtk::Box::new(Orientation::Horizontal, 12);
    keep_row.add_css_class("settings-switch-row");
    keep_row.set_margin_top(2);
    keep_row.set_margin_bottom(6);
    keep_row.set_margin_start(6);
    keep_row.set_margin_end(6);
    let keep_label = label("Keep playing while closed", "settings-menu-label");
    keep_label.set_hexpand(true);
    keep_label.set_halign(Align::Start);
    let keep_switch = gtk::Switch::builder()
        .active(state.borrow().keep_playing_while_closed)
        .valign(Align::Center)
        .build();
    {
        let state = state.clone();
        keep_switch.connect_active_notify(move |switch| {
            set_keep_playing_while_closed(&state, switch.is_active());
        });
    }
    keep_row.append(&keep_label);
    keep_row.append(&keep_switch);
    menu.append(&keep_row);

    let anim_row = gtk::Box::new(Orientation::Horizontal, 12);
    anim_row.add_css_class("settings-switch-row");
    anim_row.set_margin_top(2);
    anim_row.set_margin_bottom(6);
    anim_row.set_margin_start(6);
    anim_row.set_margin_end(6);
    let anim_label = label("Animations", "settings-menu-label");
    anim_label.set_hexpand(true);
    anim_label.set_halign(Align::Start);
    let anim_switch = gtk::Switch::builder()
        .active(state.borrow().animations_enabled)
        .valign(Align::Center)
        .build();
    {
        let state = state.clone();
        let settings = settings.clone();
        anim_switch.connect_active_notify(move |switch| {
            let enabled = switch.is_active();
            set_animations_enabled(&state, enabled);
            if let Some(window) = settings
                .root()
                .and_then(|r| r.downcast::<gtk::Window>().ok())
            {
                if enabled {
                    window.remove_css_class("no-animations");
                } else {
                    window.add_css_class("no-animations");
                }
            }
        });
    }
    anim_row.append(&anim_label);
    anim_row.append(&anim_switch);
    menu.append(&anim_row);

    let font_row = gtk::Box::new(Orientation::Horizontal, 12);
    font_row.add_css_class("settings-switch-row");
    font_row.set_margin_top(2);
    font_row.set_margin_bottom(6);
    font_row.set_margin_start(6);
    font_row.set_margin_end(6);
    let font_label = label("Font Style", "settings-menu-label");
    font_label.set_hexpand(true);
    font_label.set_halign(Align::Start);
    let font_btn_default = gtk::ToggleButton::builder()
        .icon_name("font-x-generic-symbolic")
        .tooltip_text("Default")
        .active(!state.borrow().font_mono)
        .build();
    font_btn_default.add_css_class("font-style-toggle");
    let font_btn_mono = gtk::ToggleButton::builder()
        .icon_name("utilities-terminal-symbolic")
        .tooltip_text("Monospace")
        .active(state.borrow().font_mono)
        .group(&font_btn_default)
        .build();
    font_btn_mono.add_css_class("font-style-toggle");
    let font_toggle_box = gtk::Box::new(Orientation::Horizontal, 0);
    font_toggle_box.add_css_class("linked");
    font_toggle_box.append(&font_btn_default);
    font_toggle_box.append(&font_btn_mono);
    {
        let state = state.clone();
        let settings = settings.clone();
        font_btn_mono.connect_toggled(move |btn| {
            let mono = btn.is_active();
            set_font_mono(&state, mono);
            if let Some(window) = settings
                .root()
                .and_then(|r| r.downcast::<gtk::Window>().ok())
            {
                if mono {
                    window.add_css_class("font-mono");
                } else {
                    window.remove_css_class("font-mono");
                }
            }
        });
    }
    font_row.append(&font_label);
    font_row.append(&font_toggle_box);
    menu.append(&font_row);

    menu.append(&gtk::Separator::new(Orientation::Horizontal));

    let refresh = menu_item_button("view-refresh-symbolic", "Refresh library");
    refresh.set_sensitive(false);
    {
        let state = state.clone();
        let popover = popover.clone();
        refresh.connect_clicked(move |button| {
            popover.popdown();
            refresh_jellyfin_library(state.clone(), button.clone());
        });
    }
    state.borrow_mut().refresh_button = Some(refresh.clone());
    menu.append(&refresh);

    let shortcuts = menu_item_button(
        "preferences-desktop-keyboard-shortcuts-symbolic",
        "Keyboard shortcuts",
    );
    {
        let popover = popover.clone();
        let settings = settings.clone();
        shortcuts.connect_clicked(move |_| {
            popover.popdown();
            if let Some(window) = settings
                .root()
                .and_then(|root| root.downcast::<gtk::Window>().ok())
            {
                show_keyboard_shortcuts(&window);
            }
        });
    }
    menu.append(&shortcuts);

    let about = menu_item_button("help-about-symbolic", "About gTunes");
    {
        let popover = popover.clone();
        let settings = settings.clone();
        about.connect_clicked(move |_| {
            popover.popdown();
            if let Some(window) = settings
                .root()
                .and_then(|root| root.downcast::<gtk::Window>().ok())
            {
                show_about_window(&window);
            }
        });
    }
    menu.append(&about);

    let reset = menu_item_button("edit-delete-symbolic", "Reset database and cache");
    {
        let state = state.clone();
        let popover = popover.clone();
        let settings = settings.clone();
        reset.connect_clicked(move |_| {
            popover.popdown();
            if let Some(window) = settings
                .root()
                .and_then(|root| root.downcast::<gtk::Window>().ok())
            {
                confirm_database_reset(&window, state.clone());
            }
        });
    }
    menu.append(&reset);
    menu.append(&gtk::Separator::new(Orientation::Horizontal));

    let quit = menu_item_button("application-exit-symbolic", "Quit");
    quit.add_css_class("destructive-action");
    {
        let state = state.clone();
        let settings = settings.clone();
        quit.connect_clicked(move |_| {
            if let Some(window) = settings
                .root()
                .and_then(|root| root.downcast::<gtk::Window>().ok())
            {
                quit_application(&window, &state);
            }
        });
    }
    menu.append(&quit);

    popover.set_child(Some(&menu));
    settings.set_popover(Some(&popover));
    settings
}

fn menu_item_button(icon_name: &str, title: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("flat");
    button.add_css_class("settings-menu-item");
    button.set_halign(Align::Fill);

    let row = gtk::Box::new(Orientation::Horizontal, 12);
    row.set_margin_top(7);
    row.set_margin_bottom(7);
    row.set_margin_start(8);
    row.set_margin_end(8);
    row.set_halign(Align::Fill);
    row.append(&gtk::Image::from_icon_name(icon_name));
    let title = label(title, "settings-menu-label");
    title.set_hexpand(true);
    title.set_halign(Align::Start);
    row.append(&title);
    button.set_child(Some(&row));
    button
}

#[allow(deprecated)]
fn show_keyboard_shortcuts(parent: &gtk::Window) {
    let shortcuts = gtk::Window::builder()
        .transient_for(parent)
        .modal(true)
        .title("Keyboard Shortcuts")
        .default_width(420)
        .resizable(false)
        .build();

    let content = gtk::Box::new(Orientation::Vertical, 0);
    content.add_css_class("shortcuts-dialog");
    content.set_margin_top(18);
    content.set_margin_bottom(18);
    content.set_margin_start(18);
    content.set_margin_end(18);

    let header = gtk::Box::new(Orientation::Horizontal, 12);
    header.add_css_class("shortcuts-header");
    header.set_halign(Align::Fill);

    let group_title = label("Library", "shortcuts-group-title");
    group_title.set_hexpand(true);
    group_title.set_halign(Align::Start);
    header.append(&group_title);

    let close = icon_button("window-close-symbolic", "Close keyboard shortcuts");
    close.add_css_class("toolbar-button");
    {
        let shortcuts = shortcuts.clone();
        close.connect_clicked(move |_| {
            shortcuts.close();
        });
    }
    header.append(&close);
    content.append(&header);

    let list = gtk::ListBox::new();
    list.add_css_class("boxed-list");
    list.set_selection_mode(gtk::SelectionMode::None);
    for (title, accelerator) in [
        ("Search library", "<Control>F"),
        ("Tracks", "<Control>1"),
        ("Albums", "<Control>2"),
        ("Artists", "<Control>3"),
        ("Playlists", "<Control>4"),
        ("Radio", "<Control>5"),
        ("Toggle shuffle", "<Control>S"),
        ("Play selected search result", "Return"),
    ] {
        list.append(&shortcut_row(title, accelerator));
    }
    content.append(&list);

    shortcuts.set_child(Some(&content));
    shortcuts.present();
}

#[allow(deprecated)]
fn shortcut_row(title: &str, accelerator: &str) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_activatable(false);
    row.set_selectable(false);

    let content = gtk::Box::new(Orientation::Horizontal, 12);
    content.set_margin_top(10);
    content.set_margin_bottom(10);
    content.set_margin_start(12);
    content.set_margin_end(12);

    let title = label(title, "");
    title.set_hexpand(true);
    title.set_halign(Align::Start);
    content.append(&title);

    let shortcut = gtk::ShortcutLabel::new(accelerator);
    shortcut.set_halign(Align::End);
    content.append(&shortcut);

    row.set_child(Some(&content));
    row
}

fn show_about_window(parent: &gtk::Window) {
    let about = gtk::AboutDialog::builder()
        .transient_for(parent)
        .modal(true)
        .logo_icon_name(config::APP_ID)
        .program_name(config::APP_NAME)
        .authors([config::DEVELOPER_NAME])
        .version(config::VERSION)
        .comments("A GTK4/Libadwaita Jellyfin music streaming client for Linux.")
        .license_type(gtk::License::MitX11)
        .website("https://github.com/fivves/gtunes")
        .website_label("GitHub")
        .build();
    about.present();
}

fn quit_application(parent: &gtk::Window, state: &Rc<RefCell<UiState>>) {
    let mut ui = state.borrow_mut();
    save_playback_snapshot_now(&mut ui);
    stop_playback(&mut ui);
    drop(ui);
    if let Some(app) = parent.application() {
        app.quit();
    } else {
        parent.close();
    }
}

fn connect_player_bar_responsive_layout(
    player: &gtk::Box,
    actions: &gtk::Overlay,
    playback_status: &gtk::Label,
) {
    let actions = actions.clone();
    let playback_status = playback_status.clone();
    player.add_tick_callback(move |player, _| {
        let width = player.allocated_width();
        if width > 0 {
            actions.set_visible(width >= 560);
            playback_status.set_visible(width >= 720);
        }
        gtk::glib::ControlFlow::Continue
    });
}

#[allow(deprecated)]
fn confirm_database_reset(parent: &gtk::Window, state: Rc<RefCell<UiState>>) {
    let dialog = gtk::MessageDialog::builder()
        .transient_for(parent)
        .modal(true)
        .message_type(gtk::MessageType::Warning)
        .buttons(gtk::ButtonsType::None)
        .text("Reset database and cache?")
        .secondary_text(
            "This clears saved Jellyfin login, cached library data, artwork, and waveforms. gTunes will return to first-time setup.",
        )
        .build();
    dialog.add_button("Cancel", gtk::ResponseType::Cancel);
    dialog.add_button("Reset", gtk::ResponseType::Accept);
    dialog.set_default_response(gtk::ResponseType::Cancel);
    if let Some(button) = dialog
        .widget_for_response(gtk::ResponseType::Accept)
        .and_then(|widget| widget.downcast::<gtk::Button>().ok())
    {
        button.add_css_class("destructive-action");
    }

    dialog.connect_response(move |dialog, response| {
        if response == gtk::ResponseType::Accept {
            reset_database_and_cache(state.clone());
        }
        dialog.close();
    });
    dialog.present();
}

fn reset_database_and_cache(state: Rc<RefCell<UiState>>) {
    {
        let mut ui = state.borrow_mut();
        ui.connection_generation = CONNECTION_GENERATION
            .fetch_add(1, AtomicOrdering::SeqCst)
            .wrapping_add(1);
        stop_playback(&mut ui);
        ui.connection_status.set_text("Resetting database");
        ui.connection_detail
            .set_text("Clearing saved login, library cache, artwork, and waveforms");
        ui.page_summary.set_text("Resetting database and cache");
        ui.playback_status.set_text("Jellyfin stream | Not playing");
        if let Some(status) = ui.connection_form_status.as_ref() {
            status.set_text("Resetting...");
        }
        if let Some(spinner) = ui.sync_spinner.as_ref() {
            spinner.set_visible(true);
            spinner.start();
        }
    }

    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = CACHE_RESET_LOCK
            .lock()
            .map_err(|_| "cache reset lock is poisoned".to_string())
            .and_then(|_guard| {
                CacheDatabase::reset_default_cache().map_err(|error| error.to_string())
            });
        let _ = sender.send(result);
    });

    gtk::glib::timeout_add_local(Duration::from_millis(100), move || {
        match receiver.try_recv() {
            Ok(Ok(())) => {
                apply_first_time_setup_state(&state);
                gtk::glib::ControlFlow::Break
            }
            Ok(Err(error)) => {
                set_library_loaded(&state);
                let ui = state.borrow();
                ui.connection_status.set_text("Reset failed");
                ui.connection_detail.set_text(&error);
                ui.page_summary.set_text("Database reset failed");
                if let Some(status) = ui.connection_form_status.as_ref() {
                    status.set_text("Reset failed");
                }
                gtk::glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => {
                set_library_loaded(&state);
                let ui = state.borrow();
                ui.connection_status.set_text("Reset failed");
                ui.connection_detail
                    .set_text("Database reset worker stopped unexpectedly");
                ui.page_summary.set_text("Database reset failed");
                if let Some(status) = ui.connection_form_status.as_ref() {
                    status.set_text("Reset failed");
                }
                gtk::glib::ControlFlow::Break
            }
        }
    });
}

fn apply_first_time_setup_state(state: &Rc<RefCell<UiState>>) {
    let (
        search_entry,
        server_entry,
        username_entry,
        password_entry,
        form_status,
        connection_card,
        refresh_button,
        cover,
        wave_area,
        spinner,
    ) = {
        let ui = state.borrow();
        (
            ui.search_entry.clone(),
            ui.connection_server_entry.clone(),
            ui.connection_username_entry.clone(),
            ui.connection_password_entry.clone(),
            ui.connection_form_status.clone(),
            ui.connection_card.clone(),
            ui.refresh_button.clone(),
            ui.cover_art.clone(),
            ui.wave_area.clone(),
            ui.sync_spinner.clone(),
        )
    };

    {
        let mut ui = state.borrow_mut();
        ui.all_tracks.clear();
        ui.tracks.clear();
        ui.playlists.clear();
        ui.track_filter_signature = TrackFilterSignature {
            album_filter: None,
            artist_filter: None,
            playlist_filter: None,
            search_query: String::new(),
            sort_column: LibraryViewSettings::default().sort_column,
            sort_ascending: LibraryViewSettings::default().sort_ascending,
        };
        ui.library_albums.clear();
        ui.library_artists.clear();
        ui.collection_render_generation = ui.collection_render_generation.wrapping_add(1);
        ui.active_page = LibraryPage::Tracks;
        ui.album_filter = None;
        ui.artist_filter = None;
        ui.playlist_filter = None;
        ui.collection_detail_title = None;
        ui.collection_detail_subtitle = None;
        ui.selected_index = 0;
        ui.search_query.clear();
        ui.jellyfin_connected = false;
        ui.sort_column = LibraryViewSettings::default().sort_column;
        ui.sort_ascending = LibraryViewSettings::default().sort_ascending;
        ui.playback_session.reset_to_empty_library();
        ui.track_indicators.clear();
        {
            let mut waveform = ui.waveform.borrow_mut();
            waveform.peaks.clear();
            waveform.progress = 0.0;
            waveform.loaded_key = None;
            waveform.loading_key = None;
        }

        ui.now_title.set_text("No track selected");
        ui.now_meta.set_text("Connect to Jellyfin to load music");
        ui.playback_status.set_text("Jellyfin stream | Not playing");
        ui.page_summary
            .set_text("Jellyfin music library | Not connected");
        ui.connection_status.set_text("Not connected");
        ui.connection_detail
            .set_text("Connect to Jellyfin to sync tracks");
        ui.elapsed_label.set_text("0:00");
        ui.remaining_label.set_text("--:--");
        ui.waveform_status.set_text("Select a Jellyfin track");
        update_play_button(&ui);
        update_shuffle_button(&ui);
        sync_external_playback_status(&mut ui);
    }

    if let Some(entry) = search_entry.as_ref() {
        entry.set_text("");
    }
    if let Some(entry) = server_entry.as_ref() {
        entry.set_text("");
    }
    if let Some(entry) = username_entry.as_ref() {
        entry.set_text("");
    }
    if let Some(entry) = password_entry.as_ref() {
        entry.set_text("");
    }
    if let Some(status) = form_status.as_ref() {
        status.set_text("Ready");
    }
    if let Some(card) = connection_card.as_ref() {
        card.set_visible(true);
    }
    if let Some(button) = refresh_button.as_ref() {
        button.set_sensitive(false);
    }
    set_reconnect_button_needed(state, false);
    if let Some(cover) = cover.as_ref() {
        cover.set_paintable(Option::<&gtk::gdk::Paintable>::None);
        cover.set_icon_name(Some("audio-x-generic-symbolic"));
    }
    if let Some(area) = wave_area.as_ref() {
        area.queue_draw();
    }
    if let Some(spinner) = spinner.as_ref() {
        spinner.stop();
        spinner.set_visible(false);
    }

    refresh_track_model(state);
    refresh_collection_grids(state);
    update_content_view(state, NavDirection::DrillForward);
}

fn build_body(state: Rc<RefCell<UiState>>) -> gtk::Box {
    let outer = gtk::Box::new(Orientation::Horizontal, 0);
    outer.add_css_class("main-paned");
    outer.set_hexpand(true);
    outer.set_vexpand(true);

    let (sidebar, _sidebar_queue, _sidebar_cover) = build_sidebar(state.clone());
    sidebar.set_size_request(LEFT_SIDEBAR_WIDTH, -1);
    sidebar.set_hexpand(false);
    outer.append(&sidebar);

    let content = build_content(state.clone());
    outer.append(&content);
    outer
}

fn build_sidebar(state: Rc<RefCell<UiState>>) -> (gtk::Box, gtk::Box, gtk::Box) {
    let sidebar = gtk::Box::new(Orientation::Vertical, 4);
    sidebar.add_css_class("sidebar");

    sidebar.append(&label("Library", "section-title"));
    sidebar.append(&nav_list(state.clone()));

    let spacer = gtk::Box::new(Orientation::Vertical, 0);
    spacer.set_vexpand(true);
    sidebar.append(&spacer);

    let queue = queue_card(state.clone());
    let cover = sidebar_cover_art(state.clone());
    state.borrow_mut().sidebar_queue_card = Some(queue.clone());
    sidebar.append(&queue);
    sidebar.append(&cover);

    (sidebar, queue, cover)
}

fn sidebar_cover_art(state: Rc<RefCell<UiState>>) -> gtk::Box {
    let frame = gtk::Box::new(Orientation::Vertical, 0);
    frame.add_css_class("sidebar-cover-frame");
    frame.set_size_request(LEFT_SIDEBAR_CONTENT_WIDTH, LEFT_SIDEBAR_CONTENT_WIDTH);
    frame.set_halign(Align::Center);
    frame.set_valign(Align::End);
    frame.set_overflow(gtk::Overflow::Hidden);

    let cover = cover_art(LEFT_SIDEBAR_CONTENT_WIDTH);
    cover.add_css_class("sidebar-cover");
    cover.set_halign(Align::Fill);
    cover.set_valign(Align::Fill);
    state.borrow_mut().cover_art = Some(cover.clone());
    frame.append(&cover);

    let click = gtk::GestureClick::new();
    click.connect_pressed(move |_, _, _, _| {
        show_full_size_artwork(&state);
    });
    frame.add_controller(click);
    frame.set_cursor_from_name(Some("pointer"));

    frame
}

fn build_content(state: Rc<RefCell<UiState>>) -> gtk::Box {
    let content = gtk::Box::new(Orientation::Vertical, 0);
    content.add_css_class("content");
    content.set_hexpand(true);
    content.set_vexpand(true);

    content.append(&connection_card(state.clone()));
    content.append(&detail_header(state.clone()));

    let stack = gtk::Stack::new();
    stack.set_hexpand(true);
    stack.set_vexpand(true);
    stack.set_transition_duration(200);
    stack.add_named(&track_table(state.clone()), Some("tracks"));
    stack.add_named(&album_grid_page(state.clone()), Some("albums"));
    stack.add_named(&artist_grid_page(state.clone()), Some("artists"));
    stack.add_named(&playlist_grid_page(state.clone()), Some("playlists"));
    stack.add_named(&radio_page(state.clone()), Some("radio"));
    stack.add_named(&next_up_page(state.clone()), Some("next-up"));
    stack.set_visible_child_name("tracks");
    state.borrow_mut().library_stack = Some(stack.clone());

    content.append(&stack);
    refresh_collection_grids(&state);
    update_content_view(&state, NavDirection::DrillForward);
    content
}

fn detail_header(state: Rc<RefCell<UiState>>) -> gtk::Box {
    let detail_header = gtk::Box::new(Orientation::Horizontal, 10);
    detail_header.add_css_class("detail-header");
    detail_header.set_visible(false);

    let back = icon_button("go-previous-symbolic", "Back");
    back.add_css_class("toolbar-button");
    back.set_valign(Align::Center);
    {
        let state = state.clone();
        back.connect_clicked(move |_| {
            return_to_collection_grid(&state);
        });
    }
    detail_header.append(&back);

    let detail_text = gtk::Box::new(Orientation::Vertical, 2);
    detail_text.set_halign(Align::Fill);
    detail_text.set_valign(Align::Center);
    detail_text.set_hexpand(true);
    let detail_title = label("", "page-title");
    detail_title.set_single_line_mode(true);
    detail_title.set_lines(1);
    let detail_subtitle = label("", "meta");
    detail_subtitle.set_single_line_mode(true);
    detail_subtitle.set_lines(1);
    detail_text.append(&detail_title);
    detail_text.append(&detail_subtitle);
    detail_header.append(&detail_text);

    {
        let mut ui = state.borrow_mut();
        ui.detail_header = Some(detail_header.clone());
        ui.detail_title_label = Some(detail_title);
        ui.detail_subtitle_label = Some(detail_subtitle);
    }

    detail_header
}

fn connection_card(state: Rc<RefCell<UiState>>) -> gtk::Box {
    let card = gtk::Box::new(Orientation::Vertical, 8);
    card.add_css_class("connection-card");
    state.borrow_mut().connection_card = Some(card.clone());

    let header = gtk::Box::new(Orientation::Horizontal, 8);
    let title = label("Jellyfin Connection", "rail-title");
    header.append(&title);
    let spacer = gtk::Box::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    header.append(&spacer);
    let status = label("Ready", "meta");
    header.append(&status);
    card.append(&header);

    let server_row = gtk::Box::new(Orientation::Horizontal, 8);
    let server = gtk::Entry::new();
    server.set_placeholder_text(Some("Server URL"));
    server.set_hexpand(true);
    server_row.append(&server);
    card.append(&server_row);

    let form = gtk::Box::new(Orientation::Horizontal, 8);
    let username = gtk::Entry::new();
    username.set_placeholder_text(Some("Username"));
    username.set_hexpand(true);
    username.set_width_chars(14);
    let password = gtk::PasswordEntry::new();
    password.set_placeholder_text(Some("Password"));
    password.set_hexpand(true);
    password.set_width_chars(14);
    let connect = gtk::Button::with_label("Connect");
    connect.add_css_class("connection-button");
    connect.add_css_class("suggested-action");

    {
        let mut ui = state.borrow_mut();
        ui.connection_form_status = Some(status.clone());
        ui.connection_server_entry = Some(server.clone());
        ui.connection_username_entry = Some(username.clone());
        ui.connection_password_entry = Some(password.clone());
    }

    match CacheDatabase::open_default().and_then(|db| db.load_jellyfin_session()) {
        Ok(Some(session)) => {
            let generation = CONNECTION_GENERATION.load(AtomicOrdering::SeqCst);
            server.set_text(&session.server_url);
            username.set_text(&session.username);
            status.set_text("Loading saved library...");
            set_library_loading(&state, "Loading cached Jellyfin library");
            card.set_visible(false);

            let (sender, receiver) = mpsc::channel();
            std::thread::spawn(move || {
                fetch_saved_session(session, sender, generation);
            });
            poll_connection_result(receiver, state.clone(), status.clone(), None, generation);
        }
        Ok(None) => {}
        Err(error) => {
            let message = error.to_string();
            status.set_text("Cache unavailable");
            state
                .borrow()
                .connection_status
                .set_text("Cache unavailable");
            state.borrow().connection_detail.set_text(&message);
            state.borrow().page_summary.set_text(&message);
        }
    }

    form.append(&username);
    form.append(&password);
    form.append(&connect);
    card.append(&form);

    connect.connect_clicked(move |button| {
        let server_url = server.text().trim().to_string();
        let username_text = username.text().trim().to_string();
        let password_text = password.text().to_string();

        if server_url.is_empty() || username_text.is_empty() || password_text.is_empty() {
            status.set_text("Server, username, and password are required");
            return;
        }

        button.set_sensitive(false);
        status.set_text("Connecting...");
        set_library_loading(&state, "Authenticating with Jellyfin");
        let generation = CONNECTION_GENERATION.load(AtomicOrdering::SeqCst);

        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            connect_and_fetch(
                &server_url,
                &username_text,
                &password_text,
                sender,
                generation,
            );
        });

        poll_connection_result(
            receiver,
            state.clone(),
            status.clone(),
            Some(button.clone()),
            generation,
        );
    });

    card
}

fn poll_connection_result(
    receiver: mpsc::Receiver<ConnectionMessage>,
    state: Rc<RefCell<UiState>>,
    status: gtk::Label,
    button: Option<gtk::Button>,
    generation: u64,
) {
    gtk::glib::timeout_add_local(Duration::from_millis(100), move || {
        if state.borrow().connection_generation != generation {
            if let Some(button) = button.as_ref() {
                button.set_sensitive(true);
            }
            set_refresh_button_connected_state(&state);
            set_library_loaded(&state);
            return gtk::glib::ControlFlow::Break;
        }

        match receiver.try_recv() {
            Ok(ConnectionMessage::Authenticated(_)) => gtk::glib::ControlFlow::Continue,
            Ok(ConnectionMessage::Status(message)) => {
                status.set_text(&message);
                let ui = state.borrow();
                ui.connection_status.set_text("Refreshing library");
                ui.connection_detail.set_text(&message);
                gtk::glib::ControlFlow::Continue
            }
            Ok(ConnectionMessage::Progress { loaded, total }) => {
                let progress = library_progress_text(loaded, total);
                status.set_text(&progress);
                let ui = state.borrow();
                ui.connection_status.set_text("Loading library");
                ui.connection_detail.set_text(&progress);
                gtk::glib::ControlFlow::Continue
            }
            Ok(ConnectionMessage::Finished(Ok(payload))) => {
                if let Some(button) = button.as_ref() {
                    button.set_sensitive(true);
                }
                status.set_text("Connected");
                set_library_loaded(&state);
                apply_connection_payload(&state, payload);
                set_reconnect_button_needed(&state, false);
                if let Some(card) = state.borrow().connection_card.as_ref() {
                    card.set_visible(false);
                }
                gtk::glib::ControlFlow::Break
            }
            Ok(ConnectionMessage::Finished(Err(error))) => {
                if let Some(button) = button.as_ref() {
                    button.set_sensitive(true);
                }
                set_refresh_button_connected_state(&state);
                status.set_text("Connection failed");
                set_library_loaded(&state);
                state
                    .borrow()
                    .connection_status
                    .set_text("Connection failed");
                state.borrow().connection_detail.set_text(&error);
                set_reconnect_button_error_state(&state, &error);
                gtk::glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => {
                if let Some(button) = button.as_ref() {
                    button.set_sensitive(true);
                }
                set_refresh_button_connected_state(&state);
                status.set_text("Connection worker stopped");
                set_library_loaded(&state);
                gtk::glib::ControlFlow::Break
            }
        }
    });
}

fn refresh_jellyfin_library(state: Rc<RefCell<UiState>>, button: gtk::Button) {
    let generation = state.borrow().connection_generation;
    button.set_sensitive(false);
    set_library_loading(&state, "Refreshing Jellyfin library");

    let status = {
        let ui = state.borrow();
        ui.connection_status.clone()
    };
    status.set_text("Refreshing library");

    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        refresh_saved_session(sender, generation);
    });

    poll_connection_result(receiver, state, status, Some(button), generation);
}

fn set_refresh_button_connected_state(state: &Rc<RefCell<UiState>>) {
    let (button, connected) = {
        let ui = state.borrow();
        (ui.refresh_button.clone(), ui.jellyfin_connected)
    };
    if let Some(button) = button.as_ref() {
        button.set_sensitive(connected);
    }
}

fn set_reconnect_button_needed(state: &Rc<RefCell<UiState>>, needed: bool) {
    if let Some(button) = state.borrow().reconnect_button.as_ref() {
        button.set_visible(needed);
        button.set_sensitive(needed);
    }
}

fn set_reconnect_button_error_state(state: &Rc<RefCell<UiState>>, error: &str) {
    if !error_needs_reconnect(error) {
        set_reconnect_button_needed(state, false);
        return;
    }

    let has_session = CacheDatabase::open_default()
        .and_then(|db| db.load_jellyfin_session())
        .map(|session| session.is_some())
        .unwrap_or(false);
    set_reconnect_button_needed(state, has_session);
}

fn error_needs_reconnect(error: &str) -> bool {
    error.contains("reconnect")
}

fn set_library_loading(state: &Rc<RefCell<UiState>>, message: &str) {
    let ui = state.borrow();
    ui.connection_status.set_text("Loading library");
    ui.connection_detail.set_text(message);
    ui.page_summary.set_text(message);
    if let Some(spinner) = ui.sync_spinner.as_ref() {
        spinner.set_visible(true);
        spinner.start();
    }
}

fn set_library_loaded(state: &Rc<RefCell<UiState>>) {
    let ui = state.borrow();
    if let Some(spinner) = ui.sync_spinner.as_ref() {
        spinner.stop();
        spinner.set_visible(false);
    }
}

fn library_progress_text(loaded: usize, total: Option<usize>) -> String {
    match total {
        Some(total) if total > 0 => format!("Loading full library: {loaded} of {total} tracks"),
        _ => format!("Loading full library: {loaded} tracks"),
    }
}

#[derive(Clone, Debug)]
struct AlbumSummary {
    key: String,
    name: String,
    artist: String,
    artist_image_url: Option<String>,
    artwork_url: Option<String>,
    song_count: usize,
}

#[derive(Clone, Debug)]
struct ArtistSummary {
    key: String,
    name: String,
    image_url: Option<String>,
    album_count: usize,
    song_count: usize,
}

struct AlbumAccumulator {
    key: String,
    name: String,
    artwork_url: Option<String>,
    song_count: usize,
    explicit_artist_votes: Vec<ArtistVote>,
    fallback_artist_votes: Vec<ArtistVote>,
}

struct ArtistVote {
    key: String,
    name: String,
    image_url: Option<String>,
    count: usize,
    first_seen: usize,
}

struct ArtistNameVote {
    key: String,
    name: String,
    count: usize,
    first_seen: usize,
}

fn album_grid_page(state: Rc<RefCell<UiState>>) -> gtk::ScrolledWindow {
    let scroll = gtk::ScrolledWindow::new();
    scroll.add_css_class("collection-scroll");
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_overlay_scrolling(true);
    scroll.set_hexpand(true);
    scroll.set_vexpand(true);

    let flow = gtk::FlowBox::new();
    flow.add_css_class("collection-grid");
    flow.add_css_class("album-grid");
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow.set_min_children_per_line(1);
    flow.set_max_children_per_line(8);
    flow.set_row_spacing(18);
    flow.set_column_spacing(18);
    flow.set_homogeneous(false);
    flow.set_valign(Align::Start);
    scroll.set_child(Some(&flow));

    state.borrow_mut().album_grid = Some(flow);
    scroll
}

fn artist_grid_page(state: Rc<RefCell<UiState>>) -> gtk::ScrolledWindow {
    let scroll = gtk::ScrolledWindow::new();
    scroll.add_css_class("collection-scroll");
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_overlay_scrolling(true);
    scroll.set_hexpand(true);
    scroll.set_vexpand(true);

    let flow = gtk::FlowBox::new();
    flow.add_css_class("collection-grid");
    flow.add_css_class("artist-grid");
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow.set_min_children_per_line(1);
    flow.set_max_children_per_line(8);
    flow.set_row_spacing(18);
    flow.set_column_spacing(18);
    flow.set_homogeneous(true);
    flow.set_valign(Align::Start);
    scroll.set_child(Some(&flow));

    state.borrow_mut().artist_grid = Some(flow);
    scroll
}

fn playlist_grid_page(state: Rc<RefCell<UiState>>) -> gtk::ScrolledWindow {
    let scroll = gtk::ScrolledWindow::new();
    scroll.add_css_class("collection-scroll");
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_overlay_scrolling(true);
    scroll.set_hexpand(true);
    scroll.set_vexpand(true);

    let flow = gtk::FlowBox::new();
    flow.add_css_class("collection-grid");
    flow.add_css_class("playlist-grid");
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow.set_min_children_per_line(1);
    flow.set_max_children_per_line(8);
    flow.set_row_spacing(18);
    flow.set_column_spacing(18);
    flow.set_homogeneous(false);
    flow.set_valign(Align::Start);
    scroll.set_child(Some(&flow));

    state.borrow_mut().playlist_grid = Some(flow);
    scroll
}

fn radio_page(state: Rc<RefCell<UiState>>) -> gtk::ScrolledWindow {
    let scroll = gtk::ScrolledWindow::new();
    scroll.add_css_class("collection-scroll");
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_overlay_scrolling(true);
    scroll.set_hexpand(true);
    scroll.set_vexpand(true);

    let overlay = gtk::Overlay::new();
    overlay.set_hexpand(true);
    overlay.set_vexpand(true);
    scroll.set_child(Some(&overlay));

    let page = gtk::Box::new(Orientation::Vertical, 14);
    page.add_css_class("radio-page");
    overlay.set_child(Some(&page));

    let header = gtk::Box::new(Orientation::Horizontal, 10);
    header.add_css_class("radio-header");
    header.set_hexpand(true);
    header.set_valign(Align::Center);
    let header_icon = radio_icon(34);
    header_icon.set_valign(Align::Center);
    header.append(&header_icon);

    let header_text = gtk::Box::new(Orientation::Vertical, 1);
    header_text.set_hexpand(true);
    header_text.set_valign(Align::Center);
    header_text.append(&label("Internet Radio", "page-title"));
    let station_count = radio_stations_for_display(&state).len();
    header_text.append(&label(&format!("{station_count} stations"), "meta"));
    header.append(&header_text);
    page.append(&header);

    let station_area = gtk::Box::new(Orientation::Vertical, 8);
    station_area.set_hexpand(true);
    station_area.set_vexpand(true);
    let grid = gtk::FlowBox::new();
    grid.add_css_class("radio-grid");
    grid.set_row_spacing(RADIO_GRID_COLUMN_GAP as u32);
    grid.set_column_spacing(RADIO_GRID_COLUMN_GAP as u32);
    grid.set_selection_mode(gtk::SelectionMode::None);
    grid.set_min_children_per_line(1);
    grid.set_max_children_per_line(6);
    grid.set_homogeneous(false);
    grid.set_halign(Align::Center);
    grid.set_valign(Align::Start);
    station_area.append(&grid);
    page.append(&station_area);

    let radio_state = state.clone();
    let (add_popover, name_entry, url_entry, icon_entry) = radio_station_form_popover(
        "Add Station",
        "Add Station",
        "",
        "",
        "",
        move |name, url, icon| persist_custom_radio_station(&radio_state, &name, &url, &icon),
    );

    let add_menu = gtk::MenuButton::new();
    add_menu.add_css_class("radio-add-fab");
    add_menu.set_icon_name("list-add-symbolic");
    add_menu.set_tooltip_text(Some("Add station"));
    add_menu.set_popover(Some(&add_popover));
    add_menu.set_halign(Align::End);
    add_menu.set_valign(Align::End);
    add_menu.set_margin_bottom(18);
    add_menu.set_margin_end(18);
    overlay.add_overlay(&add_menu);

    {
        let mut ui = state.borrow_mut();
        ui.radio_name_entry = Some(name_entry);
        ui.radio_url_entry = Some(url_entry);
        ui.radio_icon_entry = Some(icon_entry);
        ui.radio_grid = Some(grid);
    }
    refresh_radio_page(&state);
    scroll
}

fn next_up_page(state: Rc<RefCell<UiState>>) -> gtk::ScrolledWindow {
    let scroll = gtk::ScrolledWindow::new();
    scroll.add_css_class("collection-scroll");
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_overlay_scrolling(true);
    scroll.set_hexpand(true);
    scroll.set_vexpand(true);

    let page = gtk::Box::new(Orientation::Vertical, 0);
    page.add_css_class("next-up-page");
    page.set_margin_top(18);
    page.set_margin_bottom(18);
    page.set_margin_start(18);
    page.set_margin_end(18);
    scroll.set_child(Some(&page));

    let empty = collection_empty_state("Nothing queued yet");
    empty.add_css_class("next-up-empty");
    page.append(&empty);

    let list = gtk::Box::new(Orientation::Vertical, 8);
    list.add_css_class("next-up-list");
    list.set_hexpand(true);

    let list_drop = gtk::DropTarget::new(gtk::glib::Type::U32, gtk::gdk::DragAction::MOVE);
    {
        let state = state.clone();
        list_drop.connect_drop(move |_, value, _, _| {
            let Ok(from) = value.get::<u32>() else {
                return false;
            };
            move_next_up_track(&state, from as usize, NEXT_UP_PAGE_LIMIT)
        });
    }
    list.add_controller(list_drop);

    page.append(&list);

    state.borrow_mut().next_up_view = Some(Rc::new(NextUpPageView {
        empty,
        list,
        rows: Rc::new(RefCell::new(Vec::new())),
    }));
    rebuild_queue_list(&state);
    scroll
}

fn refresh_collection_grids(state: &Rc<RefCell<UiState>>) {
    refresh_album_grid(state);
    refresh_artist_grid(state);
    refresh_playlist_grid(state);
    update_nav_counts(state);
}

fn refresh_visible_collection_grid(state: &Rc<RefCell<UiState>>) {
    let active_view = {
        let ui = state.borrow();
        (
            ui.active_page,
            ui.album_filter.clone(),
            ui.artist_filter.clone(),
            ui.playlist_filter.clone(),
        )
    };

    match active_view {
        (LibraryPage::Albums, None, _, _) => refresh_album_grid(state),
        (LibraryPage::Artists, _, None, _) => refresh_artist_grid(state),
        (LibraryPage::Artists, None, Some(_), _) => refresh_album_grid(state),
        (LibraryPage::Playlists, _, _, None) => refresh_playlist_grid(state),
        (LibraryPage::NextUp, _, _, _) => {}
        _ => {}
    }
}

fn refresh_album_grid(state: &Rc<RefCell<UiState>>) {
    let (grid, albums) = {
        let ui = state.borrow();
        let albums = if ui.active_page == LibraryPage::Artists {
            ui.artist_filter
                .as_deref()
                .map(|selected_artist_key| {
                    album_summaries_for_artist_from(
                        &ui.library_albums,
                        selected_artist_key,
                        &ui.search_query,
                    )
                })
                .unwrap_or_else(|| filter_album_summaries(&ui.library_albums, &ui.search_query))
        } else {
            filter_album_summaries(&ui.library_albums, &ui.search_query)
        };
        (ui.album_grid.clone(), albums)
    };
    let Some(grid) = grid else {
        return;
    };
    next_collection_render_generation(state);
    clear_flow_box(&grid);

    if albums.is_empty() {
        grid.insert(&collection_empty_state("No albums found"), -1);
        return;
    }

    render_album_tiles_batched(state, grid, albums);
}

fn refresh_artist_grid(state: &Rc<RefCell<UiState>>) {
    let (grid, artists) = {
        let ui = state.borrow();
        (
            ui.artist_grid.clone(),
            filter_artist_summaries(&ui.library_artists, &ui.search_query),
        )
    };
    let Some(grid) = grid else {
        return;
    };
    next_collection_render_generation(state);
    clear_flow_box(&grid);

    if artists.is_empty() {
        grid.insert(&collection_empty_state("No artists found"), -1);
        return;
    }

    render_artist_tiles_batched(state, grid, artists);
}

fn refresh_playlist_grid(state: &Rc<RefCell<UiState>>) {
    let (grid, playlists) = {
        let ui = state.borrow();
        (
            ui.playlist_grid.clone(),
            filter_playlists(&ui.playlists, &ui.search_query),
        )
    };
    let Some(grid) = grid else {
        return;
    };
    next_collection_render_generation(state);
    clear_flow_box(&grid);

    if playlists.is_empty() {
        grid.insert(&collection_empty_state("No playlists found"), -1);
        return;
    }

    render_playlist_tiles_batched(state, grid, playlists);
}

fn clear_flow_box(flow: &gtk::FlowBox) {
    while let Some(child) = flow.first_child() {
        flow.remove(&child);
    }
}

fn next_collection_render_generation(state: &Rc<RefCell<UiState>>) -> u64 {
    let mut ui = state.borrow_mut();
    ui.collection_render_generation = ui.collection_render_generation.wrapping_add(1);
    ui.collection_render_generation
}

fn collection_render_generation(state: &Rc<RefCell<UiState>>) -> u64 {
    state.borrow().collection_render_generation
}

fn render_album_tiles_batched(
    state: &Rc<RefCell<UiState>>,
    grid: gtk::FlowBox,
    mut albums: Vec<AlbumSummary>,
) {
    let generation = next_collection_render_generation(state);
    let initial_count = albums.len().min(COLLECTION_TILE_INITIAL_BATCH);
    for (index, album) in albums.drain(..initial_count).enumerate() {
        grid.insert(&album_tile(album, state.clone(), index), -1);
    }

    if albums.is_empty() {
        return;
    }

    let state = state.clone();
    let mut next_index = initial_count;
    gtk::glib::idle_add_local(move || {
        if collection_render_generation(&state) != generation {
            return gtk::glib::ControlFlow::Break;
        }

        let batch_count = albums.len().min(COLLECTION_TILE_IDLE_BATCH);
        for album in albums.drain(..batch_count) {
            grid.insert(&album_tile(album, state.clone(), next_index), -1);
            next_index += 1;
        }

        if albums.is_empty() {
            gtk::glib::ControlFlow::Break
        } else {
            gtk::glib::ControlFlow::Continue
        }
    });
}

fn render_artist_tiles_batched(
    state: &Rc<RefCell<UiState>>,
    grid: gtk::FlowBox,
    mut artists: Vec<ArtistSummary>,
) {
    let generation = next_collection_render_generation(state);
    let initial_count = artists.len().min(COLLECTION_TILE_INITIAL_BATCH);
    for (index, artist) in artists.drain(..initial_count).enumerate() {
        grid.insert(&artist_tile(artist, state.clone(), index), -1);
    }

    if artists.is_empty() {
        return;
    }

    let state = state.clone();
    let mut next_index = initial_count;
    gtk::glib::idle_add_local(move || {
        if collection_render_generation(&state) != generation {
            return gtk::glib::ControlFlow::Break;
        }

        let batch_count = artists.len().min(COLLECTION_TILE_IDLE_BATCH);
        for artist in artists.drain(..batch_count) {
            grid.insert(&artist_tile(artist, state.clone(), next_index), -1);
            next_index += 1;
        }

        if artists.is_empty() {
            gtk::glib::ControlFlow::Break
        } else {
            gtk::glib::ControlFlow::Continue
        }
    });
}

fn render_playlist_tiles_batched(
    state: &Rc<RefCell<UiState>>,
    grid: gtk::FlowBox,
    mut playlists: Vec<UiPlaylist>,
) {
    let generation = next_collection_render_generation(state);
    let initial_count = playlists.len().min(COLLECTION_TILE_INITIAL_BATCH);
    for (index, playlist) in playlists.drain(..initial_count).enumerate() {
        grid.insert(&playlist_tile(playlist, state.clone(), index), -1);
    }

    if playlists.is_empty() {
        return;
    }

    let state = state.clone();
    let mut next_index = initial_count;
    gtk::glib::idle_add_local(move || {
        if collection_render_generation(&state) != generation {
            return gtk::glib::ControlFlow::Break;
        }

        let batch_count = playlists.len().min(COLLECTION_TILE_IDLE_BATCH);
        for playlist in playlists.drain(..batch_count) {
            grid.insert(&playlist_tile(playlist, state.clone(), next_index), -1);
            next_index += 1;
        }

        if playlists.is_empty() {
            gtk::glib::ControlFlow::Break
        } else {
            gtk::glib::ControlFlow::Continue
        }
    });
}

fn collection_empty_state(text: &str) -> gtk::Box {
    let empty = gtk::Box::new(Orientation::Vertical, 8);
    empty.add_css_class("collection-empty");
    empty.append(&label(text, "rail-title"));
    empty.append(&label(
        "Try a different search or connect to Jellyfin.",
        "meta",
    ));
    empty
}

fn album_tile(album: AlbumSummary, state: Rc<RefCell<UiState>>, tile_index: usize) -> gtk::Button {
    let button = collection_tile_button(&album.name);
    button.add_css_class("album-tile");
    button.set_halign(Align::Fill);
    button.set_hexpand(false);
    button.set_size_request(COLLECTION_TILE_WIDTH, -1);

    let layout = gtk::Box::new(Orientation::Vertical, 8);
    layout.set_halign(Align::Fill);
    layout.set_hexpand(true);

    let frame = gtk::Box::new(Orientation::Vertical, 0);
    frame.add_css_class("album-art-frame");
    frame.set_size_request(ALBUM_ART_SIZE, ALBUM_ART_SIZE);
    frame.set_halign(Align::Center);
    frame.set_valign(Align::Fill);
    frame.set_overflow(gtk::Overflow::Hidden);

    let art = cover_art(ALBUM_ART_SIZE);
    art.add_css_class("collection-art");
    art.add_css_class("album-art");
    art.set_halign(Align::Fill);
    art.set_valign(Align::Fill);
    art.set_icon_name(Some("audio-x-generic-symbolic"));
    if let Some(url) = album.artwork_url.clone() {
        art.add_css_class("artwork-loading");
        let current_url = Rc::new(RefCell::new(Some(url.clone())));
        load_collection_queue_art(Some(url), art.clone(), current_url, tile_index);
    }
    frame.append(&art);
    layout.append(&frame);
    layout.append(&collection_tile_label(&album.name, "collection-title"));
    layout.append(&collection_tile_label(
        &format!(
            "{} | {}",
            album.artist,
            count_text(album.song_count, "song", "songs")
        ),
        "collection-subtitle",
    ));
    button.set_child(Some(&layout));

    button.connect_clicked(move |_| {
        show_album_tracks(&state, &album);
    });
    button
}

fn artist_tile(
    artist: ArtistSummary,
    state: Rc<RefCell<UiState>>,
    tile_index: usize,
) -> gtk::Button {
    let button = collection_tile_button(&artist.name);
    button.add_css_class("artist-tile");

    let layout = gtk::Box::new(Orientation::Vertical, 8);
    layout.set_halign(Align::Fill);
    layout.set_hexpand(true);

    let avatar = gtk::Image::from_icon_name("avatar-default-symbolic");
    avatar.add_css_class("artist-art");
    avatar.add_css_class("artist-placeholder");
    avatar.set_size_request(ARTIST_ART_SIZE, ARTIST_ART_SIZE);
    avatar.set_halign(Align::Center);
    avatar.set_overflow(gtk::Overflow::Hidden);
    avatar.set_pixel_size(56);
    if let Some(url) = artist.image_url.clone() {
        avatar.add_css_class("artwork-loading");
        load_collection_picture_art(url, avatar.clone(), tile_index);
    }
    layout.append(&avatar);
    layout.append(&collection_tile_label(&artist.name, "collection-title"));
    layout.append(&collection_tile_label(
        &artist_count_text(artist.album_count, artist.song_count),
        "meta",
    ));
    button.set_child(Some(&layout));

    button.connect_clicked(move |_| {
        show_artist_albums(&state, &artist);
    });
    button
}

fn playlist_tile(
    playlist: UiPlaylist,
    state: Rc<RefCell<UiState>>,
    tile_index: usize,
) -> gtk::Button {
    let button = collection_tile_button(&playlist.name);
    button.add_css_class("playlist-tile");
    button.set_halign(Align::Fill);
    button.set_hexpand(false);
    button.set_size_request(COLLECTION_TILE_WIDTH, -1);

    let layout = gtk::Box::new(Orientation::Vertical, 8);
    layout.set_halign(Align::Fill);
    layout.set_hexpand(true);

    let frame = gtk::Box::new(Orientation::Vertical, 0);
    frame.add_css_class("album-art-frame");
    frame.set_size_request(ALBUM_ART_SIZE, ALBUM_ART_SIZE);
    frame.set_halign(Align::Center);
    frame.set_valign(Align::Fill);
    frame.set_overflow(gtk::Overflow::Hidden);

    let art = cover_art(ALBUM_ART_SIZE);
    art.add_css_class("collection-art");
    art.add_css_class("album-art");
    art.set_halign(Align::Fill);
    art.set_valign(Align::Fill);
    art.set_icon_name(Some("media-playlist-consecutive-symbolic"));
    if let Some(url) = playlist.thumbnail_artwork_url.clone() {
        art.add_css_class("artwork-loading");
        let current_url = Rc::new(RefCell::new(Some(url.clone())));
        load_collection_queue_art(Some(url), art.clone(), current_url, tile_index);
    }
    frame.append(&art);
    layout.append(&frame);
    layout.append(&collection_tile_label(&playlist.name, "collection-title"));
    layout.append(&collection_tile_label(
        &count_text(playlist.tracks.len(), "song", "songs"),
        "collection-subtitle",
    ));
    button.set_child(Some(&layout));

    button.connect_clicked(move |_| {
        show_playlist_tracks(&state, &playlist);
    });
    button
}

fn collection_tile_button(title: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("collection-tile");
    button.set_halign(Align::Fill);
    button.set_valign(Align::Start);
    button.set_size_request(COLLECTION_TILE_WIDTH, -1);
    button.set_tooltip_text(Some(title));
    button.set_cursor_from_name(Some("pointer"));
    button
}

fn collection_tile_label(text: &str, class_name: &str) -> gtk::Label {
    let title = label(text, class_name);
    title.set_xalign(0.5);
    title.set_justify(gtk::Justification::Center);
    title.set_single_line_mode(true);
    title.set_lines(1);
    title.set_halign(Align::Fill);
    title.set_hexpand(true);
    title.set_width_chars(1);
    title.set_max_width_chars(24);
    title
}

fn album_summaries(tracks: &[UiTrack], query: &str) -> Vec<AlbumSummary> {
    let mut albums = Vec::<AlbumAccumulator>::new();
    let mut album_indexes = HashMap::<String, usize>::new();
    for (track_index, track) in tracks.iter().enumerate() {
        let key = album_key(track);
        if let Some(album_index) = album_indexes.get(&key).copied() {
            let album = &mut albums[album_index];
            album.song_count += 1;
            if album.artwork_url.is_none() {
                album.artwork_url = track.thumbnail_artwork_url.clone();
            }
            add_artist_vote(
                &mut album.explicit_artist_votes,
                track.album_artist.as_deref(),
                track
                    .album_artist
                    .as_deref()
                    .and_then(|artist| track.artist_thumbnail_url_for(artist)),
                track_index,
            );
            add_artist_vote(
                &mut album.fallback_artist_votes,
                Some(&track.artist),
                track.artist_thumbnail_url_for(&track.artist),
                track_index,
            );
            continue;
        }

        album_indexes.insert(key.clone(), albums.len());
        let mut album = AlbumAccumulator {
            key,
            name: track.album.clone(),
            artwork_url: track.thumbnail_artwork_url.clone(),
            song_count: 1,
            explicit_artist_votes: Vec::new(),
            fallback_artist_votes: Vec::new(),
        };
        add_artist_vote(
            &mut album.explicit_artist_votes,
            track.album_artist.as_deref(),
            track
                .album_artist
                .as_deref()
                .and_then(|artist| track.artist_thumbnail_url_for(artist)),
            track_index,
        );
        add_artist_vote(
            &mut album.fallback_artist_votes,
            Some(&track.artist),
            track.artist_thumbnail_url_for(&track.artist),
            track_index,
        );
        albums.push(album);
    }

    let mut albums = albums
        .into_iter()
        .map(|album| {
            let preferred_artist =
                preferred_album_artist(&album.explicit_artist_votes, &album.fallback_artist_votes);
            AlbumSummary {
                key: album.key,
                name: album.name,
                artist: preferred_artist.name,
                artist_image_url: preferred_artist.image_url,
                artwork_url: album.artwork_url,
                song_count: album.song_count,
            }
        })
        .collect::<Vec<_>>();

    let query = query.trim().to_lowercase();
    if !query.is_empty() {
        albums.retain(|album| {
            album.name.to_lowercase().contains(&query)
                || album.artist.to_lowercase().contains(&query)
        });
    }

    albums.sort_by(|left, right| {
        compare_text(&left.name, &right.name)
            .then_with(|| compare_text(&left.artist, &right.artist))
    });
    albums
}

fn album_summaries_for_artist_from(
    albums: &[AlbumSummary],
    selected_artist_key: &str,
    query: &str,
) -> Vec<AlbumSummary> {
    filter_album_summaries(albums, query)
        .into_iter()
        .filter(|album| artist_key(&album.artist) == selected_artist_key)
        .collect()
}

fn filter_album_summaries(albums: &[AlbumSummary], query: &str) -> Vec<AlbumSummary> {
    let query = query.trim().to_lowercase();
    let mut albums = albums
        .iter()
        .filter(|album| {
            query.is_empty()
                || album.name.to_lowercase().contains(&query)
                || album.artist.to_lowercase().contains(&query)
        })
        .cloned()
        .collect::<Vec<_>>();
    albums.sort_by(|left, right| {
        compare_text(&left.name, &right.name)
            .then_with(|| compare_text(&left.artist, &right.artist))
    });
    albums
}

fn artist_album_song_counts_from(
    albums: &[AlbumSummary],
    selected_artist_key: &str,
    query: &str,
) -> (usize, usize) {
    let albums = album_summaries_for_artist_from(albums, selected_artist_key, query);
    album_song_counts(&albums)
}

fn album_song_counts(albums: &[AlbumSummary]) -> (usize, usize) {
    let song_count = albums.iter().map(|album| album.song_count).sum();
    (albums.len(), song_count)
}

fn artist_summaries(tracks: &[UiTrack], query: &str) -> Vec<ArtistSummary> {
    struct ArtistAccumulator {
        key: String,
        image_url: Option<String>,
        album_count: usize,
        song_count: usize,
        name_votes: Vec<ArtistNameVote>,
    }

    let mut artists = Vec::<ArtistAccumulator>::new();
    let mut artist_indexes = HashMap::<String, usize>::new();
    for (album_index, album) in album_summaries(tracks, "").into_iter().enumerate() {
        let key = artist_key(&album.artist);
        let image_url = artist_summary_image_url(&album);
        if let Some(artist_index) = artist_indexes.get(&key).copied() {
            let artist = &mut artists[artist_index];
            artist.album_count += 1;
            artist.song_count += album.song_count;
            if artist.image_url.is_none() {
                artist.image_url = image_url;
            }
            add_artist_name_vote(
                &mut artist.name_votes,
                &album.artist,
                album.song_count,
                album_index,
            );
            continue;
        }

        artist_indexes.insert(key.clone(), artists.len());
        let mut name_votes = Vec::new();
        add_artist_name_vote(
            &mut name_votes,
            &album.artist,
            album.song_count,
            album_index,
        );
        artists.push(ArtistAccumulator {
            key,
            image_url,
            album_count: 1,
            song_count: album.song_count,
            name_votes,
        });
    }

    let mut artists = artists
        .into_iter()
        .map(|artist| ArtistSummary {
            key: artist.key,
            name: preferred_artist_name(&artist.name_votes)
                .unwrap_or("Unknown Artist")
                .to_string(),
            image_url: artist.image_url,
            album_count: artist.album_count,
            song_count: artist.song_count,
        })
        .collect::<Vec<_>>();

    let query = query.trim().to_lowercase();
    if !query.is_empty() {
        artists.retain(|artist| artist.name.to_lowercase().contains(&query));
    }

    artists.sort_by(|left, right| compare_text(&left.name, &right.name));
    artists
}

fn filter_artist_summaries(artists: &[ArtistSummary], query: &str) -> Vec<ArtistSummary> {
    let query = query.trim().to_lowercase();
    let mut artists = artists
        .iter()
        .filter(|artist| query.is_empty() || artist.name.to_lowercase().contains(&query))
        .cloned()
        .collect::<Vec<_>>();
    artists.sort_by(|left, right| compare_text(&left.name, &right.name));
    artists
}

fn rebuild_library_summaries(ui: &mut UiState) {
    ui.library_albums = album_summaries(&ui.all_tracks, "");
    ui.library_artists = artist_summaries(&ui.all_tracks, "");
}

fn assign_album_positions(tracks: &mut [UiTrack]) {
    let mut album_counts = HashMap::<String, usize>::new();
    for track in tracks {
        let position = album_counts.entry(album_key(track)).or_default();
        track.album_position = Some(*position);
        *position += 1;
    }
}

fn filter_playlists(playlists: &[UiPlaylist], query: &str) -> Vec<UiPlaylist> {
    let query = query.trim().to_lowercase();
    let mut playlists = playlists
        .iter()
        .filter(|playlist| query.is_empty() || playlist.name.to_lowercase().contains(&query))
        .cloned()
        .collect::<Vec<_>>();
    playlists.sort_by(|left, right| compare_text(&left.name, &right.name));
    playlists
}

fn artist_summary_image_url(album: &AlbumSummary) -> Option<String> {
    album.artist_image_url.clone()
}

fn artist_count_text(album_count: usize, song_count: usize) -> String {
    format!(
        "{} | {}",
        count_text(album_count, "album", "albums"),
        count_text(song_count, "song", "songs")
    )
}

fn count_text(count: usize, singular: &str, plural: &str) -> String {
    let noun = if count == 1 { singular } else { plural };
    format!("{count} {noun}")
}

fn album_key(track: &UiTrack) -> String {
    track
        .album_id
        .clone()
        .unwrap_or_else(|| normalized_key(&track.album))
}

fn artist_key(artist: &str) -> String {
    normalized_artist_key(artist)
}

fn normalized_key(value: &str) -> String {
    normalized_text_key(value)
}

fn normalized_text_key(value: &str) -> String {
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

fn normalized_artist_key(value: &str) -> String {
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

fn normalized_artist_display_key(value: &str) -> String {
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

fn push_key_separator(key: &mut String) {
    if !key.is_empty() && !key.ends_with(' ') {
        key.push(' ');
    }
}

fn is_dash_character(character: char) -> bool {
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

fn is_ignored_artist_key_character(character: char) -> bool {
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

fn add_artist_vote(
    votes: &mut Vec<ArtistVote>,
    artist: Option<&str>,
    image_url: Option<String>,
    first_seen: usize,
) {
    let Some(artist) = artist.map(str::trim).filter(|artist| !artist.is_empty()) else {
        return;
    };
    let key = artist_key(artist);
    if let Some(vote) = votes.iter_mut().find(|vote| vote.key == key) {
        vote.count += 1;
        if vote.image_url.is_none() {
            vote.image_url = image_url;
        }
        return;
    }

    votes.push(ArtistVote {
        key,
        name: artist.to_string(),
        image_url,
        count: 1,
        first_seen,
    });
}

fn add_artist_name_vote(
    votes: &mut Vec<ArtistNameVote>,
    artist: &str,
    count: usize,
    first_seen: usize,
) {
    let artist = artist.trim();
    if artist.is_empty() {
        return;
    }

    let key = normalized_artist_display_key(artist);
    if let Some(vote) = votes.iter_mut().find(|vote| vote.key == key) {
        vote.count += count;
        return;
    }

    votes.push(ArtistNameVote {
        key,
        name: artist.to_string(),
        count,
        first_seen,
    });
}

fn preferred_artist_name(votes: &[ArtistNameVote]) -> Option<&str> {
    votes
        .iter()
        .max_by(|left, right| {
            left.count
                .cmp(&right.count)
                .then_with(|| right.first_seen.cmp(&left.first_seen))
        })
        .map(|vote| vote.name.as_str())
}

struct PreferredArtist {
    name: String,
    image_url: Option<String>,
}

fn preferred_album_artist(
    explicit_votes: &[ArtistVote],
    fallback_votes: &[ArtistVote],
) -> PreferredArtist {
    preferred_artist_vote(explicit_votes)
        .or_else(|| preferred_artist_vote(fallback_votes))
        .map(|vote| PreferredArtist {
            name: vote.name.clone(),
            image_url: vote.image_url.clone(),
        })
        .unwrap_or_else(|| PreferredArtist {
            name: "Unknown Artist".to_string(),
            image_url: None,
        })
}

fn preferred_artist_vote(votes: &[ArtistVote]) -> Option<&ArtistVote> {
    votes.iter().max_by(|left, right| {
        left.count
            .cmp(&right.count)
            .then_with(|| right.first_seen.cmp(&left.first_seen))
    })
}

const TITLE_WIDTH: i32 = 260;
const ARTIST_WIDTH: i32 = 160;
const ALBUM_WIDTH: i32 = 220;
const DURATION_WIDTH: i32 = 66;

#[derive(Clone, Copy)]
struct TrackColumn {
    header: &'static str,
    width: i32,
    expand: bool,
    xalign: f32,
    sort_column: SortColumn,
    class_name: Option<&'static str>,
}

const TRACK_COLUMNS: [TrackColumn; 4] = [
    TrackColumn {
        header: "Title",
        width: TITLE_WIDTH,
        expand: true,
        xalign: 0.0,
        sort_column: SortColumn::Title,
        class_name: Some("track-title"),
    },
    TrackColumn {
        header: "Artist",
        width: ARTIST_WIDTH,
        expand: true,
        xalign: 0.0,
        sort_column: SortColumn::Artist,
        class_name: None,
    },
    TrackColumn {
        header: "Album",
        width: ALBUM_WIDTH,
        expand: true,
        xalign: 0.0,
        sort_column: SortColumn::Album,
        class_name: None,
    },
    TrackColumn {
        header: "Time",
        width: DURATION_WIDTH,
        expand: false,
        xalign: 1.0,
        sort_column: SortColumn::Duration,
        class_name: Some("mono"),
    },
];

fn track_table(state: Rc<RefCell<UiState>>) -> gtk::Box {
    let wrapper = gtk::Box::new(Orientation::Vertical, 0);
    wrapper.set_hexpand(true);
    wrapper.set_vexpand(true);

    let stack = gtk::Stack::new();
    stack.set_hexpand(true);
    stack.set_vexpand(true);

    let scroll = gtk::ScrolledWindow::new();
    scroll.add_css_class("track-scroll");
    scroll.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
    scroll.set_overlay_scrolling(true);
    scroll.set_hexpand(true);
    scroll.set_vexpand(true);

    let model = state.borrow().track_model.clone();
    let selection = gtk::SingleSelection::new(Some(model));
    selection.set_autoselect(false);
    selection.set_can_unselect(false);

    let list = gtk::ColumnView::new(Some(selection.clone()));
    list.add_css_class("track-list");
    list.set_single_click_activate(true);
    list.set_hexpand(true);
    list.set_vexpand(true);
    list.set_show_column_separators(false);
    list.set_show_row_separators(true);
    let current_sort = state.borrow().sort_column;
    let current_direction = if state.borrow().sort_ascending {
        gtk::SortType::Ascending
    } else {
        gtk::SortType::Descending
    };
    let mut active_sort_column = None;
    for column in TRACK_COLUMNS {
        let view_column = track_column_view(column, state.clone());
        if column.sort_column == current_sort {
            active_sort_column = Some(view_column.clone());
        }
        list.append_column(&view_column);
    }
    if let Some(sorter) = list.sorter() {
        let state = state.clone();
        sorter.connect_changed(move |sorter, _| {
            let Ok(sorter) = sorter.clone().downcast::<gtk::ColumnViewSorter>() else {
                return;
            };
            let Some(column) = sorter.primary_sort_column() else {
                return;
            };
            let Some(title) = column.title() else {
                return;
            };
            let Some(sort_column) = sort_column_for_header(title.as_str()) else {
                return;
            };
            let sort_ascending = sorter.primary_sort_order() == gtk::SortType::Ascending;
            set_sort_order(&state, sort_column, sort_ascending);
        });
    }
    if let Some(column) = active_sort_column.as_ref() {
        list.sort_by_column(Some(column), current_direction);
    }
    {
        let state = state.clone();
        list.connect_activate(move |_, position| {
            play_track_at(&state, position as usize);
        });
    }

    scroll.set_child(Some(&list));
    stack.add_named(&scroll, Some("list"));

    let empty_state = gtk::Box::new(Orientation::Vertical, 8);
    empty_state.add_css_class("track-empty-state");
    empty_state.set_valign(Align::Start);
    let empty_icon = gtk::Image::from_icon_name("folder-music-symbolic");
    empty_icon.add_css_class("placeholder-icon");
    empty_icon.set_pixel_size(28);
    empty_icon.set_halign(Align::Start);
    let empty = label("Connect to Jellyfin to load your music", "rail-title");
    let empty_detail = label("Your Jellyfin tracks will appear here after sync.", "meta");
    empty_detail.set_wrap(true);
    empty_detail.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    empty_state.append(&empty_icon);
    empty_state.append(&empty);
    empty_state.append(&empty_detail);
    stack.add_named(&empty_state, Some("empty"));
    stack.set_visible_child_name("empty");

    {
        let mut ui = state.borrow_mut();
        ui.track_selection = Some(selection);
        ui.track_stack = Some(stack.clone());
        ui.track_empty = Some(empty);
        ui.track_empty_detail = Some(empty_detail);
    }
    refresh_track_model(&state);

    wrapper.append(&stack);
    wrapper
}

fn track_column_view(column: TrackColumn, state: Rc<RefCell<UiState>>) -> gtk::ColumnViewColumn {
    let factory = gtk::SignalListItemFactory::new();

    let state_setup = state.clone();
    factory.connect_setup(move |_, list_item| {
        let Some(list_item) = list_item.downcast_ref::<gtk::ListItem>() else {
            return;
        };

        if column.sort_column == SortColumn::Title {
            let (cell, _) = track_title_cell(false);
            list_item.set_child(Some(&cell));
        } else {
            let cell = track_cell_label(column);
            list_item.set_child(Some(&cell));
        }

        if let Some(child) = list_item.child() {
            let state = state_setup.clone();
            let list_item = list_item.clone();
            connect_play_next_gesture(&child, move || {
                queue_visible_track_next(&state, list_item.position() as usize)
            });
        }
    });

    let state_bind = state.clone();
    factory.connect_bind(move |_, list_item| {
        let Some(list_item) = list_item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let position = list_item.position() as usize;
        let (track, is_now_playing) = {
            let ui = state_bind.borrow();
            let Some(track) = ui.tracks.get(position).cloned() else {
                return;
            };
            let key = track_key(&track);
            let is_now_playing =
                ui.playback_session.now_playing_key.as_deref() == Some(key.as_str());
            (track, is_now_playing)
        };

        if column.sort_column == SortColumn::Title {
            bind_title_cell(list_item, &track.title, is_now_playing);
            if let Some(indicator) = get_indicator_image(list_item) {
                let key = track_key(&track);
                let mut ui = state_bind.borrow_mut();
                ui.track_indicators
                    .retain(|(_, existing)| existing != &indicator);
                ui.track_indicators.push((key, indicator));
            }
        } else if let Some(label) = list_item
            .child()
            .and_then(|child| child.downcast::<gtk::Label>().ok())
        {
            label.set_text(track_value(&track, column.sort_column));
        }
    });

    if column.sort_column == SortColumn::Title {
        let state = state.clone();
        factory.connect_unbind(move |_, list_item| {
            let Some(list_item) = list_item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            if let Some(indicator) = get_indicator_image(list_item) {
                state
                    .borrow_mut()
                    .track_indicators
                    .retain(|(_, existing)| existing != &indicator);
            }
        });
    }

    let view_column = gtk::ColumnViewColumn::new(Some(column.header), Some(factory));
    view_column.set_resizable(true);
    view_column.set_expand(column.expand);
    let sorter = gtk::CustomSorter::new(|_, _| gtk::Ordering::Equal);
    view_column.set_sorter(Some(&sorter));
    if column.width > 0 {
        view_column.set_fixed_width(column.width);
    }
    view_column
}

fn track_cell_label(column: TrackColumn) -> gtk::Label {
    let cell = label("", column.class_name.unwrap_or_default());
    cell.add_css_class("track-cell");
    if column.sort_column == SortColumn::Duration {
        cell.add_css_class("track-time-cell");
    }
    cell.set_single_line_mode(true);
    cell.set_lines(1);
    cell.set_xalign(column.xalign);
    cell.set_halign(Align::Fill);
    cell.set_hexpand(true);
    cell
}

fn track_title_cell(is_now_playing: bool) -> (gtk::Box, gtk::Image) {
    let cell = gtk::Box::new(Orientation::Horizontal, 7);
    cell.add_css_class("track-cell");
    cell.add_css_class("track-title-cell");
    cell.set_halign(Align::Fill);
    cell.set_hexpand(true);
    cell.set_valign(Align::Center);

    let indicator = gtk::Image::from_icon_name("media-playback-start-symbolic");
    indicator.add_css_class("now-playing-indicator");
    indicator.set_pixel_size(13);
    indicator.set_size_request(14, -1);
    indicator.set_opacity(if is_now_playing { 1.0 } else { 0.0 });
    cell.append(&indicator);

    let title = label("", "track-title");
    title.set_single_line_mode(true);
    title.set_lines(1);
    title.set_halign(Align::Fill);
    title.set_hexpand(true);
    cell.append(&title);

    (cell, indicator)
}

fn bind_title_cell(list_item: &gtk::ListItem, title: &str, is_now_playing: bool) {
    let Some(cell) = list_item
        .child()
        .and_then(|child| child.downcast::<gtk::Box>().ok())
    else {
        return;
    };

    if let Some(indicator) = cell
        .first_child()
        .and_then(|child| child.downcast::<gtk::Image>().ok())
    {
        indicator.set_opacity(if is_now_playing { 1.0 } else { 0.0 });
    }

    let label = cell
        .first_child()
        .and_then(|child| child.next_sibling())
        .and_then(|child| child.downcast::<gtk::Label>().ok());
    if let Some(label) = label {
        label.set_text(title);
    }
}

fn track_value(track: &UiTrack, column: SortColumn) -> &str {
    match column {
        SortColumn::Title => &track.title,
        SortColumn::Artist => &track.artist,
        SortColumn::Album => &track.album,
        SortColumn::Duration => &track.duration,
    }
}

fn sort_column_for_header(header: &str) -> Option<SortColumn> {
    TRACK_COLUMNS
        .iter()
        .find(|column| column.header == header)
        .map(|column| column.sort_column)
}

fn refresh_track_model(state: &Rc<RefCell<UiState>>) {
    {
        let mut ui = state.borrow_mut();
        ui.track_indicators.clear();
    }
    let (
        model,
        selection,
        stack,
        empty,
        empty_detail,
        track_count,
        selected_index,
        empty_text,
        empty_detail_text,
    ) = {
        let ui = state.borrow();
        let (empty_text, empty_detail_text) = if ui.search_query.is_empty() {
            (
                "Connect to Jellyfin to load your music".to_string(),
                "Your Jellyfin tracks will appear here after sync.".to_string(),
            )
        } else {
            (
                format!("No tracks match \"{}\"", ui.search_query),
                "Try a different search or clear the search field.".to_string(),
            )
        };
        (
            ui.track_model.clone(),
            ui.track_selection.clone(),
            ui.track_stack.clone(),
            ui.track_empty.clone(),
            ui.track_empty_detail.clone(),
            ui.tracks.len(),
            ui.selected_index,
            empty_text,
            empty_detail_text,
        )
    };

    let v_adj = stack
        .as_ref()
        .and_then(|s| s.child_by_name("list"))
        .and_then(|c| c.downcast::<gtk::ScrolledWindow>().ok())
        .map(|s| s.vadjustment());

    let old_val = v_adj.as_ref().map(|a| a.value());

    let additions = vec![""; track_count];
    model.splice(0, model.n_items(), &additions);

    if let Some(empty) = empty.as_ref() {
        empty.set_text(&empty_text);
    }
    if let Some(empty_detail) = empty_detail.as_ref() {
        empty_detail.set_text(&empty_detail_text);
    }
    if let Some(stack) = stack.as_ref() {
        stack.set_visible_child_name(if track_count == 0 { "empty" } else { "list" });
    }

    gtk::glib::idle_add_local(move || {
        if let Some(selection) = selection.as_ref()
            && track_count > 0
        {
            selection.set_selected(selected_index.min(track_count.saturating_sub(1)) as u32);
        }
        if let (Some(adj), Some(val)) = (v_adj.as_ref(), old_val) {
            adj.set_value(val);
        }
        gtk::glib::ControlFlow::Break
    });

    rebuild_queue_list(state);
}

fn update_list_indicators(state: &Rc<RefCell<UiState>>) {
    let ui = state.borrow();
    let now_playing_key = ui.playback_session.now_playing_key.as_deref();

    for (track_key_value, indicator) in &ui.track_indicators {
        let is_playing = now_playing_key == Some(track_key_value.as_str());
        indicator.set_opacity(if is_playing { 1.0 } else { 0.0 });
    }
}

fn get_indicator_image(list_item: &gtk::ListItem) -> Option<gtk::Image> {
    list_item
        .child()
        .and_then(|child| child.downcast::<gtk::Box>().ok())
        .and_then(|cell| cell.first_child())
        .and_then(|child| child.downcast::<gtk::Image>().ok())
}

fn set_sort_order(state: &Rc<RefCell<UiState>>, column: SortColumn, ascending: bool) {
    let view_settings = {
        let mut ui = state.borrow_mut();
        if ui.sort_column == column && ui.sort_ascending == ascending {
            return;
        }
        ui.sort_column = column;
        ui.sort_ascending = ascending;
        let view_settings = LibraryViewSettings {
            sort_column: ui.sort_column,
            sort_ascending: ui.sort_ascending,
        };
        apply_track_filter(&mut ui, None);
        update_now_playing_labels(&ui);
        update_play_button(&ui);
        update_page_summary(&ui);
        view_settings
    };
    save_library_view_settings(view_settings);
    refresh_track_model(state);
    load_selected_cover_art(state);
    load_selected_waveform(state);
}

fn load_library_view_settings() -> LibraryViewSettings {
    let result = CacheDatabase::open_default()
        .and_then(|cache| cache.get_setting(LIBRARY_VIEW_SETTINGS_KEY))
        .and_then(|json| {
            json.map(|json| serde_json::from_str(&json).map_err(crate::cache::CacheError::from))
                .transpose()
        });

    match result {
        Ok(Some(settings)) => settings,
        Ok(None) => LibraryViewSettings::default(),
        Err(error) => {
            tracing::warn!(%error, "failed to load library view settings");
            LibraryViewSettings::default()
        }
    }
}

fn save_library_view_settings(settings: LibraryViewSettings) {
    let result = serde_json::to_string(&settings)
        .map_err(crate::cache::CacheError::from)
        .and_then(|json| {
            CacheDatabase::open_default()
                .and_then(|cache| cache.set_setting(LIBRARY_VIEW_SETTINGS_KEY, &json))
        });

    if let Err(error) = result {
        tracing::warn!(%error, "failed to save library view settings");
    }
}

const LIBRARY_VIEW_SETTINGS_KEY: &str = "library.view.settings";
const KEEP_PLAYING_WHILE_CLOSED_KEY: &str = "player.keep_playing_while_closed";
const ANIMATIONS_ENABLED_KEY: &str = "ui.animations.enabled";
const FONT_MONO_KEY: &str = "ui.font.mono";
const PLAYBACK_STATE_KEY: &str = "player.playback.state";
const PLAYBACK_SNAPSHOT_INTERVAL: Duration = Duration::from_secs(5);

fn load_keep_playing_while_closed() -> bool {
    match CacheDatabase::open_default()
        .and_then(|cache| cache.get_setting(KEEP_PLAYING_WHILE_CLOSED_KEY))
    {
        Ok(Some(value)) => value == "true",
        Ok(None) => false,
        Err(error) => {
            tracing::warn!(%error, "failed to load close behavior setting");
            false
        }
    }
}

fn set_keep_playing_while_closed(state: &Rc<RefCell<UiState>>, enabled: bool) {
    state.borrow_mut().keep_playing_while_closed = enabled;
    if let Err(error) = CacheDatabase::open_default().and_then(|cache| {
        cache.set_setting(
            KEEP_PLAYING_WHILE_CLOSED_KEY,
            if enabled { "true" } else { "false" },
        )
    }) {
        tracing::warn!(%error, "failed to save close behavior setting");
    }
}

fn load_animations_enabled() -> bool {
    match CacheDatabase::open_default()
        .and_then(|cache| cache.get_setting(ANIMATIONS_ENABLED_KEY))
    {
        Ok(Some(value)) => value != "false",
        Ok(None) => true,
        Err(error) => {
            tracing::warn!(%error, "failed to load animations setting");
            true
        }
    }
}

fn set_animations_enabled(state: &Rc<RefCell<UiState>>, enabled: bool) {
    state.borrow_mut().animations_enabled = enabled;
    if let Some(gtk_settings) = gtk::Settings::default() {
        gtk_settings.set_gtk_enable_animations(enabled);
    }
    if let Err(error) = CacheDatabase::open_default().and_then(|cache| {
        cache.set_setting(
            ANIMATIONS_ENABLED_KEY,
            if enabled { "true" } else { "false" },
        )
    }) {
        tracing::warn!(%error, "failed to save animations setting");
    }
}

fn load_font_mono() -> bool {
    match CacheDatabase::open_default().and_then(|cache| cache.get_setting(FONT_MONO_KEY)) {
        Ok(Some(value)) => value == "true",
        Ok(None) => false,
        Err(error) => {
            tracing::warn!(%error, "failed to load font style setting");
            false
        }
    }
}

fn set_font_mono(state: &Rc<RefCell<UiState>>, mono: bool) {
    state.borrow_mut().font_mono = mono;
    if let Err(error) = CacheDatabase::open_default().and_then(|cache| {
        cache.set_setting(FONT_MONO_KEY, if mono { "true" } else { "false" })
    }) {
        tracing::warn!(%error, "failed to save font style setting");
    }
}


fn playback_snapshot(ui: &UiState) -> Option<session::PersistedPlaybackState> {
    if ui.playback_session.mode.is_radio() {
        return None;
    }

    let playback_index = ui.playback_session.queue_index?;
    let current_track = ui.playback_session.queue_tracks.get(playback_index)?;
    let current_item_id = current_track.item_id.clone()?;
    if current_item_id.is_empty() {
        return None;
    }

    let position_secs = ui
        .playback
        .as_ref()
        .and_then(PlaybackEngine::position)
        .map(|position| position.as_secs())
        .unwrap_or(0);
    let item_ids_by_index = ui
        .playback_session
        .queue_tracks
        .iter()
        .map(|track| track.item_id.clone())
        .collect::<Vec<_>>();

    session::playback_snapshot(
        current_item_id,
        &item_ids_by_index,
        &ui.playback_session.playback_order,
        position_secs,
        ui.playback_session.shuffle_enabled,
    )
}

fn save_playback_snapshot_now(ui: &mut UiState) {
    ui.last_playback_snapshot_at = Some(Instant::now());
    let Some(snapshot) = playback_snapshot(ui) else {
        return;
    };

    let result = serde_json::to_string(&snapshot)
        .map_err(crate::cache::CacheError::from)
        .and_then(|json| {
            CacheDatabase::open_default()
                .and_then(|cache| cache.set_setting(PLAYBACK_STATE_KEY, &json))
        });

    if let Err(error) = result {
        tracing::warn!(%error, "failed to save playback queue state");
    }
}

fn save_playback_snapshot_if_due(ui: &mut UiState) {
    if ui
        .last_playback_snapshot_at
        .is_some_and(|last_saved| last_saved.elapsed() < PLAYBACK_SNAPSHOT_INTERVAL)
    {
        return;
    }
    save_playback_snapshot_now(ui);
}

fn clear_playback_snapshot() {
    if let Err(error) = CacheDatabase::open_default().and_then(|cache| {
        cache
            .connection()
            .execute(
                "DELETE FROM app_settings WHERE key = ?1",
                [PLAYBACK_STATE_KEY],
            )
            .map(|_| ())
            .map_err(crate::cache::CacheError::from)
    }) {
        tracing::warn!(%error, "failed to clear playback queue state");
    }
}

fn load_playback_snapshot() -> Option<session::PersistedPlaybackState> {
    let result = CacheDatabase::open_default()
        .and_then(|cache| cache.get_setting(PLAYBACK_STATE_KEY))
        .and_then(|json| {
            json.map(|json| {
                serde_json::from_str::<session::PersistedPlaybackState>(&json)
                    .map_err(crate::cache::CacheError::from)
            })
            .transpose()
        });

    match result {
        Ok(Some(snapshot)) if snapshot.version == session::PLAYBACK_STATE_VERSION => Some(snapshot),
        Ok(Some(_)) | Ok(None) => None,
        Err(error) => {
            tracing::warn!(%error, "failed to load playback queue state");
            None
        }
    }
}

fn restore_playback_snapshot_tracks(
    library_tracks: &[UiTrack],
    snapshot: &session::PersistedPlaybackState,
) -> Option<(Vec<UiTrack>, usize, Vec<usize>)> {
    let tracks_by_id = library_tracks
        .iter()
        .filter_map(|track| track.item_id.as_deref().map(|item_id| (item_id, track)))
        .collect::<HashMap<_, _>>();
    let library_item_ids = library_tracks
        .iter()
        .filter_map(|track| track.item_id.clone())
        .collect::<Vec<_>>();
    let restored = session::restore_ordered_item_ids(
        &library_item_ids,
        &snapshot.ordered_item_ids,
        &snapshot.current_item_id,
    )?;
    let playback_tracks = restored
        .item_ids
        .iter()
        .filter_map(|item_id| {
            tracks_by_id
                .get(item_id.as_str())
                .map(|track| (*track).clone())
        })
        .collect::<Vec<_>>();

    Some((
        playback_tracks,
        restored.current_index,
        restored.playback_order,
    ))
}

fn sort_track_slice(
    tracks: &mut [UiTrack],
    column: SortColumn,
    ascending: bool,
    search_query: &str,
    album_order_first: bool,
    selected_key: Option<&str>,
    selected_index: &mut usize,
) {
    let normalized_query = search_query.trim().to_lowercase();
    tracks.sort_by(|left, right| {
        let exact_match_ordering = if normalized_query.is_empty() {
            Ordering::Equal
        } else {
            exact_title_match_rank(left, &normalized_query)
                .cmp(&exact_title_match_rank(right, &normalized_query))
        };

        let column_ordering = match column {
            SortColumn::Title => compare_text(&left.title, &right.title),
            SortColumn::Artist => compare_artist_album_track(left, right),
            SortColumn::Album => compare_text(&left.album, &right.album),
            SortColumn::Duration => {
                duration_seconds(&left.duration).cmp(&duration_seconds(&right.duration))
            }
        };

        let ordering = if album_order_first {
            compare_album_track_order(left, right).then(exact_match_ordering)
        } else {
            exact_match_ordering.then(column_ordering)
        }
        .then_with(|| compare_text(&left.title, &right.title))
        .then_with(|| compare_text(&left.artist, &right.artist))
        .then_with(|| compare_text(&left.album, &right.album));

        if ascending || album_order_first {
            ordering
        } else {
            ordering.reverse()
        }
    });

    if let Some(selected_key) = selected_key {
        *selected_index = tracks
            .iter()
            .position(|track| track_key(track) == selected_key)
            .unwrap_or(0);
    } else {
        *selected_index = 0;
    }
}

fn set_search_query(state: &Rc<RefCell<UiState>>, query: &str) {
    let show_tracks = state.borrow().is_track_list_visible();
    {
        let mut ui = state.borrow_mut();
        if ui.search_query == query {
            return;
        }
        ui.search_query = query.to_string();
        if show_tracks {
            let selected_key = ui.tracks.get(ui.selected_index).map(track_key);
            apply_track_filter(&mut ui, selected_key.as_deref());
            update_now_playing_labels(&ui);
            update_play_button(&ui);
        }
        update_page_summary(&ui);
    }
    if show_tracks {
        refresh_track_model(state);
    }
    refresh_visible_collection_grid(state);
    update_content_view(state, NavDirection::DrillForward);
    if show_tracks {
        load_selected_cover_art(state);
        load_selected_waveform(state);
    }
}

fn library_page_order(page: LibraryPage) -> usize {
    match page {
        LibraryPage::Tracks => 0,
        LibraryPage::Albums => 1,
        LibraryPage::Artists => 2,
        LibraryPage::Playlists => 3,
        LibraryPage::Radio => 4,
        LibraryPage::NextUp => 5,
    }
}

fn set_library_page(state: &Rc<RefCell<UiState>>, page: LibraryPage) {
    let show_tracks = page == LibraryPage::Tracks;
    let mut refresh_tracks = false;
    let old_page;
    {
        let mut ui = state.borrow_mut();
        if ui.active_page == page
            && ui.album_filter.is_none()
            && ui.artist_filter.is_none()
            && ui.playlist_filter.is_none()
            && ui.collection_detail_title.is_none()
            && ui.collection_detail_subtitle.is_none()
            && ui.collection_detail_parent_search_query.is_none()
            && ui.collection_return_target.is_none()
            && ui.collection_parent_return_target.is_none()
        {
            return;
        }
        old_page = ui.active_page;
        ui.active_page = page;
        ui.album_filter = None;
        ui.artist_filter = None;
        ui.playlist_filter = None;
        ui.collection_detail_title = None;
        ui.collection_detail_subtitle = None;
        ui.collection_detail_parent_search_query = None;
        ui.collection_return_target = None;
        ui.collection_parent_return_target = None;
        if show_tracks {
            if !ui.track_filter_is_current() {
                let selected_key = ui.tracks.get(ui.selected_index).map(track_key);
                apply_track_filter(&mut ui, selected_key.as_deref());
                refresh_tracks = true;
            }
            update_now_playing_labels(&ui);
            update_play_button(&ui);
        }
        update_page_summary(&ui);
    }
    let direction = if library_page_order(page) >= library_page_order(old_page) {
        NavDirection::PageForward
    } else {
        NavDirection::PageBackward
    };
    if refresh_tracks {
        refresh_track_model(state);
    }
    refresh_visible_collection_grid(state);
    update_nav_selection(state);
    update_content_view(state, direction);
    focus_active_collection_grid(state);
    if refresh_tracks {
        load_selected_cover_art(state);
        load_selected_waveform(state);
    }
}

fn show_album_tracks(state: &Rc<RefCell<UiState>>, album: &AlbumSummary) {
    let selected_key = {
        let ui = state.borrow();
        current_display_track(&ui).and_then(|track| track_key_if_same_album(track, &album.key))
    };
    let return_target = collection_return_target_for_key(state, album.key.clone());
    save_active_collection_scroll_position(state);

    {
        let mut ui = state.borrow_mut();
        let selected_artist = if ui.active_page == LibraryPage::Artists {
            ui.artist_filter.clone()
        } else {
            None
        };
        let parent_return_target = selected_artist
            .is_some()
            .then(|| ui.collection_return_target.clone())
            .flatten();
        ui.active_page = if selected_artist.is_some() {
            LibraryPage::Artists
        } else {
            LibraryPage::Albums
        };
        ui.playlist_filter = None;
        ui.album_filter = Some(album.key.clone());
        ui.artist_filter = selected_artist;
        ui.collection_detail_title = Some(album.name.clone());
        ui.collection_detail_subtitle = Some(format!(
            "{} | {}",
            album.artist,
            count_text(album.song_count, "song", "songs")
        ));
        ui.collection_detail_parent_search_query = Some(ui.search_query.clone());
        ui.collection_return_target = return_target;
        ui.collection_parent_return_target = parent_return_target;
        apply_track_filter(&mut ui, selected_key.as_deref());
        update_now_playing_labels(&ui);
        update_play_button(&ui);
        update_page_summary(&ui);
    }
    refresh_track_model(state);
    update_content_view(state, NavDirection::DrillForward);
    load_selected_cover_art(state);
    load_selected_waveform(state);
}

fn restore_persisted_playback(state: &Rc<RefCell<UiState>>) {
    if state.borrow().playback_session.now_playing_key.is_some() {
        return;
    }

    let Some(snapshot) = load_playback_snapshot() else {
        return;
    };

    let restored = {
        let mut ui = state.borrow_mut();
        let Some((playback_tracks, playback_index, playback_order)) =
            restore_playback_snapshot_tracks(&ui.all_tracks, &snapshot)
        else {
            return;
        };
        let tracks = ui.tracks.clone();
        let fallback_selected_index = ui.selected_index;
        let Some(selection) = ui.playback_session.restore_library_playback(
            session::RestoredPlayback {
                tracks: playback_tracks,
                current_index: playback_index,
                playback_order,
                shuffle_enabled: snapshot.shuffle_enabled,
            },
            &tracks,
            fallback_selected_index,
            |queued, visible| track_key(queued) == track_key(visible),
            track_key,
        ) else {
            return;
        };
        ui.selected_index = selection.selected_index;
        update_shuffle_button(&ui);
        update_now_playing_labels(&ui);
        update_play_button(&ui);
        true
    };

    if !restored {
        return;
    }

    let selected_index = state.borrow().selected_index;
    select_track_model_row(state, selected_index);
    scroll_track_list_to_index(state, selected_index);
    rebuild_queue_list(state);
    play_selected_track(state);

    {
        let mut ui = state.borrow_mut();
        if snapshot.position_secs > 0
            && let Some(playback) = ui.playback.as_mut()
            && let Err(error) = playback.seek(Duration::from_secs(snapshot.position_secs))
        {
            ui.playback_status
                .set_text(&format!("Restore seek failed: {error}"));
        }
        if let Some(playback) = ui.playback.as_mut() {
            match playback.pause() {
                Ok(()) => update_now_playing_labels(&ui),
                Err(error) => ui
                    .playback_status
                    .set_text(&format!("Restore pause failed: {error}")),
            }
        }
        save_playback_snapshot_now(&mut ui);
        update_play_button(&ui);
        sync_external_playback_status(&mut ui);
    }

    update_list_indicators(state);
    load_selected_cover_art(state);
    load_selected_waveform(state);
    scroll_to_now_playing(state);
}

fn show_playlist_tracks(state: &Rc<RefCell<UiState>>, playlist: &UiPlaylist) {
    let return_target = collection_return_target_for_key(state, playlist.id.clone());
    save_active_collection_scroll_position(state);

    {
        let mut ui = state.borrow_mut();
        ui.active_page = LibraryPage::Playlists;
        ui.album_filter = None;
        ui.artist_filter = None;
        ui.playlist_filter = Some(playlist.id.clone());
        ui.collection_detail_title = Some(playlist.name.clone());
        ui.collection_detail_subtitle = Some(count_text(playlist.tracks.len(), "song", "songs"));
        ui.collection_detail_parent_search_query = Some(ui.search_query.clone());
        ui.collection_return_target = return_target;
        ui.collection_parent_return_target = None;
        ui.selected_index = 0;
        apply_track_filter(&mut ui, None);
        update_now_playing_labels(&ui);
        update_play_button(&ui);
        update_page_summary(&ui);
    }
    refresh_track_model(state);
    update_content_view(state, NavDirection::DrillForward);
    load_selected_cover_art(state);
    load_selected_waveform(state);
}

fn show_artist_albums(state: &Rc<RefCell<UiState>>, artist: &ArtistSummary) {
    let return_target = collection_return_target_for_key(state, artist.key.clone());
    save_active_collection_scroll_position(state);

    {
        let mut ui = state.borrow_mut();
        ui.active_page = LibraryPage::Artists;
        ui.album_filter = None;
        ui.playlist_filter = None;
        ui.artist_filter = Some(artist.key.clone());
        ui.collection_detail_title = Some(artist.name.clone());
        ui.collection_detail_subtitle =
            Some(artist_count_text(artist.album_count, artist.song_count));
        ui.collection_detail_parent_search_query = Some(ui.search_query.clone());
        ui.collection_return_target = return_target;
        ui.collection_parent_return_target = None;
        ui.selected_index = 0;
        ui.page_summary.set_text(&format!(
            "{} | {}",
            artist.name,
            artist_count_text(artist.album_count, artist.song_count)
        ));
    }
    refresh_visible_collection_grid(state);
    update_content_view(state, NavDirection::DrillForward);
    focus_active_collection_grid(state);
}

fn focus_active_collection_grid(state: &Rc<RefCell<UiState>>) {
    let (
        active_page,
        album_filter,
        artist_filter,
        playlist_filter,
        album_grid,
        artist_grid,
        playlist_grid,
    ) = {
        let ui = state.borrow();
        (
            ui.active_page,
            ui.album_filter.clone(),
            ui.artist_filter.clone(),
            ui.playlist_filter.clone(),
            ui.album_grid.clone(),
            ui.artist_grid.clone(),
            ui.playlist_grid.clone(),
        )
    };

    let grid = match active_page {
        LibraryPage::Albums if album_filter.is_none() => album_grid,
        LibraryPage::Artists if artist_filter.is_none() => artist_grid,
        LibraryPage::Artists if album_filter.is_none() => album_grid,
        LibraryPage::Playlists if playlist_filter.is_none() => playlist_grid,
        _ => None,
    };

    let Some(grid) = grid else {
        return;
    };
    let Some(child) = grid.first_child() else {
        return;
    };
    if let Some(button) = child
        .first_child()
        .and_then(|widget| widget.downcast::<gtk::Button>().ok())
    {
        button.grab_focus();
    } else {
        child.grab_focus();
    }
}

fn return_to_collection_grid(state: &Rc<RefCell<UiState>>) {
    let refresh_grid = {
        let ui = state.borrow();
        ui.collection_detail_parent_search_query
            .as_deref()
            .is_none_or(|query| query != ui.search_query)
    };
    let return_target = state.borrow().collection_return_target.clone();

    {
        let mut ui = state.borrow_mut();
        if ui.active_page == LibraryPage::Artists && ui.album_filter.is_some() {
            ui.album_filter = None;
            ui.collection_return_target = ui.collection_parent_return_target.take();
            if let Some(artist_key_value) = ui.artist_filter.clone() {
                let artist_detail = ui
                    .library_artists
                    .iter()
                    .find(|artist| artist.key == artist_key_value)
                    .map(|artist| {
                        (
                            artist.name.clone(),
                            artist_count_text(artist.album_count, artist.song_count),
                        )
                    });
                if let Some((name, subtitle)) = artist_detail {
                    ui.collection_detail_title = Some(name);
                    ui.collection_detail_subtitle = Some(subtitle);
                }
            }
        } else {
            ui.album_filter = None;
            ui.artist_filter = None;
            ui.playlist_filter = None;
            ui.collection_detail_title = None;
            ui.collection_detail_subtitle = None;
            ui.collection_detail_parent_search_query = None;
            ui.collection_return_target = None;
            ui.collection_parent_return_target = None;
        }
        ui.selected_index = 0;
        update_page_summary(&ui);
    }
    if refresh_grid {
        refresh_visible_collection_grid(state);
    }
    update_content_view(state, NavDirection::DrillBackward);
    focus_active_collection_grid(state);
    restore_active_collection_scroll_position(state);
    pulse_collection_return_target(state, return_target);
}

fn navigate_to_now_playing_artist(state: &Rc<RefCell<UiState>>) {
    let artist = {
        let ui = state.borrow();
        if ui.playback_session.mode.is_radio() {
            return;
        }
        let Some(track) = current_display_track(&ui) else {
            return;
        };
        let key = artist_key(&track.artist);
        ui.library_artists
            .iter()
            .find(|artist| artist.key == key)
            .cloned()
    };

    if let Some(artist) = artist {
        set_library_page(state, LibraryPage::Artists);
        show_artist_albums(state, &artist);
    }
}

fn navigate_to_now_playing_album(state: &Rc<RefCell<UiState>>) {
    let album = {
        let ui = state.borrow();
        if ui.playback_session.mode.is_radio() {
            return;
        }
        let Some(track) = current_display_track(&ui) else {
            return;
        };
        let key = album_key(track);
        ui.library_albums
            .iter()
            .find(|album| album.key == key)
            .cloned()
    };

    if let Some(album) = album {
        set_library_page(state, LibraryPage::Albums);
        show_album_tracks(state, &album);
    }
}

fn update_content_view(state: &Rc<RefCell<UiState>>, direction: NavDirection) {
    let (
        stack,
        detail_header,
        title_label,
        subtitle_label,
        visible_child,
        show_detail,
        title,
        subtitle,
    ) = {
        let ui = state.borrow();
        let show_detail =
            ui.album_filter.is_some() || ui.artist_filter.is_some() || ui.playlist_filter.is_some();
        let visible_child = match (ui.active_page, show_detail) {
            (LibraryPage::Tracks, _) => "tracks",
            (LibraryPage::Albums, false) => "albums",
            (LibraryPage::Albums, true) => "tracks",
            (LibraryPage::Artists, false) => "artists",
            (LibraryPage::Artists, true) if ui.album_filter.is_none() => "albums",
            (LibraryPage::Artists, true) => "tracks",
            (LibraryPage::Playlists, false) => "playlists",
            (LibraryPage::Playlists, true) => "tracks",
            (LibraryPage::Radio, _) => "radio",
            (LibraryPage::NextUp, _) => "next-up",
        };
        (
            ui.library_stack.clone(),
            ui.detail_header.clone(),
            ui.detail_title_label.clone(),
            ui.detail_subtitle_label.clone(),
            visible_child.to_string(),
            show_detail,
            ui.collection_detail_title.clone().unwrap_or_default(),
            ui.collection_detail_subtitle.clone().unwrap_or_default(),
        )
    };

    if let Some(stack) = stack.as_ref() {
        stack.set_transition_type(match direction {
            NavDirection::DrillForward => gtk::StackTransitionType::SlideLeft,
            NavDirection::DrillBackward => gtk::StackTransitionType::SlideRight,
            NavDirection::PageForward => gtk::StackTransitionType::SlideUp,
            NavDirection::PageBackward => gtk::StackTransitionType::SlideDown,
        });
        stack.set_visible_child_name(&visible_child);
    }
    if let Some(header) = detail_header.as_ref() {
        header.set_visible(show_detail);
    }
    {
        let (sidebar_queue_card, show_sidebar_queue) = {
            let ui = state.borrow();
            (
                ui.sidebar_queue_card.clone(),
                !matches!(ui.active_page, LibraryPage::NextUp | LibraryPage::Radio),
            )
        };
        if let Some(queue_card) = sidebar_queue_card.as_ref() {
            queue_card.set_visible(show_sidebar_queue);
        }
    }
    if let Some(label) = title_label.as_ref() {
        label.set_text(&title);
    }
    if let Some(label) = subtitle_label.as_ref() {
        label.set_text(&subtitle);
    }
}

fn update_nav_counts(state: &Rc<RefCell<UiState>>) {
    let (
        track_label,
        album_label,
        artist_label,
        playlist_label,
        radio_label,
        tracks,
        albums,
        artists,
        playlists,
        radio_stations,
    ) = {
        let ui = state.borrow();
        (
            ui.nav_track_count.clone(),
            ui.nav_album_count.clone(),
            ui.nav_artist_count.clone(),
            ui.nav_playlist_count.clone(),
            ui.nav_radio_count.clone(),
            ui.all_tracks.len(),
            ui.library_albums.len(),
            ui.library_artists.len(),
            ui.playlists.len(),
            radio_stations_for_display_from(&ui.radio_stations).len(),
        )
    };

    if let Some(label) = track_label.as_ref() {
        label.set_text(&tracks.to_string());
    }
    if let Some(label) = album_label.as_ref() {
        label.set_text(&albums.to_string());
    }
    if let Some(label) = artist_label.as_ref() {
        label.set_text(&artists.to_string());
    }
    if let Some(label) = playlist_label.as_ref() {
        label.set_text(&playlists.to_string());
    }
    if let Some(label) = radio_label.as_ref() {
        label.set_text(&radio_stations.to_string());
    }
}

fn update_nav_selection(state: &Rc<RefCell<UiState>>) {
    let (list, active_page) = {
        let ui = state.borrow();
        (ui.nav_list.clone(), ui.active_page)
    };
    let Some(list) = list else {
        return;
    };

    let row_index = match active_page {
        LibraryPage::Tracks => Some(0),
        LibraryPage::Albums => Some(1),
        LibraryPage::Artists => Some(2),
        LibraryPage::Playlists => Some(3),
        LibraryPage::Radio => Some(4),
        LibraryPage::NextUp => None,
    };
    if row_index.is_none() {
        list.unselect_all();
        return;
    }
    let Some(row_index) = row_index else {
        return;
    };
    if list.selected_row().as_ref().map(gtk::ListBoxRow::index) == Some(row_index) {
        return;
    }
    if let Some(row) = list.row_at_index(row_index) {
        list.select_row(Some(&row));
    }
}

fn built_in_radio_stations() -> Vec<RadioStation> {
    vec![
        RadioStation::built_in(
            "Lofi",
            "http://radio.cliamp.stream/lofi/stream",
            RADIO_DEFAULT_ICON,
        ),
        RadioStation::built_in(
            "Synthwave",
            "http://radio.cliamp.stream/synthwave/stream",
            RADIO_DEFAULT_ICON,
        ),
        RadioStation::built_in(
            "EDM",
            "http://radio.cliamp.stream/edm/stream",
            RADIO_DEFAULT_ICON,
        ),
    ]
}

fn radio_stations_for_display(state: &Rc<RefCell<UiState>>) -> Vec<RadioStation> {
    let ui = state.borrow();
    radio_stations_for_display_from(&ui.radio_stations)
}

fn radio_stations_for_display_from(custom_stations: &[RadioStation]) -> Vec<RadioStation> {
    let mut stations = built_in_radio_stations();
    stations.extend(
        custom_stations
            .iter()
            .filter(|station| !station.built_in)
            .cloned(),
    );
    stations
}

fn load_radio_stations() -> Vec<RadioStation> {
    match CacheDatabase::open_default().and_then(|cache| cache.get_setting(RADIO_STATIONS_KEY)) {
        Ok(Some(json)) => serde_json::from_str(&json).unwrap_or_else(|error| {
            tracing::warn!(%error, "failed to parse radio stations");
            Vec::new()
        }),
        Ok(None) => Vec::new(),
        Err(error) => {
            tracing::warn!(%error, "failed to load radio stations");
            Vec::new()
        }
    }
}

fn save_radio_stations(stations: &[RadioStation]) {
    let custom = stations
        .iter()
        .filter(|station| !station.built_in)
        .cloned()
        .collect::<Vec<_>>();
    let result = serde_json::to_string(&custom)
        .map_err(crate::cache::CacheError::from)
        .and_then(|json| {
            CacheDatabase::open_default()
                .and_then(|cache| cache.set_setting(RADIO_STATIONS_KEY, &json))
        });
    if let Err(error) = result {
        tracing::warn!(%error, "failed to save radio stations");
    }
}

fn current_radio_station(ui: &UiState) -> Option<RadioStation> {
    let station_id = ui.playback_session.mode.radio_station_id()?;
    radio_stations_for_display_from(&ui.radio_stations)
        .into_iter()
        .find(|station| station.id == station_id)
}

fn persist_custom_radio_station(
    state: &Rc<RefCell<UiState>>,
    name: &str,
    url: &str,
    icon: &str,
) -> bool {
    let Ok(parsed_url) = url.parse::<url::Url>() else {
        return false;
    };
    let source = radio_source_kind_for_url(&parsed_url);
    let icon = icon.trim();
    let icon = if icon.is_empty() {
        None
    } else {
        Some(icon.to_string())
    };

    let mut ui = state.borrow_mut();
    if radio_station_conflicts(&ui.radio_stations, None, name, url) {
        return false;
    }
    let next_id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let next_index = ui.radio_stations.len() + 1;
    ui.radio_stations.push(RadioStation {
        id: format!("custom:{next_id}:{next_index}"),
        name: name.to_string(),
        url: url.to_string(),
        source: source.as_str().to_string(),
        icon,
        built_in: false,
    });
    save_radio_stations(&ui.radio_stations);
    true
}

fn update_custom_radio_station(
    state: &Rc<RefCell<UiState>>,
    station_id: &str,
    name: &str,
    url: &str,
    icon: &str,
) -> bool {
    let Ok(parsed_url) = url.parse::<url::Url>() else {
        return false;
    };
    let source = radio_source_kind_for_url(&parsed_url);
    let icon = icon.trim();
    let icon = if icon.is_empty() {
        None
    } else {
        Some(icon.to_string())
    };

    let mut ui = state.borrow_mut();
    if radio_station_conflicts(&ui.radio_stations, Some(station_id), name, url) {
        return false;
    }

    let Some(station) = ui
        .radio_stations
        .iter_mut()
        .find(|station| station.id == station_id && !station.built_in)
    else {
        return false;
    };

    let previous_station = station.clone();
    station.name = name.to_string();
    station.url = url.to_string();
    station.source = source.as_str().to_string();
    station.icon = icon;

    let updated_station = station.clone();
    let was_current = ui.playback_session.mode.radio_station_id() == Some(station_id);
    let needs_restart = was_current && previous_station.url != updated_station.url;
    save_radio_stations(&ui.radio_stations);
    drop(ui);

    if needs_restart {
        play_radio_station(state, &updated_station);
    } else if was_current {
        {
            let mut ui = state.borrow_mut();
            set_active_radio_station_ui(&mut ui, &updated_station, None);
            ui.playback_status.set_text("Radio station updated");
        }
        refresh_radio_page(state);
    } else {
        refresh_radio_page(state);
    }

    true
}

fn radio_station_conflicts(
    stations: &[RadioStation],
    ignored_station_id: Option<&str>,
    name: &str,
    url: &str,
) -> bool {
    stations.iter().any(|station| {
        ignored_station_id != Some(station.id.as_str())
            && (station.name.eq_ignore_ascii_case(name) || station.url == url)
    })
}

fn refresh_radio_page(state: &Rc<RefCell<UiState>>) {
    let Some(grid) = state.borrow().radio_grid.clone() else {
        return;
    };
    while let Some(child) = grid.first_child() {
        grid.remove(&child);
    }

    for station in radio_stations_for_display(state) {
        grid.insert(&radio_station_card(state.clone(), station), -1);
    }

    update_nav_counts(state);
    let ui = state.borrow();
    if ui.active_page == LibraryPage::Radio {
        update_page_summary(&ui);
    }
}

fn radio_station_card(state: Rc<RefCell<UiState>>, station: RadioStation) -> gtk::Overlay {
    let card = gtk::Overlay::new();
    card.add_css_class("radio-station-card");
    card.set_width_request(RADIO_CARD_CONTENT_WIDTH);
    card.set_height_request(RADIO_CARD_CONTENT_WIDTH);
    card.set_halign(Align::Start);
    card.set_valign(Align::Start);
    card.set_hexpand(false);
    card.set_vexpand(false);
    card.set_tooltip_text(Some(&station.url));
    card.set_cursor_from_name(Some("pointer"));
    let is_current =
        state.borrow().playback_session.mode.radio_station_id() == Some(station.id.as_str());
    if is_current {
        card.add_css_class("radio-station-card-playing");
    }

    let click = gtk::GestureClick::new();
    click.set_button(1);
    {
        let state = state.clone();
        let station = station.clone();
        click.connect_released(move |_, _, _, _| {
            play_radio_station(&state, &station);
        });
    }
    card.add_controller(click);

    if !station.built_in {
        let edit_card = card.clone();
        let edit_state = state.clone();
        let edit_station = station.clone();
        let edit_click = gtk::GestureClick::new();
        edit_click.set_button(3);
        edit_click.connect_pressed(move |gesture, _, x, y| {
            let popover = radio_station_edit_popover(edit_state.clone(), edit_station.clone());
            popover.set_parent(&edit_card);
            popover.set_has_arrow(true);
            popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
            popover.popup();
            gesture.set_state(gtk::EventSequenceState::Claimed);
        });
        card.add_controller(edit_click);
    }

    let content = gtk::Box::new(Orientation::Vertical, 7);
    content.set_hexpand(true);
    content.set_vexpand(true);

    let status_row = gtk::Box::new(Orientation::Horizontal, 0);
    status_row.set_size_request(-1, 24);
    status_row.set_hexpand(true);
    if is_current {
        status_row.append(&radio_status_badge("On Air"));
    }
    content.append(&status_row);

    let icon = radio_station_icon(station.icon_glyph());
    content.append(&icon);

    let text = gtk::Box::new(Orientation::Vertical, 2);
    text.set_halign(Align::Fill);
    text.set_valign(Align::End);
    text.set_vexpand(true);
    text.set_size_request(0, -1);
    let title = label(&station.name, "radio-station-title");
    title.set_xalign(0.5);
    title.set_justify(gtk::Justification::Center);
    title.set_single_line_mode(true);
    title.set_lines(1);
    title.set_width_chars(1);
    title.set_max_width_chars(18);
    text.append(&title);
    let subtitle = label(&radio_station_subtitle(&station), "meta");
    subtitle.set_xalign(0.5);
    subtitle.set_justify(gtk::Justification::Center);
    subtitle.set_single_line_mode(true);
    subtitle.set_lines(1);
    subtitle.set_width_chars(1);
    subtitle.set_max_width_chars(18);
    text.append(&subtitle);
    content.append(&text);
    card.set_child(Some(&content));

    if !station.built_in {
        let remove = icon_button("user-trash-symbolic", "Remove station");
        remove.add_css_class("radio-remove-button");
        remove.set_halign(Align::End);
        remove.set_valign(Align::Start);
        remove.set_margin_top(2);
        remove.set_margin_end(2);
        let station_id = station.id.clone();
        let state = state.clone();
        remove.connect_clicked(move |_| {
            let mut ui = state.borrow_mut();
            let was_current =
                ui.playback_session.mode.radio_station_id() == Some(station_id.as_str());
            ui.radio_stations
                .retain(|candidate| candidate.id != station_id);
            if was_current {
                stop_playback(&mut ui);
                update_now_playing_labels(&ui);
                ui.playback_status.set_text("Radio station removed");
            }
            save_radio_stations(&ui.radio_stations);
            drop(ui);
            refresh_radio_page(&state);
        });
        card.add_overlay(&remove);
    }

    card
}

fn radio_station_subtitle(station: &RadioStation) -> String {
    if station.built_in {
        return "Stream Preset".to_string();
    }

    match station.source_kind() {
        RadioSourceKind::Stream => "Custom Stream".to_string(),
        RadioSourceKind::YouTube => "YouTube live".to_string(),
        RadioSourceKind::Twitch => "Twitch live".to_string(),
    }
}

fn radio_station_form_popover<F>(
    title: &str,
    submit_label: &str,
    name: &str,
    url: &str,
    icon: &str,
    on_submit: F,
) -> (gtk::Popover, gtk::Entry, gtk::Entry, gtk::Entry)
where
    F: Fn(String, String, String) -> bool + 'static,
{
    let popover = gtk::Popover::new();
    popover.add_css_class("radio-add-popover");

    let panel = gtk::Box::new(Orientation::Vertical, 10);
    panel.add_css_class("radio-add-panel");
    panel.append(&label(title, "rail-title"));

    let name_entry = gtk::Entry::new();
    name_entry.set_placeholder_text(Some("Station name"));
    name_entry.set_text(name);
    name_entry.set_hexpand(true);
    panel.append(&name_entry);

    let url_entry = gtk::Entry::new();
    url_entry.set_placeholder_text(Some("Stream, YouTube live, or Twitch URL"));
    url_entry.set_text(url);
    url_entry.set_hexpand(true);
    panel.append(&url_entry);

    let icon_entry = gtk::Entry::new();
    icon_entry.set_placeholder_text(Some("Nerd Font icon (optional)"));
    icon_entry.set_text(icon);
    icon_entry.set_hexpand(true);
    icon_entry.set_max_length(1);
    icon_entry.set_width_chars(1);
    panel.append(&icon_entry);

    let submit_button = gtk::Button::with_label(submit_label);
    submit_button.add_css_class("connection-button");
    submit_button.add_css_class("suggested-action");
    panel.append(&submit_button);
    popover.set_child(Some(&panel));

    let popover_for_submit = popover.clone();
    let name_entry_for_submit = name_entry.clone();
    let url_entry_for_submit = url_entry.clone();
    let icon_entry_for_submit = icon_entry.clone();
    submit_button.connect_clicked(move |_| {
        let name = name_entry_for_submit.text().trim().to_string();
        let url = url_entry_for_submit.text().trim().to_string();
        let icon = icon_entry_for_submit.text().trim().to_string();
        if name.is_empty() || url.is_empty() {
            return;
        }
        if on_submit(name, url, icon) {
            name_entry_for_submit.set_text("");
            url_entry_for_submit.set_text("");
            icon_entry_for_submit.set_text("");
            popover_for_submit.popdown();
        }
    });

    (popover, name_entry, url_entry, icon_entry)
}

fn radio_station_edit_popover(state: Rc<RefCell<UiState>>, station: RadioStation) -> gtk::Popover {
    let station_id = station.id.clone();
    let (popover, _, _, _) = radio_station_form_popover(
        "Edit Station",
        "Save Changes",
        station.name.as_str(),
        station.url.as_str(),
        station.icon.as_deref().unwrap_or(""),
        move |name, url, icon| update_custom_radio_station(&state, &station_id, &name, &url, &icon),
    );
    popover
}

fn radio_status_badge(text: &str) -> gtk::Label {
    let badge = label(text, "radio-playing-badge");
    badge.set_halign(Align::End);
    badge.set_valign(Align::Start);
    badge
}

fn radio_icon(size: i32) -> gtk::DrawingArea {
    let icon = gtk::DrawingArea::new();
    icon.add_css_class("radio-receiver-icon");
    icon.set_content_width(size);
    icon.set_content_height(size);
    icon.set_size_request(size, size);
    icon.set_draw_func(move |area, context, width, height| {
        let color = area.color();
        context.set_source_rgba(
            color.red() as f64,
            color.green() as f64,
            color.blue() as f64,
            0.92,
        );

        let scale = f64::from(width.min(height)) / 48.0;
        context.scale(scale, scale);
        context.set_line_width(2.4);
        context.set_line_cap(gtk::cairo::LineCap::Round);
        context.set_line_join(gtk::cairo::LineJoin::Round);

        context.move_to(13.0, 14.0);
        context.line_to(34.0, 7.0);
        let _ = context.stroke();

        rounded_rect(context, 8.0, 17.0, 32.0, 23.0, 5.0);
        let _ = context.stroke();

        context.arc(18.0, 28.5, 5.3, 0.0, std::f64::consts::TAU);
        let _ = context.stroke();

        context.move_to(29.0, 25.0);
        context.line_to(35.0, 25.0);
        context.move_to(29.0, 31.0);
        context.line_to(35.0, 31.0);
        context.move_to(29.0, 37.0);
        context.line_to(35.0, 37.0);
        let _ = context.stroke();
    });
    icon
}

fn radio_station_icon(icon: &str) -> gtk::Label {
    let icon = gtk::Label::new(Some(icon));
    icon.add_css_class("radio-card-icon");
    icon.set_size_request(48, 48);
    icon.set_halign(Align::Center);
    icon.set_valign(Align::Center);
    icon.set_xalign(0.5);
    icon.set_justify(gtk::Justification::Center);
    icon.set_single_line_mode(true);
    icon.set_wrap(false);
    icon.set_lines(1);
    icon.set_width_chars(1);
    icon.set_max_width_chars(1);
    icon
}

fn default_radio_icon_for_kind(kind: RadioSourceKind) -> &'static str {
    match kind {
        RadioSourceKind::Stream => RADIO_DEFAULT_ICON,
        RadioSourceKind::YouTube => RADIO_DEFAULT_ICON,
        RadioSourceKind::Twitch => RADIO_DEFAULT_ICON,
    }
}

fn play_radio_station(state: &Rc<RefCell<UiState>>, station: &RadioStation) {
    let Ok(input_url) = station.url.parse::<url::Url>() else {
        state
            .borrow()
            .playback_status
            .set_text("Radio station URL is invalid");
        return;
    };

    if let Some(external_source) = station.source_kind().external_source() {
        resolve_and_play_radio_station(state, station.clone(), input_url, external_source);
    } else {
        play_resolved_radio_station(state, station, input_url);
    }
}

fn resolve_and_play_radio_station(
    state: &Rc<RefCell<UiState>>,
    station: RadioStation,
    page_url: url::Url,
    external_source: ExternalStreamSource,
) {
    if state.borrow().playback.is_none() {
        state
            .borrow()
            .playback_status
            .set_text("GStreamer playbin is unavailable");
        return;
    }

    let (sender, receiver) = mpsc::channel();
    {
        let mut ui = state.borrow_mut();
        stop_playback(&mut ui);
        set_active_radio_station_ui(
            &mut ui,
            &station,
            Some(&format!(
                "Resolving {} audio stream",
                station.source_label()
            )),
        );
    }
    refresh_radio_page(state);

    std::thread::spawn(move || {
        let result = resolve_external_stream_url(external_source, &page_url)
            .map_err(|error| error.to_string());
        let _ = sender.send(result);
    });

    let state = state.clone();
    let station_id = station.id.clone();
    gtk::glib::timeout_add_local(Duration::from_millis(100), move || {
        match receiver.try_recv() {
            Ok(Ok(stream_url)) => {
                let still_selected = state.borrow().playback_session.mode.radio_station_id()
                    == Some(station_id.as_str());
                if still_selected {
                    play_resolved_radio_station(&state, &station, stream_url);
                }
                gtk::glib::ControlFlow::Break
            }
            Ok(Err(error)) => {
                let mut ui = state.borrow_mut();
                if ui.playback_session.mode.radio_station_id() == Some(station_id.as_str()) {
                    ui.playback_status.set_text(&format!(
                        "{} resolver failed: {error}",
                        station.source_label()
                    ));
                    update_play_button(&ui);
                    sync_external_playback_status(&mut ui);
                }
                gtk::glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => {
                let mut ui = state.borrow_mut();
                if ui.playback_session.mode.radio_station_id() == Some(station_id.as_str()) {
                    ui.playback_status
                        .set_text("Radio resolver stopped unexpectedly");
                    update_play_button(&ui);
                    sync_external_playback_status(&mut ui);
                }
                gtk::glib::ControlFlow::Break
            }
        }
    });
}

fn play_resolved_radio_station(
    state: &Rc<RefCell<UiState>>,
    station: &RadioStation,
    stream_url: url::Url,
) {
    let request = PlaybackRequest {
        item_id: station.id.clone(),
        stream_url,
        http_headers: Vec::new(),
        stream_kind: PlaybackStreamKind::Direct,
        title: station.name.clone(),
    };
    let played = {
        let mut ui = state.borrow_mut();
        ui.playback.as_mut().map(|playback| playback.play(request))
    };
    match played {
        Some(Ok(())) => {
            let mut ui = state.borrow_mut();
            set_active_radio_station_ui(&mut ui, station, None);
            drop(ui);
            update_list_indicators(state);
            refresh_radio_page(state);
        }
        Some(Err(error)) => {
            state
                .borrow()
                .playback_status
                .set_text(&format!("Radio playback failed: {error}"));
        }
        None => {
            state
                .borrow()
                .playback_status
                .set_text("GStreamer playbin is unavailable");
        }
    }
}

fn set_active_radio_station_ui(
    ui: &mut UiState,
    station: &RadioStation,
    status_override: Option<&str>,
) {
    ui.playback_session.activate_radio(station.id.clone());
    ui.elapsed_label.set_text("0:00");
    ui.remaining_label.set_text("--:--");
    clear_track_visuals_for_radio(ui);
    update_now_playing_labels(ui);
    if let Some(status) = status_override {
        ui.playback_status.set_text(status);
    }
    update_play_button(ui);
    sync_external_playback(ui);
}

fn resume_radio_station(state: &Rc<RefCell<UiState>>) -> bool {
    let station = {
        let ui = state.borrow();
        current_radio_station(&ui)
    };

    if let Some(station) = station {
        play_radio_station(state, &station);
        true
    } else {
        false
    }
}

fn clear_track_visuals_for_radio(ui: &mut UiState) {
    {
        let mut waveform = ui.waveform.borrow_mut();
        waveform.peaks.clear();
        waveform.progress = 0.0;
        waveform.loaded_key = None;
        waveform.loading_key = None;
    }
    ui.waveform_status.set_text("Radio stream");
    if let Some(area) = ui.wave_area.as_ref() {
        area.queue_draw();
    }
    if let Some(cover) = ui.cover_art.as_ref() {
        cover.set_paintable(Option::<&gtk::gdk::Paintable>::None);
        cover.set_icon_name(Some("audio-x-generic-symbolic"));
    }
}

fn apply_track_filter(ui: &mut UiState, selected_key: Option<&str>) {
    let query = ui.search_query.to_lowercase();
    let artist_album_keys = ui.artist_filter.as_deref().map(|selected_artist_key| {
        ui.library_albums
            .iter()
            .filter(|album| artist_key(&album.artist) == selected_artist_key)
            .map(|album| album.key.clone())
            .collect::<HashSet<_>>()
    });
    let source_tracks = ui
        .playlist_filter
        .as_deref()
        .and_then(|playlist_id| {
            ui.playlists
                .iter()
                .find(|playlist| playlist.id == playlist_id)
                .map(|playlist| playlist.tracks.as_slice())
        })
        .unwrap_or(ui.all_tracks.as_slice());
    ui.tracks = source_tracks
        .iter()
        .filter(|track| {
            let album_matches = ui
                .album_filter
                .as_deref()
                .map(|key| album_key(track) == key)
                .unwrap_or(true);
            let artist_matches = ui
                .artist_filter
                .as_deref()
                .map(|_| {
                    artist_album_keys
                        .as_ref()
                        .is_some_and(|album_keys| album_keys.contains(&album_key(track)))
                })
                .unwrap_or(true);
            let search_matches = query.is_empty() || track_matches_query(track, &query);
            album_matches && artist_matches && search_matches
        })
        .cloned()
        .collect();
    sort_track_slice(
        &mut ui.tracks,
        ui.sort_column,
        ui.sort_ascending,
        &ui.search_query,
        ui.album_filter.is_some(),
        selected_key,
        &mut ui.selected_index,
    );
    ui.track_filter_signature = ui.current_track_filter_signature();
    if ui.playback_session.queue_tracks.is_empty() {
        let selected_index = ui.selected_index;
        rebuild_playback_order(ui, selected_index);
    }
}

fn update_page_summary(ui: &UiState) {
    if let Some(title) = ui.collection_detail_title.as_deref() {
        if ui.active_page == LibraryPage::Artists && ui.album_filter.is_none() {
            let (album_count, song_count) = ui
                .artist_filter
                .as_deref()
                .map(|artist_key| {
                    artist_album_song_counts_from(&ui.library_albums, artist_key, &ui.search_query)
                })
                .unwrap_or((0, 0));
            ui.page_summary.set_text(&format!(
                "{title} | {}",
                artist_count_text(album_count, song_count)
            ));
            return;
        }
        ui.page_summary
            .set_text(&format!("{title} | {} matching tracks", ui.tracks.len()));
        return;
    }

    match ui.active_page {
        LibraryPage::Tracks => {
            ui.page_summary
                .set_text(&format!("Tracks | {} tracks", ui.all_tracks.len()));
        }
        LibraryPage::Albums => {
            ui.page_summary
                .set_text(&format!("Albums | {} albums", ui.library_albums.len()));
        }
        LibraryPage::Artists => {
            ui.page_summary
                .set_text(&format!("Artists | {} artists", ui.library_artists.len()));
        }
        LibraryPage::Playlists => {
            ui.page_summary
                .set_text(&format!("Playlists | {} playlists", ui.playlists.len()));
        }
        LibraryPage::Radio => {
            ui.page_summary.set_text(&format!(
                "Radio | {} Stations",
                radio_stations_for_display_from(&ui.radio_stations).len()
            ));
        }
        LibraryPage::NextUp => {
            ui.page_summary.set_text(&format!(
                "Next Up | {} queued tracks",
                upcoming_track_count(ui)
            ));
        }
    }
}

fn rebuild_playback_order(ui: &mut UiState, start_index: usize) {
    ui.playback_session
        .rebuild_order_for_library(ui.tracks.len(), start_index);
}

fn next_playback_index(ui: &UiState) -> Option<usize> {
    let current_index = ui.playback_session.current_index_or(ui.selected_index);
    ui.playback_session.next_index(current_index)
}

fn previous_playback_index(ui: &UiState) -> Option<usize> {
    let current_index = ui.playback_session.current_index_or(ui.selected_index);
    ui.playback_session.previous_index(current_index)
}

fn queued_tracks(ui: &UiState) -> Vec<(usize, UiTrack)> {
    queued_tracks_with_limit(ui, QUEUE_PREVIEW_LIMIT)
}

fn next_up_tracks(ui: &UiState) -> Vec<(usize, UiTrack)> {
    queued_tracks_with_limit(ui, NEXT_UP_PAGE_LIMIT)
}

fn queued_tracks_with_limit(ui: &UiState, limit: usize) -> Vec<(usize, UiTrack)> {
    let tracks = if ui.playback_session.queue_tracks.is_empty() {
        ui.tracks.as_slice()
    } else {
        ui.playback_session.queue_tracks.as_slice()
    };
    let current_index = ui.playback_session.current_index_or(ui.selected_index);
    queued_tracks_from_order_with_limit(
        tracks,
        &ui.playback_session.playback_order,
        current_index,
        limit,
    )
}

#[cfg(test)]
fn queued_tracks_from_order(
    tracks: &[UiTrack],
    playback_order: &[usize],
    current_index: usize,
) -> Vec<(usize, UiTrack)> {
    queued_tracks_from_order_with_limit(tracks, playback_order, current_index, QUEUE_PREVIEW_LIMIT)
}

fn queued_tracks_from_order_with_limit(
    tracks: &[UiTrack],
    playback_order: &[usize],
    current_index: usize,
    limit: usize,
) -> Vec<(usize, UiTrack)> {
    session::queued_indices_with_limit(playback_order, current_index, limit)
        .into_iter()
        .filter_map(|index| tracks.get(index).cloned().map(|track| (index, track)))
        .collect()
}

fn upcoming_track_count(ui: &UiState) -> usize {
    let current_index = ui.playback_session.current_index_or(ui.selected_index);
    ui.playback_session.upcoming_count(current_index)
}

fn move_next_up_track(state: &Rc<RefCell<UiState>>, from: usize, to_slot: usize) -> bool {
    let changed = {
        let mut ui = state.borrow_mut();
        let current_index = ui.playback_session.current_index_or(ui.selected_index);
        let changed = ui.playback_session.move_upcoming_track(
            current_index,
            from,
            to_slot,
            NEXT_UP_PAGE_LIMIT,
        );
        if changed {
            arm_gapless_next(&mut ui);
            save_playback_snapshot_now(&mut ui);
        }
        changed
    };
    if changed {
        rebuild_queue_list(state);
    }
    changed
}

fn queue_track_next(ui: &mut UiState, target_track: UiTrack) -> bool {
    let tracks = ui.tracks.clone();
    ui.playback_session.queue_library_track_next(
        &tracks,
        ui.selected_index,
        target_track,
        |queued, target| track_key(queued) == track_key(target),
    )
}

fn finalize_queue_change(state: &Rc<RefCell<UiState>>) {
    {
        let mut ui = state.borrow_mut();
        arm_gapless_next(&mut ui);
        save_playback_snapshot_now(&mut ui);
        update_page_summary(&ui);
    }
    rebuild_queue_list(state);
}

fn queue_visible_track_next(state: &Rc<RefCell<UiState>>, visible_index: usize) -> bool {
    let changed = {
        let mut ui = state.borrow_mut();
        let Some(track) = ui.tracks.get(visible_index).cloned() else {
            return false;
        };
        queue_track_next(&mut ui, track)
    };
    if changed {
        finalize_queue_change(state);
    }
    changed
}

fn queue_existing_track_next(state: &Rc<RefCell<UiState>>, playback_index: usize) -> bool {
    let changed = {
        let mut ui = state.borrow_mut();
        let track = if ui.playback_session.queue_tracks.is_empty() {
            ui.tracks.get(playback_index).cloned()
        } else {
            ui.playback_session
                .queue_tracks
                .get(playback_index)
                .cloned()
        };
        let Some(track) = track else {
            return false;
        };
        queue_track_next(&mut ui, track)
    };
    if changed {
        finalize_queue_change(state);
    }
    changed
}

fn connect_play_next_gesture<F>(widget: &impl IsA<gtk::Widget>, handler: F)
where
    F: Fn() -> bool + 'static,
{
    let gesture = gtk::GestureClick::new();
    gesture.set_button(0);
    gesture.connect_pressed(move |gesture, _, _, _| {
        let button = gesture.current_button();
        let modifiers = gesture.current_event_state();
        let should_queue_next = button == 3
            || (button == 1 && modifiers.contains(gtk::gdk::ModifierType::CONTROL_MASK));
        if should_queue_next && handler() {
            gesture.set_state(gtk::EventSequenceState::Claimed);
        }
    });
    widget.add_controller(gesture);
}

fn track_matches_query(track: &UiTrack, query: &str) -> bool {
    track.title.to_lowercase().contains(query)
        || track.artist.to_lowercase().contains(query)
        || track.album.to_lowercase().contains(query)
}

fn exact_title_match_rank(track: &UiTrack, query: &str) -> u8 {
    if track.title.trim().eq_ignore_ascii_case(query) {
        0
    } else {
        1
    }
}

fn track_key(track: &UiTrack) -> String {
    track
        .item_id
        .clone()
        .unwrap_or_else(|| format!("{}\u{1f}{}\u{1f}{}", track.title, track.artist, track.album))
}

fn track_key_if_same_album(track: &UiTrack, album_key_value: &str) -> Option<String> {
    (album_key(track) == album_key_value).then(|| track_key(track))
}

fn preferred_refresh_track_key(
    tracks: &[UiTrack],
    now_playing_key: Option<&str>,
    selected_key: Option<&str>,
) -> Option<String> {
    now_playing_key
        .filter(|key| tracks.iter().any(|track| track_key(track) == *key))
        .map(|key| key.to_string())
        .or_else(|| {
            selected_key
                .filter(|key| tracks.iter().any(|track| track_key(track) == *key))
                .map(|key| key.to_string())
        })
}

fn current_display_track(state: &UiState) -> Option<&UiTrack> {
    if state.playback_session.mode.is_radio() {
        return None;
    }

    state
        .playback_session
        .now_playing_key
        .as_deref()
        .and_then(|key| find_track_by_key(state, key))
        .or_else(|| state.tracks.get(state.selected_index))
}

fn find_track_by_key<'a>(state: &'a UiState, key: &str) -> Option<&'a UiTrack> {
    state
        .all_tracks
        .iter()
        .find(|track| track_key(track) == key)
        .or_else(|| state.tracks.iter().find(|track| track_key(track) == key))
}

fn compare_text(left: &str, right: &str) -> Ordering {
    left.to_lowercase().cmp(&right.to_lowercase())
}

fn compare_artist_album_track(left: &UiTrack, right: &UiTrack) -> Ordering {
    compare_text(&left.artist, &right.artist)
        .then_with(|| compare_text(&left.album, &right.album))
        .then_with(|| compare_optional_i32(left.disc_number, right.disc_number))
        .then_with(|| compare_optional_i32(left.track_number, right.track_number))
        .then_with(|| compare_text(&left.title, &right.title))
}

fn compare_album_track_order(left: &UiTrack, right: &UiTrack) -> Ordering {
    compare_optional_usize(left.album_position, right.album_position)
        .then_with(|| compare_optional_i32(left.disc_number, right.disc_number))
        .then_with(|| compare_optional_i32(left.track_number, right.track_number))
        .then_with(|| compare_text(&left.title, &right.title))
        .then_with(|| compare_text(&left.artist, &right.artist))
}

fn compare_optional_usize(left: Option<usize>, right: Option<usize>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.cmp(&right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn compare_optional_i32(left: Option<i32>, right: Option<i32>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.cmp(&right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn duration_seconds(duration: &str) -> i32 {
    let mut total = 0;
    for part in duration.split(':') {
        let Ok(value) = part.parse::<i32>() else {
            return 0;
        };
        total = total * 60 + value;
    }
    total
}

fn apply_connection_payload(state: &Rc<RefCell<UiState>>, payload: ConnectionPayload) {
    let (now_playing_key, selected_key) = {
        let ui = state.borrow();
        (
            ui.playback_session.now_playing_key.clone(),
            ui.tracks.get(ui.selected_index).map(track_key),
        )
    };

    {
        let mut ui = state.borrow_mut();
        ui.all_tracks = payload.tracks;
        ui.playlists = payload.playlists;
        rebuild_library_summaries(&mut ui);
        ui.jellyfin_connected = true;
        ui.active_page = LibraryPage::Tracks;
        ui.album_filter = None;
        ui.artist_filter = None;
        ui.playlist_filter = None;
        ui.collection_detail_title = None;
        ui.collection_detail_subtitle = None;
        ui.collection_detail_parent_search_query = None;
        ui.collection_return_target = None;
        ui.collection_parent_return_target = None;
        let selected_key = preferred_refresh_track_key(
            &ui.all_tracks,
            now_playing_key.as_deref(),
            selected_key.as_deref(),
        );
        let all_tracks = ui.all_tracks.clone();
        ui.playback_session
            .reconcile_library_refresh(&all_tracks, track_key);
        apply_track_filter(&mut ui, selected_key.as_deref());
        update_now_playing_labels(&ui);
        update_play_button(&ui);
        update_page_summary(&ui);
        ui.page_summary.set_text(&format!(
            "Jellyfin music library | {} tracks synced from {}",
            ui.all_tracks.len(),
            payload.session.server_url
        ));
        ui.connection_status.set_text("Connected to Jellyfin");
        ui.connection_detail.set_text(&format!(
            "{} | {} tracks cached",
            payload.session.username,
            ui.all_tracks.len()
        ));
        if let Some(button) = ui.refresh_button.as_ref() {
            button.set_sensitive(true);
        }
        if let Some(button) = ui.reconnect_button.as_ref() {
            button.set_visible(false);
            button.set_sensitive(false);
        }
    }
    refresh_track_model(state);
    update_nav_counts(state);
    update_content_view(state, NavDirection::DrillForward);
    load_selected_cover_art(state);
    load_selected_waveform(state);
    restore_persisted_playback(state);
}

fn update_now_playing_labels(state: &UiState) {
    if let Some(station) = current_radio_station(state) {
        state.now_title.set_text(&station.name);
        state
            .now_meta
            .set_text(&format!("{} | Radio stream", station.source_label()));
        state
            .playback_status
            .set_text(&radio_playback_status_text(state, &station));
        return;
    }

    if let Some(track) = current_display_track(state) {
        state.now_title.set_text(&track.title);
        state.now_meta.set_markup(&format!(
            "<a href=\"gtunes:artist\"><span underline=\"none\">{}</span></a> - <a href=\"gtunes:album\"><span underline=\"none\">{}</span></a>",
            gtk::glib::markup_escape_text(&track.artist),
            gtk::glib::markup_escape_text(&track.album)
        ));
        state
            .playback_status
            .set_text(&playback_status_text(state, track));
    } else {
        state.now_title.set_text("No track selected");
        if state.search_query.is_empty() {
            state.now_meta.set_text("Connect to Jellyfin to load music");
            state
                .playback_status
                .set_text("Jellyfin stream | Not playing");
        } else {
            state.now_meta.set_text("No search results");
            state.playback_status.set_text("Search returned no tracks");
        }
    }
}

fn radio_playback_status_text(state: &UiState, station: &RadioStation) -> String {
    match state.playback.as_ref().map(PlaybackEngine::state) {
        Some(PlaybackState::Playing) => format!("Playing radio | {}", station.name),
        Some(PlaybackState::Paused) => format!("Paused radio | {}", station.name),
        Some(PlaybackState::Error(error)) => format!("Radio stream failed: {error}"),
        _ => format!("Radio stream | {}", station.name),
    }
}

fn playback_status_text(state: &UiState, track: &UiTrack) -> String {
    match state.playback.as_ref().map(PlaybackEngine::state) {
        Some(PlaybackState::Playing) => {
            if state
                .playback
                .as_ref()
                .and_then(|playback| playback.current_stream_kind())
                == Some(PlaybackStreamKind::Transcode)
            {
                format!("Playing transcoded stream | {}", track.quality)
            } else {
                format!("Playing | {}", track.quality)
            }
        }
        Some(PlaybackState::Paused) => "Paused".to_string(),
        Some(PlaybackState::Error(error)) => format!("Playback failed: {error}"),
        _ if track.stream_url.is_some() => format!("Ready to stream | {}", track.quality),
        _ => "Track is missing a Jellyfin stream URL".to_string(),
    }
}

fn update_play_button(state: &UiState) {
    let Some(button) = state.play_button.as_ref() else {
        return;
    };

    let is_buffering = state
        .playback
        .as_ref()
        .map(PlaybackEngine::is_buffering)
        .unwrap_or(false);

    if let Some(spinner) = state.loading_spinner.as_ref() {
        if is_buffering {
            spinner.set_visible(true);
            spinner.start();
            button.add_css_class("play-button-loading");
        } else {
            spinner.stop();
            spinner.set_visible(false);
            button.remove_css_class("play-button-loading");
        }
    }

    match state.playback.as_ref().map(PlaybackEngine::state) {
        Some(PlaybackState::Playing) => {
            button.set_icon_name("media-playback-pause-symbolic");
            button.set_tooltip_text(Some("Pause"));
        }
        _ => {
            button.set_icon_name("media-playback-start-symbolic");
            button.set_tooltip_text(Some("Play"));
        }
    }
}

fn update_shuffle_button(state: &UiState) {
    let Some(button) = state.shuffle_button.as_ref() else {
        return;
    };

    if state.playback_session.shuffle_enabled {
        button.add_css_class("suggested-action");
        button.add_css_class("shuffle-on");
        button.remove_css_class("shuffle-off");
        button.set_tooltip_text(Some("Shuffle on"));
    } else {
        button.remove_css_class("suggested-action");
        button.remove_css_class("shuffle-on");
        button.add_css_class("shuffle-off");
        button.set_tooltip_text(Some("Shuffle"));
    }
}

fn toggle_shuffle(state: &Rc<RefCell<UiState>>) {
    {
        let mut ui = state.borrow_mut();
        let track_count = ui.tracks.len();
        let selected_index = ui.selected_index;
        ui.playback_session
            .toggle_shuffle(track_count, selected_index);
        arm_gapless_next(&mut ui);
        save_playback_snapshot_now(&mut ui);
        update_shuffle_button(&ui);
    }
    rebuild_queue_list(state);
}

fn pause_playback(state: &Rc<RefCell<UiState>>) {
    let mut ui = state.borrow_mut();
    let radio_is_active = ui.playback_session.mode.is_radio();

    if let Some(playback) = ui.playback.as_mut() {
        let result = if radio_is_active {
            playback.pause_live_stream()
        } else {
            playback.pause()
        };
        match result {
            Ok(()) => {
                save_playback_snapshot_now(&mut ui);
                update_now_playing_labels(&ui);
            }
            Err(error) => ui
                .playback_status
                .set_text(&format!("Pause failed: {error}")),
        }
        update_play_button(&ui);
        sync_external_playback_status(&mut ui);
    }
}

fn resume_playback(state: &Rc<RefCell<UiState>>) {
    let mut ui = state.borrow_mut();

    match ui.playback.as_ref().map(PlaybackEngine::state).cloned() {
        Some(PlaybackState::Paused) => {
            if ui.playback_session.mode.is_radio() {
                drop(ui);
                if !resume_radio_station(state) {
                    let mut ui = state.borrow_mut();
                    ui.playback_session.leave_radio_mode();
                    ui.playback_status.set_text("Radio station is unavailable");
                    update_play_button(&ui);
                    sync_external_playback_status(&mut ui);
                }
                return;
            }
            let result = ui.playback.as_mut().expect("playback was present").resume();
            match result {
                Ok(()) => {
                    save_playback_snapshot_now(&mut ui);
                    update_now_playing_labels(&ui);
                }
                Err(error) => ui
                    .playback_status
                    .set_text(&format!("Resume failed: {error}")),
            }
            update_play_button(&ui);
            sync_external_playback_status(&mut ui);
        }
        Some(PlaybackState::Playing) => {}
        _ => {
            drop(ui);
            play_track_at_selected_index(state);
        }
    }
}

fn toggle_play_pause(state: &Rc<RefCell<UiState>>) {
    let ui = state.borrow_mut();

    match ui.playback.as_ref().map(PlaybackEngine::state).cloned() {
        Some(PlaybackState::Playing) => {
            drop(ui);
            pause_playback(state);
        }
        Some(PlaybackState::Paused) => {
            drop(ui);
            resume_playback(state);
        }
        _ => {
            drop(ui);
            play_track_at_selected_index(state);
        }
    }
}

fn play_track_at_selected_index(state: &Rc<RefCell<UiState>>) {
    let index = state.borrow().selected_index;
    play_track_at(state, index);
}

fn play_previous_track(state: &Rc<RefCell<UiState>>) {
    let previous_index = {
        let ui = state.borrow();
        if ui.playback_session.mode.is_radio() {
            return;
        }
        previous_playback_index(&ui)
            .unwrap_or_else(|| ui.playback_session.current_index_or(ui.selected_index))
    };
    play_track_at_existing_order(state, previous_index);
}

fn play_next_track(state: &Rc<RefCell<UiState>>) {
    let next_index = {
        let ui = state.borrow();
        if ui.playback_session.mode.is_radio() {
            return;
        }
        next_playback_index(&ui)
            .unwrap_or_else(|| ui.playback_session.current_index_or(ui.selected_index))
    };
    play_track_at_existing_order(state, next_index);
}

fn play_track_at(state: &Rc<RefCell<UiState>>, index: usize) {
    play_track_at_with_order(state, index, true);
}

fn play_track_at_existing_order(state: &Rc<RefCell<UiState>>, index: usize) {
    play_track_at_with_order(state, index, false);
}

fn play_track_at_with_order(state: &Rc<RefCell<UiState>>, index: usize, rebuild_order: bool) {
    let (selected_index, visible_index) = {
        let mut ui = state.borrow_mut();
        let tracks = ui.tracks.clone();
        let selection = ui.playback_session.select_library_track(
            &tracks,
            index,
            rebuild_order,
            |queued, visible| track_key(queued) == track_key(visible),
        );
        ui.selected_index = selection.selected_index;

        update_now_playing_labels(&ui);
        update_play_button(&ui);
        (ui.selected_index, selection.visible_index)
    };
    if visible_index.is_some() {
        select_track_model_row(state, selected_index);
    }
    rebuild_queue_list(state);
    load_selected_cover_art(state);
    load_selected_waveform(state);
    play_selected_track(state);
}

fn select_track_model_row(state: &Rc<RefCell<UiState>>, index: usize) {
    let (selection, track_count) = {
        let ui = state.borrow();
        (ui.track_selection.clone(), ui.tracks.len())
    };
    if let Some(selection) = selection
        && track_count > 0
        && selection.selected() != index as u32
    {
        selection.set_selected(index.min(track_count - 1) as u32);
    }
}

fn play_selected_track(state: &Rc<RefCell<UiState>>) {
    let mut ui = state.borrow_mut();
    let Some(track) = ui
        .playback_session
        .queue_index
        .and_then(|index| ui.playback_session.queue_tracks.get(index))
        .cloned()
    else {
        stop_playback(&mut ui);
        ui.playback_status.set_text("No track selected");
        update_play_button(&ui);
        drop(ui);
        refresh_track_model(state);
        return;
    };
    let Some(stream_url) = track.stream_url.as_deref() else {
        stop_playback(&mut ui);
        ui.playback_status
            .set_text("Connect to Jellyfin before playback");
        update_play_button(&ui);
        drop(ui);
        refresh_track_model(state);
        return;
    };
    let Ok(stream_url) = stream_url.parse() else {
        stop_playback(&mut ui);
        ui.playback_status.set_text("Invalid Jellyfin stream URL");
        update_play_button(&ui);
        drop(ui);
        refresh_track_model(state);
        return;
    };
    let Some(playback) = ui.playback.as_mut() else {
        ui.playback_session.clear_now_playing();
        ui.playback_status
            .set_text("GStreamer playbin is unavailable");
        update_play_button(&ui);
        drop(ui);
        refresh_track_model(state);
        return;
    };

    let request = PlaybackRequest {
        item_id: track.item_id.clone().unwrap_or_default(),
        stream_url,
        http_headers: track.stream_http_headers.clone(),
        stream_kind: PlaybackStreamKind::Direct,
        title: track.title.clone(),
    };
    let mut refresh_now_playing = false;
    match playback.play(request) {
        Ok(()) => {
            ui.playback_session
                .start_library_playback(track_key(&track));
            arm_gapless_next(&mut ui);
            save_playback_snapshot_now(&mut ui);
            update_now_playing_labels(&ui);
            ui.playback_status
                .set_text(&format!("Playing | {}", track.quality));
            sync_external_playback(&mut ui);
            refresh_now_playing = true;
        }
        Err(error) => {
            ui.playback_session.clear_now_playing();
            ui.playback_status
                .set_text(&format!("Playback failed: {error}"));
            sync_external_playback_status(&mut ui);
        }
    }
    update_play_button(&ui);
    drop(ui);
    update_list_indicators(state);
    refresh_radio_page(state);
    if refresh_now_playing {
        load_selected_cover_art(state);
        load_selected_waveform(state);
    }
}

fn playback_request_for_track(track: &UiTrack) -> Option<PlaybackRequest> {
    playback_request_for_track_kind(track, PlaybackStreamKind::Direct)
}

fn playback_request_for_track_kind(
    track: &UiTrack,
    stream_kind: PlaybackStreamKind,
) -> Option<PlaybackRequest> {
    let stream_url = match stream_kind {
        PlaybackStreamKind::Direct => track.stream_url.as_deref(),
        PlaybackStreamKind::Transcode => track.fallback_stream_url.as_deref(),
    }?;

    Some(PlaybackRequest {
        item_id: track.item_id.clone().unwrap_or_default(),
        stream_url: stream_url.parse().ok()?,
        http_headers: track.stream_http_headers.clone(),
        stream_kind,
        title: track.title.clone(),
    })
}

fn next_gapless_request(ui: &UiState) -> Option<PlaybackRequest> {
    let next_index = next_playback_index(ui)?;
    playback_request_for_track(ui.playback_session.queue_tracks.get(next_index)?)
}

fn arm_gapless_next(ui: &mut UiState) {
    let request = next_gapless_request(ui);
    if let Some(playback) = ui.playback.as_mut() {
        playback.set_next(request);
    }
}

fn stop_playback(ui: &mut UiState) {
    ui.playback_session.reset_to_library();
    if let Some(playback) = ui.playback.as_mut()
        && let Err(error) = playback.stop()
    {
        ui.playback_status
            .set_text(&format!("Stop failed: {error}"));
    }
    sync_external_playback_status(ui);
}

fn load_selected_cover_art(state: &Rc<RefCell<UiState>>) {
    let (url, cover) = {
        let ui = state.borrow();
        let url = current_display_track(&ui).and_then(|track| track.thumbnail_artwork_url.clone());
        let cover = ui.cover_art.clone();
        (url, cover)
    };

    let Some(url) = url else {
        if let Some(cover) = cover.as_ref() {
            cover.set_paintable(Option::<&gtk::gdk::Paintable>::None);
        }
        return;
    };
    let Some(cover) = cover else {
        return;
    };

    let (sender, receiver) = mpsc::channel();
    let request_url = url.clone();
    std::thread::spawn(move || {
        let result = fetch_cached_image_file(&request_url).map(|path| (request_url, path));
        let _ = sender.send(result);
    });

    let state = state.clone();
    gtk::glib::timeout_add_local(Duration::from_millis(100), move || {
        match receiver.try_recv() {
            Ok(Ok((loaded_url, path))) => {
                let displayed_url = {
                    let ui = state.borrow();
                    current_display_track(&ui)
                        .and_then(|track| track.thumbnail_artwork_url.as_deref())
                        .map(str::to_string)
                };

                if displayed_url.as_deref() == Some(loaded_url.as_str()) {
                    let mut ui = state.borrow_mut();
                    sync_external_playback_metadata(&mut ui);
                    let file = gtk::gio::File::for_path(path);
                    match gtk::gdk::Texture::from_file(&file) {
                        Ok(texture) => cover.set_paintable(Some(&texture)),
                        Err(error) => tracing::warn!(%error, "failed to decode album artwork"),
                    }
                }
                gtk::glib::ControlFlow::Break
            }
            Ok(Err(error)) => {
                tracing::warn!(%error, "failed to fetch album artwork");
                gtk::glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => gtk::glib::ControlFlow::Break,
        }
    });
}

fn show_full_size_artwork(state: &Rc<RefCell<UiState>>) {
    let (title, full_url, paintable) = {
        let ui = state.borrow();
        let track = current_display_track(&ui);
        let title = track
            .map(|track| track.title.clone())
            .unwrap_or_else(|| config::APP_NAME.to_string());
        let full_url = track.and_then(|track| track.artwork_url.clone());
        let paintable = ui.cover_art.as_ref().and_then(gtk::Image::paintable);
        (title, full_url, paintable)
    };

    if paintable.is_none() && full_url.is_none() {
        return;
    }

    let picture = gtk::Picture::new();
    if let Some(paintable) = paintable.as_ref() {
        picture.set_paintable(Some(paintable));
    }
    picture.set_can_shrink(true);

    let window = gtk::Window::builder()
        .title(title)
        .decorated(false)
        .default_width(720)
        .default_height(720)
        .build();
    window.set_child(Some(&picture));
    window.present();

    if let Some(full_url) = full_url {
        load_full_size_artwork(full_url, picture);
    }
}

fn load_full_size_artwork(url: String, picture: gtk::Picture) {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = fetch_cached_image_file(&url);
        let _ = sender.send(result);
    });

    gtk::glib::timeout_add_local(Duration::from_millis(100), move || {
        match receiver.try_recv() {
            Ok(Ok(path)) => {
                let file = gtk::gio::File::for_path(path);
                match gtk::gdk::Texture::from_file(&file) {
                    Ok(texture) => picture.set_paintable(Some(&texture)),
                    Err(error) => tracing::warn!(%error, "failed to decode full-size artwork"),
                }
                gtk::glib::ControlFlow::Break
            }
            Ok(Err(error)) => {
                tracing::warn!(%error, "failed to fetch full-size artwork");
                gtk::glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => gtk::glib::ControlFlow::Break,
        }
    });
}

fn fetch_image_file(url: &str) -> Result<PathBuf, ImageFetchError> {
    let response = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|_| ImageFetchError::Request("request client failed"))?
        .get(url)
        .send()
        .map_err(|_| ImageFetchError::Request("request failed"))?;
    let status = response.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err(ImageFetchError::Missing);
    }
    if !status.is_success() {
        return Err(ImageFetchError::HttpStatus(status));
    }

    let bytes = response
        .bytes()
        .map_err(|_| ImageFetchError::Request("failed to read response body"))?;

    let path = artwork_cache_path(url);
    std::fs::write(&path, bytes).map_err(ImageFetchError::Io)?;
    Ok(path)
}

fn fetch_cached_image_file(url: &str) -> Result<PathBuf, ImageFetchError> {
    let path = artwork_cache_path(url);
    if path.exists() {
        Ok(path)
    } else {
        fetch_image_file(url)
    }
}

fn connect_and_fetch(
    server_url: &str,
    username: &str,
    password: &str,
    sender: mpsc::Sender<ConnectionMessage>,
    generation: u64,
) {
    let result = connect_and_fetch_payload(
        server_url,
        username,
        password,
        Some(sender.clone()),
        generation,
    );
    let _ = sender.send(ConnectionMessage::Finished(result));
}

fn reconnect_and_fetch(
    server_url: &str,
    username: &str,
    password: &str,
    sender: mpsc::Sender<ConnectionMessage>,
    generation: u64,
) {
    let result = reconnect_and_fetch_payload(
        server_url,
        username,
        password,
        Some(sender.clone()),
        generation,
    );
    let _ = sender.send(ConnectionMessage::Finished(result));
}

fn reconnect_and_fetch_payload(
    server_url: &str,
    username: &str,
    password: &str,
    sender: Option<mpsc::Sender<ConnectionMessage>>,
    generation: u64,
) -> Result<ConnectionPayload, String> {
    let (client, auth) = JellyfinClient::authenticate(server_url, username, password)
        .map_err(describe_jellyfin_error)?;
    let session = JellyfinSession {
        server_url: client.server_url().to_string(),
        server_id: auth.server_id,
        user_id: auth.user.id,
        username: auth.user.name,
        access_token: auth.access_token,
    };

    {
        let _guard = CACHE_RESET_LOCK
            .lock()
            .map_err(|_| "cache reset lock is poisoned".to_string())?;
        ensure_connection_generation_current(generation)?;
        let cache = CacheDatabase::open_default().map_err(|error| error.to_string())?;
        cache
            .save_jellyfin_session(&session)
            .map_err(|error| error.to_string())?;
    }

    if let Some(sender) = sender.as_ref() {
        let _ = sender.send(ConnectionMessage::Authenticated(session.clone()));
    }

    let payload = fetch_library_for_session(client, session, sender)?;
    {
        let _guard = CACHE_RESET_LOCK
            .lock()
            .map_err(|_| "cache reset lock is poisoned".to_string())?;
        ensure_connection_generation_current(generation)?;
        let cache = CacheDatabase::open_default().map_err(|error| error.to_string())?;
        if let Err(error) = save_library_cache(&cache, &payload.session, &payload) {
            tracing::warn!(%error, "failed to cache Jellyfin library");
        }
    }

    Ok(payload)
}

fn connect_and_fetch_payload(
    server_url: &str,
    username: &str,
    password: &str,
    sender: Option<mpsc::Sender<ConnectionMessage>>,
    generation: u64,
) -> Result<ConnectionPayload, String> {
    let (client, auth) = JellyfinClient::authenticate(server_url, username, password)
        .map_err(describe_jellyfin_error)?;
    let session = JellyfinSession {
        server_url: client.server_url().to_string(),
        server_id: auth.server_id,
        user_id: auth.user.id,
        username: auth.user.name,
        access_token: auth.access_token,
    };

    let payload = fetch_library_for_session(client, session, sender)?;
    {
        let _guard = CACHE_RESET_LOCK
            .lock()
            .map_err(|_| "cache reset lock is poisoned".to_string())?;
        ensure_connection_generation_current(generation)?;
        let cache = CacheDatabase::open_default().map_err(|error| error.to_string())?;
        cache
            .save_jellyfin_session(&payload.session)
            .map_err(|error| error.to_string())?;
        if let Err(error) = save_library_cache(&cache, &payload.session, &payload) {
            tracing::warn!(%error, "failed to cache Jellyfin library");
        }
    }

    Ok(payload)
}

fn fetch_saved_session(
    session: JellyfinSession,
    sender: mpsc::Sender<ConnectionMessage>,
    generation: u64,
) {
    let result = fetch_saved_session_payload(session, Some(sender.clone()), generation);
    let _ = sender.send(ConnectionMessage::Finished(result));
}

fn refresh_saved_session(sender: mpsc::Sender<ConnectionMessage>, generation: u64) {
    let result = refresh_saved_session_payload(Some(sender.clone()), generation);
    let _ = sender.send(ConnectionMessage::Finished(result));
}

fn refresh_saved_session_payload(
    sender: Option<mpsc::Sender<ConnectionMessage>>,
    generation: u64,
) -> Result<ConnectionPayload, String> {
    let cache = CacheDatabase::open_default().map_err(|error| error.to_string())?;
    let session = cache
        .load_jellyfin_session()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "No saved Jellyfin session. Connect to Jellyfin first.".to_string())?;

    let client = JellyfinClient::new(&session.server_url, Some(session.access_token.clone()))
        .map_err(describe_jellyfin_error)?;

    let cached_library = load_library_cache(&cache, &session)
        .map_err(|error| error.to_string())?
        .map(|mut library| {
            hydrate_cached_library(&mut library, &session);
            library
        })
        .filter(|library| {
            if library_needs_album_order_refresh(library) {
                send_connection_status(sender.as_ref(), "Refreshing album order metadata");
                false
            } else {
                true
            }
        });

    let payload = if let Some(cached_library) = cached_library {
        fetch_incremental_library_for_session(client, session, cached_library, sender)?
    } else {
        fetch_library_for_session(client, session, sender)?
    };
    {
        let _guard = CACHE_RESET_LOCK
            .lock()
            .map_err(|_| "cache reset lock is poisoned".to_string())?;
        ensure_connection_generation_current(generation)?;
        if let Err(error) = save_library_cache(&cache, &payload.session, &payload) {
            tracing::warn!(%error, "failed to cache Jellyfin library");
        }
    }
    Ok(payload)
}

fn fetch_saved_session_payload(
    session: JellyfinSession,
    sender: Option<mpsc::Sender<ConnectionMessage>>,
    generation: u64,
) -> Result<ConnectionPayload, String> {
    let cache = CacheDatabase::open_default().map_err(|error| error.to_string())?;
    match load_library_cache(&cache, &session) {
        Ok(Some(mut library)) => {
            hydrate_cached_library(&mut library, &session);
            if !library_needs_album_order_refresh(&library) {
                return Ok(ConnectionPayload {
                    session,
                    tracks: library.tracks,
                    playlists: library.playlists,
                });
            }
            send_connection_status(sender.as_ref(), "Refreshing album order metadata");
        }
        Ok(None) => {}
        Err(error) => tracing::warn!(%error, "failed to load cached Jellyfin library"),
    }

    let client = JellyfinClient::new(&session.server_url, Some(session.access_token.clone()))
        .map_err(describe_jellyfin_error)?;

    let payload = fetch_library_for_session(client, session, sender)?;
    {
        let _guard = CACHE_RESET_LOCK
            .lock()
            .map_err(|_| "cache reset lock is poisoned".to_string())?;
        ensure_connection_generation_current(generation)?;
        if let Err(error) = save_library_cache(&cache, &payload.session, &payload) {
            tracing::warn!(%error, "failed to cache Jellyfin library");
        }
    }
    Ok(payload)
}

fn ensure_connection_generation_current(generation: u64) -> Result<(), String> {
    if CONNECTION_GENERATION.load(AtomicOrdering::SeqCst) == generation {
        Ok(())
    } else {
        Err("Connection was reset".to_string())
    }
}

fn fetch_library_for_session(
    client: JellyfinClient,
    session: JellyfinSession,
    sender: Option<mpsc::Sender<ConnectionMessage>>,
) -> Result<ConnectionPayload, String> {
    let mut tracks = client
        .music_tracks_with_progress(&session.user_id, |loaded, total| {
            if let Some(sender) = sender.as_ref() {
                let _ = sender.send(ConnectionMessage::Progress { loaded, total });
            }
        })
        .map_err(describe_jellyfin_error)?
        .into_iter()
        .map(|track| UiTrack::from_jellyfin(track, &client))
        .collect::<Vec<_>>();
    assign_album_positions(&mut tracks);

    let playlists = client
        .music_playlists(&session.user_id)
        .map_err(describe_jellyfin_error)?
        .into_iter()
        .map(|playlist| {
            let playlist_tracks = client
                .playlist_tracks(&session.user_id, &playlist.id)
                .map_err(describe_jellyfin_error)?
                .into_iter()
                .map(|track| UiTrack::from_jellyfin(track, &client))
                .collect::<Vec<_>>();
            Ok(UiPlaylist::from_jellyfin(
                playlist,
                playlist_tracks,
                &client,
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;

    Ok(ConnectionPayload {
        session,
        tracks,
        playlists,
    })
}

fn fetch_incremental_library_for_session(
    client: JellyfinClient,
    session: JellyfinSession,
    cached_library: CachedLibrary,
    sender: Option<mpsc::Sender<ConnectionMessage>>,
) -> Result<ConnectionPayload, String> {
    send_connection_status(sender.as_ref(), "Scanning Jellyfin changes");
    let track_summaries = client
        .music_track_summaries_with_progress(&session.user_id, |loaded, total| {
            if let Some(sender) = sender.as_ref() {
                let _ = sender.send(ConnectionMessage::Progress { loaded, total });
            }
        })
        .map_err(describe_jellyfin_error)?;

    if summaries_missing_change_stamps(&track_summaries)
        || cached_library
            .tracks
            .iter()
            .any(|track| track.date_last_saved.is_none())
    {
        send_connection_status(sender.as_ref(), "Refreshing full library metadata");
        return fetch_library_for_session(client, session, sender);
    }

    let cached_tracks_by_id = cached_library
        .tracks
        .into_iter()
        .filter_map(|track| track.item_id.clone().map(|id| (id, track)))
        .collect::<HashMap<_, _>>();
    let changed_track_ids = changed_summary_ids(&track_summaries, &cached_tracks_by_id, |track| {
        track.date_last_saved.as_deref()
    });

    let mut fetched_tracks_by_id = HashMap::new();
    if !changed_track_ids.is_empty() {
        send_connection_status(
            sender.as_ref(),
            &format!("Fetching {} changed tracks", changed_track_ids.len()),
        );
    }
    for ids in changed_track_ids.chunks(100) {
        for track in client
            .music_tracks_by_ids(&session.user_id, ids)
            .map_err(describe_jellyfin_error)?
            .into_iter()
            .map(|track| UiTrack::from_jellyfin(track, &client))
        {
            if let Some(id) = track.item_id.clone() {
                fetched_tracks_by_id.insert(id, track);
            }
        }
    }

    let mut tracks = Vec::with_capacity(track_summaries.len());
    for summary in &track_summaries {
        if let Some(track) = fetched_tracks_by_id
            .remove(&summary.id)
            .or_else(|| cached_tracks_by_id.get(&summary.id).cloned())
        {
            tracks.push(track);
        }
    }
    assign_album_positions(&mut tracks);

    send_connection_status(sender.as_ref(), "Scanning Jellyfin playlists");
    let playlist_summaries = client
        .music_playlist_summaries(&session.user_id)
        .map_err(describe_jellyfin_error)?;
    if summaries_missing_change_stamps(&playlist_summaries)
        || cached_library
            .playlists
            .iter()
            .any(|playlist| playlist.date_last_saved.is_none())
    {
        send_connection_status(sender.as_ref(), "Refreshing playlist metadata");
        return fetch_library_for_session(client, session, sender);
    }

    let playlists = merge_incremental_playlists(
        &client,
        &session,
        &tracks,
        cached_library.playlists,
        playlist_summaries,
        sender.as_ref(),
    )?;

    Ok(ConnectionPayload {
        session,
        tracks,
        playlists,
    })
}

fn merge_incremental_playlists(
    client: &JellyfinClient,
    session: &JellyfinSession,
    tracks: &[UiTrack],
    cached_playlists: Vec<UiPlaylist>,
    playlist_summaries: Vec<JellyfinItemSummary>,
    sender: Option<&mpsc::Sender<ConnectionMessage>>,
) -> Result<Vec<UiPlaylist>, String> {
    let cached_playlists_by_id = cached_playlists
        .into_iter()
        .map(|playlist| (playlist.id.clone(), playlist))
        .collect::<HashMap<_, _>>();
    let changed_playlist_ids =
        changed_summary_ids(&playlist_summaries, &cached_playlists_by_id, |playlist| {
            playlist.date_last_saved.as_deref()
        });
    let changed_playlist_id_set = changed_playlist_ids.iter().cloned().collect::<HashSet<_>>();

    let all_playlists = client
        .music_playlists(&session.user_id)
        .map_err(describe_jellyfin_error)?;
    let playlist_models_by_id = all_playlists
        .into_iter()
        .map(|playlist| (playlist.id.clone(), playlist))
        .collect::<HashMap<_, _>>();

    if !changed_playlist_ids.is_empty() {
        send_connection_status(
            sender,
            &format!(
                "Refreshing {} changed playlists",
                changed_playlist_ids.len()
            ),
        );
    }

    let tracks_by_id = tracks
        .iter()
        .filter_map(|track| track.item_id.as_ref().map(|id| (id.as_str(), track)))
        .collect::<HashMap<_, _>>();
    let mut playlists = Vec::with_capacity(playlist_summaries.len());

    for summary in playlist_summaries {
        if changed_playlist_id_set.contains(&summary.id) {
            let Some(playlist) = playlist_models_by_id.get(&summary.id).cloned() else {
                continue;
            };
            let playlist_tracks = client
                .playlist_tracks(&session.user_id, &summary.id)
                .map_err(describe_jellyfin_error)?
                .into_iter()
                .map(|track| UiTrack::from_jellyfin(track, client))
                .collect::<Vec<_>>();
            playlists.push(UiPlaylist::from_jellyfin(playlist, playlist_tracks, client));
            continue;
        }

        if let Some(mut playlist) = cached_playlists_by_id.get(&summary.id).cloned() {
            playlist.tracks = playlist
                .tracks
                .into_iter()
                .filter_map(|track| {
                    track
                        .item_id
                        .as_deref()
                        .and_then(|id| tracks_by_id.get(id).copied())
                        .cloned()
                })
                .collect();
            playlists.push(playlist);
        }
    }

    Ok(playlists)
}

fn summaries_missing_change_stamps(summaries: &[JellyfinItemSummary]) -> bool {
    summaries
        .iter()
        .any(|summary| summary.date_last_saved.is_none())
}

fn changed_summary_ids<T>(
    summaries: &[JellyfinItemSummary],
    cached_by_id: &HashMap<String, T>,
    cached_stamp: impl Fn(&T) -> Option<&str>,
) -> Vec<String> {
    summaries
        .iter()
        .filter(|summary| {
            cached_by_id.get(&summary.id).and_then(&cached_stamp)
                != summary.date_last_saved.as_deref()
        })
        .map(|summary| summary.id.clone())
        .collect()
}

fn send_connection_status(sender: Option<&mpsc::Sender<ConnectionMessage>>, message: &str) {
    if let Some(sender) = sender {
        let _ = sender.send(ConnectionMessage::Status(message.to_string()));
    }
}

fn describe_jellyfin_error(error: JellyfinClientError) -> String {
    match &error {
        JellyfinClientError::Http(http_error) => {
            if matches!(
                http_error.status(),
                Some(reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN)
            ) {
                return "Jellyfin session expired or was revoked; enter your password to reconnect"
                    .to_string();
            }
            if http_error.is_timeout() {
                return "Jellyfin server timed out; check the server and try again".to_string();
            }
            if http_error.is_connect() {
                return "Jellyfin server is offline or unreachable; cached library data will remain available when present".to_string();
            }
        }
        JellyfinClientError::InvalidServerUrl(_) => {
            return "Invalid Jellyfin server URL".to_string();
        }
        JellyfinClientError::InvalidAuthHeader => {
            return "Saved Jellyfin token is invalid; enter your password to reconnect".to_string();
        }
    }

    error.to_string()
}

fn save_library_cache(
    cache: &CacheDatabase,
    session: &JellyfinSession,
    payload: &ConnectionPayload,
) -> Result<(), crate::cache::CacheError> {
    let json = serde_json::to_string(&CachedLibrary {
        tracks: payload.tracks.clone(),
        playlists: payload.playlists.clone(),
    })?;
    cache.set_setting(&library_cache_key(session), &json)
}

fn load_library_cache(
    cache: &CacheDatabase,
    session: &JellyfinSession,
) -> Result<Option<CachedLibrary>, crate::cache::CacheError> {
    if let Some(library) = load_cached_library(cache, &library_cache_key(session))? {
        if !library.tracks.is_empty() || !library.playlists.is_empty() {
            return Ok(Some(library));
        }
        tracing::warn!("ignoring empty Jellyfin library cache");
    }

    for key in [
        legacy_library_cache_key_v3(session),
        legacy_library_cache_key_v2(session),
    ] {
        if let Some(tracks) = load_cached_tracks(cache, &key)? {
            if !tracks.is_empty() {
                return Ok(Some(CachedLibrary {
                    tracks,
                    playlists: Vec::new(),
                }));
            }
            tracing::warn!("ignoring empty legacy Jellyfin library cache");
        }
    }

    Ok(None)
}

fn load_cached_library(
    cache: &CacheDatabase,
    key: &str,
) -> Result<Option<CachedLibrary>, crate::cache::CacheError> {
    cache
        .get_setting(key)?
        .map(|json| serde_json::from_str(&json).map_err(crate::cache::CacheError::from))
        .transpose()
}

fn load_cached_tracks(
    cache: &CacheDatabase,
    key: &str,
) -> Result<Option<Vec<UiTrack>>, crate::cache::CacheError> {
    cache
        .get_setting(key)?
        .map(|json| serde_json::from_str(&json).map_err(crate::cache::CacheError::from))
        .transpose()
}

fn hydrate_stream_http_headers(tracks: &mut [UiTrack], session: &JellyfinSession) {
    let headers = stream_http_headers_for_token(Some(&session.access_token));
    for track in tracks {
        track.stream_http_headers = headers.clone();
    }
}

fn hydrate_cached_library(library: &mut CachedLibrary, session: &JellyfinSession) {
    hydrate_stream_http_headers(&mut library.tracks, session);
    hydrate_sidebar_cover_thumbnail_urls(&mut library.tracks);
    for playlist in &mut library.playlists {
        normalize_sidebar_cover_thumbnail_url(&mut playlist.thumbnail_artwork_url);
        hydrate_stream_http_headers(&mut playlist.tracks, session);
        hydrate_sidebar_cover_thumbnail_urls(&mut playlist.tracks);
    }
}

fn hydrate_sidebar_cover_thumbnail_urls(tracks: &mut [UiTrack]) {
    for track in tracks {
        normalize_sidebar_cover_thumbnail_url(&mut track.thumbnail_artwork_url);
    }
}

fn normalize_sidebar_cover_thumbnail_url(url: &mut Option<String>) {
    let Some(existing_url) = url.as_mut() else {
        return;
    };
    if let Some(normalized_url) =
        resized_jellyfin_image_url(existing_url, SIDEBAR_COVER_ART_IMAGE_SIZE)
    {
        *existing_url = normalized_url;
    }
}

fn resized_jellyfin_image_url(url: &str, max_size: u32) -> Option<String> {
    let Ok(mut parsed) = url::Url::parse(url) else {
        return None;
    };
    if !parsed.path().contains("/Images/") {
        return None;
    }

    let mut has_size_query = false;
    let retained_pairs = parsed
        .query_pairs()
        .filter_map(|(key, value)| match key.as_ref() {
            "maxWidth" | "maxHeight" | "quality" => {
                has_size_query = true;
                None
            }
            _ => Some((key.into_owned(), value.into_owned())),
        })
        .collect::<Vec<_>>();
    if !has_size_query {
        return None;
    }

    let max_size = max_size.to_string();
    {
        let mut query = parsed.query_pairs_mut();
        query.clear();
        for (key, value) in retained_pairs {
            query.append_pair(&key, &value);
        }
        query
            .append_pair("maxWidth", &max_size)
            .append_pair("maxHeight", &max_size)
            .append_pair("quality", "80");
    }
    Some(parsed.to_string())
}

fn library_needs_album_order_refresh(library: &CachedLibrary) -> bool {
    let mut album_order_metadata = HashMap::<String, (usize, bool)>::new();
    for track in &library.tracks {
        let (count, has_order_metadata) = album_order_metadata
            .entry(album_key(track))
            .or_insert((0, true));
        *count += 1;
        *has_order_metadata &= track.album_position.is_some()
            || track.disc_number.is_some()
            || track.track_number.is_some();
    }

    album_order_metadata
        .values()
        .any(|(count, has_order_metadata)| *count > 1 && !*has_order_metadata)
}

fn library_cache_key(session: &JellyfinSession) -> String {
    format!(
        "jellyfin.library.v4.{}.{}",
        library_cache_server_key(session),
        session.user_id
    )
}

fn legacy_library_cache_key_v3(session: &JellyfinSession) -> String {
    format!(
        "jellyfin.library.v3.{}.{}",
        library_cache_server_key(session),
        session.user_id
    )
}

fn legacy_library_cache_key_v2(session: &JellyfinSession) -> String {
    format!(
        "jellyfin.library.{}.{}",
        library_cache_server_key(session),
        session.user_id
    )
}

fn library_cache_server_key(session: &JellyfinSession) -> &str {
    session
        .server_id
        .as_deref()
        .unwrap_or(session.server_url.as_str())
}

const QUEUE_PREVIEW_LIMIT: usize = 15;

fn queue_card(state: Rc<RefCell<UiState>>) -> gtk::Box {
    let card = gtk::Box::new(Orientation::Vertical, 4);
    card.add_css_class("queue-card");
    card.set_halign(Align::Fill);
    card.set_hexpand(true);
    card.append(&next_up_link_button(state.clone()));

    let empty = label("Nothing up next", "meta");
    card.append(&empty);

    let scroll = gtk::ScrolledWindow::new();
    scroll.add_css_class("queue-scroll");
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_overlay_scrolling(true);
    scroll.set_propagate_natural_height(true);
    scroll.set_max_content_height(128);
    scroll.set_hexpand(true);

    let list = gtk::Box::new(Orientation::Vertical, 4);
    list.set_hexpand(true);
    scroll.set_child(Some(&list));

    let mut rows = Vec::with_capacity(QUEUE_PREVIEW_LIMIT);
    for _ in 0..QUEUE_PREVIEW_LIMIT {
        let button = gtk::Button::new();
        button.add_css_class("queue-row");
        button.add_css_class("flat");
        button.set_visible(false);
        button.set_halign(Align::Fill);
        button.set_hexpand(true);
        button.set_cursor_from_name(Some("pointer"));

        let row = gtk::Box::new(Orientation::Horizontal, 7);
        row.set_halign(Align::Fill);
        row.set_valign(Align::Center);
        row.set_hexpand(true);
        row.set_size_request(0, -1);

        let art = cover_art(28);
        art.set_icon_name(Some("audio-x-generic-symbolic"));
        art.set_valign(Align::Center);
        row.append(&art);

        let title = label("", "queue-title");
        title.set_single_line_mode(true);
        title.set_lines(1);
        title.set_width_chars(1);
        title.set_max_width_chars(22);
        title.set_halign(Align::Fill);
        title.set_hexpand(true);
        let artist = label("", "queue-artist");
        artist.set_single_line_mode(true);
        artist.set_lines(1);
        artist.set_width_chars(1);
        artist.set_max_width_chars(22);
        artist.set_halign(Align::Fill);
        artist.set_hexpand(true);
        let text = gtk::Box::new(Orientation::Vertical, 1);
        text.set_halign(Align::Fill);
        text.set_valign(Align::Center);
        text.set_hexpand(true);
        text.set_size_request(0, -1);
        text.append(&title);
        text.append(&artist);
        row.append(&text);
        button.set_child(Some(&row));

        let track_index = Rc::new(RefCell::new(None));
        let click_track_index = track_index.clone();
        let click_state = state.clone();
        button.connect_clicked(move |_| {
            let index = *click_track_index.borrow();
            if let Some(index) = index {
                play_track_at_existing_order(&click_state, index);
            }
        });
        {
            let gesture_track_index = track_index.clone();
            let gesture_state = state.clone();
            connect_play_next_gesture(&button, move || {
                let Some(index) = *gesture_track_index.borrow() else {
                    return false;
                };
                queue_existing_track_next(&gesture_state, index)
            });
        }

        list.append(&button);
        rows.push(QueueRow {
            button,
            art,
            title,
            artist,
            track_index,
            artwork_url: Rc::new(RefCell::new(None)),
        });
    }

    card.append(&scroll);
    state.borrow_mut().queue_view = Some(Rc::new(QueueView { empty, rows }));
    rebuild_queue_list(&state);

    card
}

fn rebuild_queue_list(state: &Rc<RefCell<UiState>>) {
    let (queue_view, upcoming) = {
        let ui = state.borrow();
        (ui.queue_view.clone(), queued_tracks(&ui))
    };
    if let Some(queue_view) = queue_view {
        queue_view.empty.set_visible(upcoming.is_empty());

        for (row, item) in queue_view.rows.iter().zip(
            upcoming
                .into_iter()
                .map(Some)
                .chain(std::iter::repeat(None)),
        ) {
            let Some((index, track)) = item else {
                row.button.set_visible(false);
                *row.track_index.borrow_mut() = None;
                *row.artwork_url.borrow_mut() = None;
                continue;
            };

            row.button.set_visible(true);
            row.button
                .set_tooltip_text(Some(&format!("Play {}", track.title)));
            *row.track_index.borrow_mut() = Some(index);
            row.title.set_text(&track.title);
            row.artist.set_text(&track.artist);

            if *row.artwork_url.borrow() != track.thumbnail_artwork_url {
                *row.artwork_url.borrow_mut() = track.thumbnail_artwork_url.clone();
                row.art.set_paintable(Option::<&gtk::gdk::Paintable>::None);
                row.art.set_icon_name(Some("audio-x-generic-symbolic"));
                load_queue_art(
                    track.thumbnail_artwork_url,
                    row.art.clone(),
                    row.artwork_url.clone(),
                );
            }
        }
    }

    rebuild_next_up_page(state);
}

fn rebuild_next_up_page(state: &Rc<RefCell<UiState>>) {
    let (next_up_view, upcoming) = {
        let ui = state.borrow();
        (ui.next_up_view.clone(), next_up_tracks(&ui))
    };
    let Some(next_up_view) = next_up_view else {
        return;
    };

    next_up_view.empty.set_visible(upcoming.is_empty());
    while let Some(child) = next_up_view.list.first_child() {
        next_up_view.list.remove(&child);
    }
    next_up_view.rows.borrow_mut().clear();

    for (position, (index, track)) in upcoming.into_iter().enumerate() {
        let row = next_up_row(
            state.clone(),
            position,
            index,
            track,
            next_up_view.rows.clone(),
        );
        next_up_view.list.append(&row);
        next_up_view.rows.borrow_mut().push(row);
    }
}

fn next_up_row(
    state: Rc<RefCell<UiState>>,
    position: usize,
    track_index: usize,
    track: UiTrack,
    rows: Rc<RefCell<Vec<gtk::Button>>>,
) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("flat");
    button.add_css_class("next-up-row");
    button.set_halign(Align::Fill);
    button.set_hexpand(true);
    button.set_cursor_from_name(Some("pointer"));
    button.set_tooltip_text(Some(&format!("Play {}", track.title)));

    let row = gtk::Box::new(Orientation::Horizontal, 14);
    row.set_halign(Align::Fill);
    row.set_hexpand(true);
    row.set_valign(Align::Center);

    let index_label = label(&(position + 1).to_string(), "next-up-index");
    index_label.set_xalign(0.5);
    index_label.set_valign(Align::Center);
    row.append(&index_label);

    let art = cover_art(48);
    art.add_css_class("next-up-art");
    art.set_icon_name(Some("audio-x-generic-symbolic"));
    art.set_valign(Align::Center);
    row.append(&art);

    let text = gtk::Box::new(Orientation::Vertical, 2);
    text.add_css_class("next-up-text");
    text.set_halign(Align::Fill);
    text.set_valign(Align::Center);
    text.set_hexpand(true);
    let title = label(&track.title, "next-up-title");
    title.set_single_line_mode(true);
    title.set_lines(1);
    title.set_valign(Align::End);
    let artist = label(&track.artist, "next-up-artist");
    artist.set_single_line_mode(true);
    artist.set_lines(1);
    artist.set_valign(Align::Start);
    text.append(&title);
    text.append(&artist);
    row.append(&text);

    let trailing = gtk::Box::new(Orientation::Horizontal, 12);
    trailing.add_css_class("next-up-trailing");
    trailing.set_valign(Align::Center);

    let duration = label(&track.duration, "meta");
    duration.add_css_class("next-up-time");
    duration.add_css_class("mono");
    duration.set_xalign(1.0);
    duration.set_valign(Align::Center);
    trailing.append(&duration);

    let handle = gtk::Box::new(Orientation::Horizontal, 0);
    handle.add_css_class("next-up-handle");
    handle.set_valign(Align::Center);
    handle.append(&gtk::Image::from_icon_name("list-drag-handle-symbolic"));
    trailing.append(&handle);

    row.append(&trailing);

    button.set_child(Some(&row));
    {
        let state = state.clone();
        button.connect_clicked(move |_| {
            play_track_at_existing_order(&state, track_index);
        });
    }
    {
        let state = state.clone();
        connect_play_next_gesture(&button, move || {
            queue_existing_track_next(&state, track_index)
        });
    }

    let artwork_url = Rc::new(RefCell::new(track.thumbnail_artwork_url.clone()));
    load_queue_art(
        track.thumbnail_artwork_url.clone(),
        art.clone(),
        artwork_url.clone(),
    );

    let drag_source = gtk::DragSource::new();
    drag_source.set_actions(gtk::gdk::DragAction::MOVE);
    drag_source.connect_prepare(move |_, _, _| {
        Some(gtk::gdk::ContentProvider::for_value(
            &(position as u32).to_value(),
        ))
    });
    {
        let button = button.clone();
        drag_source.connect_drag_begin(move |_, _| {
            button.add_css_class("dragging");
        });
    }
    {
        let button = button.clone();
        let rows_end = rows.clone();
        drag_source.connect_drag_end(move |_, _, _| {
            button.remove_css_class("dragging");
            for row in rows_end.borrow().iter() {
                row.remove_css_class("dodge-up");
                row.remove_css_class("dodge-down");
            }
        });
    }
    button.add_controller(drag_source);

    let drop_target = gtk::DropTarget::new(gtk::glib::Type::U32, gtk::gdk::DragAction::MOVE);
    {
        let button = button.clone();
        let rows_motion = rows.clone();
        drop_target.connect_motion(move |_, _, y| {
            let is_drop_after = y >= f64::from(button.height()) / 2.0;
            button.remove_css_class("drop-before");
            button.remove_css_class("drop-after");
            if is_drop_after {
                button.add_css_class("drop-after");
            } else {
                button.add_css_class("drop-before");
            }
            let insert_at = position + usize::from(is_drop_after);
            let rows_ref = rows_motion.borrow();
            for (i, row) in rows_ref.iter().enumerate() {
                row.remove_css_class("dodge-up");
                row.remove_css_class("dodge-down");
                if i + 1 == insert_at {
                    row.add_css_class("dodge-up");
                } else if i == insert_at {
                    row.add_css_class("dodge-down");
                }
            }
            gtk::gdk::DragAction::MOVE
        });
    }
    {
        let button = button.clone();
        let rows_leave = rows.clone();
        drop_target.connect_leave(move |_| {
            button.remove_css_class("drop-before");
            button.remove_css_class("drop-after");
            for row in rows_leave.borrow().iter() {
                row.remove_css_class("dodge-up");
                row.remove_css_class("dodge-down");
            }
        });
    }
    {
        let state = state.clone();
        let button = button.clone();
        let rows_drop = rows.clone();
        drop_target.connect_drop(move |_, value, _, y| {
            button.remove_css_class("drop-before");
            button.remove_css_class("drop-after");
            for row in rows_drop.borrow().iter() {
                row.remove_css_class("dodge-up");
                row.remove_css_class("dodge-down");
            }
            let Ok(from) = value.get::<u32>() else {
                return false;
            };
            let to_slot = position + usize::from(y >= f64::from(button.height()) / 2.0);
            move_next_up_track(&state, from as usize, to_slot)
        });
    }
    button.add_controller(drop_target);

    button
}

fn load_queue_art(
    url: Option<String>,
    image: gtk::Image,
    current_url: Rc<RefCell<Option<String>>>,
) {
    let Some(url) = url else {
        return;
    };

    let (sender, receiver) = mpsc::channel();
    let request_url = url.clone();
    std::thread::spawn(move || {
        let result = fetch_cached_image_file(&request_url);
        let _ = sender.send(result);
    });

    gtk::glib::timeout_add_local(Duration::from_millis(100), move || {
        match receiver.try_recv() {
            Ok(Ok(path)) => {
                if current_url.borrow().as_deref() != Some(url.as_str()) {
                    return gtk::glib::ControlFlow::Break;
                }
                let file = gtk::gio::File::for_path(path);
                match gtk::gdk::Texture::from_file(&file) {
                    Ok(texture) => {
                        image.set_paintable(Some(&texture));
                        image.remove_css_class("artwork-loading");
                    }
                    Err(error) => {
                        tracing::warn!(%error, "failed to decode queue artwork");
                        image.remove_css_class("artwork-loading");
                    }
                }
                gtk::glib::ControlFlow::Break
            }
            Ok(Err(error)) => {
                tracing::warn!(%error, "failed to fetch queue artwork");
                image.remove_css_class("artwork-loading");
                gtk::glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => gtk::glib::ControlFlow::Break,
        }
    });
}

fn load_collection_queue_art(
    url: Option<String>,
    image: gtk::Image,
    current_url: Rc<RefCell<Option<String>>>,
    tile_index: usize,
) {
    gtk::glib::timeout_add_local_once(collection_artwork_delay(tile_index), move || {
        load_queue_art(url, image, current_url);
    });
}

fn load_collection_picture_art(url: String, image: gtk::Image, tile_index: usize) {
    gtk::glib::timeout_add_local_once(collection_artwork_delay(tile_index), move || {
        load_picture_art(url, image);
    });
}

fn collection_artwork_delay(tile_index: usize) -> Duration {
    let stagger_index = tile_index.min(COLLECTION_ARTWORK_MAX_STAGGERED_ITEMS) as u64;
    Duration::from_millis(
        COLLECTION_ARTWORK_INITIAL_DELAY_MS + (stagger_index * COLLECTION_ARTWORK_STAGGER_MS),
    )
}

fn load_picture_art(url: String, image: gtk::Image) {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = fetch_cached_image_file(&url);
        let _ = sender.send(result);
    });

    gtk::glib::timeout_add_local(Duration::from_millis(100), move || {
        match receiver.try_recv() {
            Ok(Ok(path)) => {
                let file = gtk::gio::File::for_path(path);
                match gtk::gdk::Texture::from_file(&file) {
                    Ok(texture) => {
                        image.remove_css_class("artist-placeholder");
                        image.remove_css_class("artwork-loading");
                        image.set_pixel_size(ARTIST_ART_SIZE);
                        image.set_paintable(Some(&texture));
                    }
                    Err(error) => {
                        tracing::warn!(%error, "failed to decode artist artwork");
                        image.remove_css_class("artwork-loading");
                    }
                }
                gtk::glib::ControlFlow::Break
            }
            Ok(Err(ImageFetchError::Missing)) => {
                tracing::debug!("artist artwork is unavailable");
                image.remove_css_class("artwork-loading");
                gtk::glib::ControlFlow::Break
            }
            Ok(Err(error)) => {
                tracing::warn!(%error, "failed to fetch artist artwork");
                image.remove_css_class("artwork-loading");
                gtk::glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => gtk::glib::ControlFlow::Break,
        }
    });
}

fn scroll_to_now_playing(state: &Rc<RefCell<UiState>>) {
    let needs_tracks_page = {
        let ui = state.borrow();
        ui.active_page != LibraryPage::Tracks
    };
    if needs_tracks_page {
        set_library_page(state, LibraryPage::Tracks);
        let state = state.clone();
        gtk::glib::idle_add_local(move || {
            scroll_to_now_playing(&state);
            gtk::glib::ControlFlow::Break
        });
        return;
    }

    let idx = {
        let ui = state.borrow();
        ui.tracks
            .iter()
            .position(|t| Some(track_key(t)) == ui.playback_session.now_playing_key)
    };

    let Some(idx) = idx else {
        return;
    };

    {
        let mut ui = state.borrow_mut();
        ui.selected_index = idx;
        update_now_playing_labels(&ui);
        update_play_button(&ui);
    }
    select_track_model_row(state, idx);
    rebuild_queue_list(state);
    load_selected_cover_art(state);
    load_selected_waveform(state);
    update_list_indicators(state);

    scroll_track_list_to_index(state, idx);
}

fn build_bottom_bar(state: Rc<RefCell<UiState>>) -> gtk::Box {
    let bar = gtk::Box::new(Orientation::Horizontal, 12);
    bar.add_css_class("bottom-bar");

    let spinner = gtk::Spinner::new();
    spinner.set_visible(false);
    state.borrow_mut().sync_spinner = Some(spinner.clone());
    bar.append(&spinner);
    bar.append(&state.borrow().connection_status);
    bar.append(&state.borrow().connection_detail);

    let spacer = gtk::Box::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    bar.append(&spacer);

    let reconnect = gtk::Button::with_label("Reconnect");
    reconnect.add_css_class("connection-button");
    reconnect.add_css_class("bottom-reconnect-button");
    reconnect.set_visible(false);
    reconnect.set_sensitive(false);
    reconnect.set_tooltip_text(Some("Reconnect to Jellyfin"));
    {
        let state = state.clone();
        reconnect.connect_clicked(move |button| {
            if let Some(window) = button
                .root()
                .and_then(|root| root.downcast::<gtk::Window>().ok())
            {
                show_reconnect_dialog(&window, state.clone());
            }
        });
    }
    state.borrow_mut().reconnect_button = Some(reconnect.clone());
    bar.append(&reconnect);

    bar
}

#[allow(deprecated)]
fn show_reconnect_dialog(parent: &gtk::Window, state: Rc<RefCell<UiState>>) {
    let session = match CacheDatabase::open_default().and_then(|db| db.load_jellyfin_session()) {
        Ok(Some(session)) => session,
        Ok(None) => {
            set_reconnect_button_needed(&state, false);
            let ui = state.borrow();
            ui.connection_status.set_text("Not connected");
            ui.connection_detail
                .set_text("Connect to Jellyfin before reconnecting");
            return;
        }
        Err(error) => {
            let message = error.to_string();
            let ui = state.borrow();
            ui.connection_status.set_text("Reconnect unavailable");
            ui.connection_detail.set_text(&message);
            return;
        }
    };

    let dialog = gtk::Dialog::builder()
        .transient_for(parent)
        .modal(true)
        .title("Reconnect to Jellyfin")
        .build();
    dialog.set_default_size(390, -1);
    dialog.add_button("Cancel", gtk::ResponseType::Cancel);
    dialog.add_button("Reconnect", gtk::ResponseType::Accept);
    dialog.set_default_response(gtk::ResponseType::Accept);

    let reconnect_button = dialog
        .widget_for_response(gtk::ResponseType::Accept)
        .and_then(|widget| widget.downcast::<gtk::Button>().ok());
    if let Some(button) = reconnect_button.as_ref() {
        button.add_css_class("suggested-action");
    }

    let content = dialog.content_area();
    content.add_css_class("reconnect-dialog-content");
    content.set_spacing(14);
    content.set_margin_top(18);
    content.set_margin_bottom(16);
    content.set_margin_start(18);
    content.set_margin_end(18);

    let summary = gtk::Box::new(Orientation::Vertical, 6);
    summary.add_css_class("reconnect-summary");
    summary.append(&reconnect_summary_row("Server", &session.server_url));
    summary.append(&reconnect_summary_row("User", &session.username));
    content.append(&summary);

    let password = gtk::PasswordEntry::new();
    password.set_placeholder_text(Some("Password"));
    password.set_activates_default(true);
    password.set_hexpand(true);
    password.add_css_class("reconnect-password");
    content.append(&password);

    let status = label("Enter your password to reconnect", "meta");
    status.add_css_class("reconnect-status");
    status.set_wrap(true);
    content.append(&status);

    let session_for_response = session.clone();
    dialog.connect_response(move |dialog, response| {
        if response != gtk::ResponseType::Accept {
            dialog.close();
            return;
        }

        let password_text = password.text().to_string();
        if password_text.is_empty() {
            status.set_text("Password is required");
            return;
        }

        if let Some(button) = reconnect_button.as_ref() {
            button.set_sensitive(false);
        }
        password.set_sensitive(false);
        status.set_text("Connecting...");
        set_library_loading(&state, "Reconnecting to Jellyfin");

        let generation = state.borrow().connection_generation;
        let server_url = session_for_response.server_url.clone();
        let username = session_for_response.username.clone();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            reconnect_and_fetch(&server_url, &username, &password_text, sender, generation);
        });

        poll_reconnect_result(
            receiver,
            state.clone(),
            status.clone(),
            password.clone(),
            reconnect_button.clone(),
            dialog.clone(),
            generation,
        );
    });

    dialog.present();
}

fn reconnect_summary_row(name: &str, value: &str) -> gtk::Box {
    let row = gtk::Box::new(Orientation::Horizontal, 10);
    row.add_css_class("reconnect-summary-row");

    let name = label(name, "reconnect-summary-name");
    name.set_width_chars(7);
    row.append(&name);

    let value = label(value, "reconnect-summary-value");
    value.set_hexpand(true);
    value.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    value.set_single_line_mode(true);
    value.set_lines(1);
    row.append(&value);

    row
}

#[allow(deprecated)]
fn poll_reconnect_result(
    receiver: mpsc::Receiver<ConnectionMessage>,
    state: Rc<RefCell<UiState>>,
    status: gtk::Label,
    password: gtk::PasswordEntry,
    button: Option<gtk::Button>,
    dialog: gtk::Dialog,
    generation: u64,
) {
    gtk::glib::timeout_add_local(Duration::from_millis(100), move || {
        if state.borrow().connection_generation != generation {
            if let Some(button) = button.as_ref() {
                button.set_sensitive(true);
            }
            password.set_sensitive(true);
            set_refresh_button_connected_state(&state);
            set_reconnect_button_needed(&state, false);
            set_library_loaded(&state);
            return gtk::glib::ControlFlow::Break;
        }

        match receiver.try_recv() {
            Ok(ConnectionMessage::Authenticated(session)) => {
                if let Some(button) = button.as_ref() {
                    button.set_sensitive(true);
                }
                password.set_sensitive(true);
                set_reconnect_button_needed(&state, false);
                {
                    let ui = state.borrow();
                    ui.connection_status.set_text("Connected to Jellyfin");
                    ui.connection_detail.set_text(&format!(
                        "{} | refreshing library in background",
                        session.username
                    ));
                    ui.page_summary
                        .set_text("Refreshing Jellyfin library in background");
                }
                dialog.close();
                gtk::glib::ControlFlow::Continue
            }
            Ok(ConnectionMessage::Status(message)) => {
                status.set_text(&message);
                let ui = state.borrow();
                ui.connection_status.set_text("Refreshing library");
                ui.connection_detail.set_text(&message);
                gtk::glib::ControlFlow::Continue
            }
            Ok(ConnectionMessage::Progress { loaded, total }) => {
                let progress = library_progress_text(loaded, total);
                status.set_text(&progress);
                let ui = state.borrow();
                ui.connection_status.set_text("Loading library");
                ui.connection_detail.set_text(&progress);
                gtk::glib::ControlFlow::Continue
            }
            Ok(ConnectionMessage::Finished(Ok(payload))) => {
                set_library_loaded(&state);
                apply_connection_payload(&state, payload);
                if let Some(card) = state.borrow().connection_card.as_ref() {
                    card.set_visible(false);
                }
                dialog.close();
                gtk::glib::ControlFlow::Break
            }
            Ok(ConnectionMessage::Finished(Err(error))) => {
                if let Some(button) = button.as_ref() {
                    button.set_sensitive(true);
                }
                password.set_sensitive(true);
                set_refresh_button_connected_state(&state);
                set_library_loaded(&state);
                set_reconnect_button_error_state(&state, &error);
                status.set_text(&error);
                let ui = state.borrow();
                ui.connection_status.set_text("Connection failed");
                ui.connection_detail.set_text(&error);
                gtk::glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => {
                if let Some(button) = button.as_ref() {
                    button.set_sensitive(true);
                }
                password.set_sensitive(true);
                set_refresh_button_connected_state(&state);
                set_reconnect_button_needed(&state, false);
                set_library_loaded(&state);
                status.set_text("Connection worker stopped");
                gtk::glib::ControlFlow::Break
            }
        }
    });
}

fn nav_list(state: Rc<RefCell<UiState>>) -> gtk::ListBox {
    let list = gtk::ListBox::new();
    list.add_css_class("nav-list");
    list.set_selection_mode(gtk::SelectionMode::Single);
    state.borrow_mut().nav_list = Some(list.clone());
    let rows = [
        (
            "audio-x-generic-symbolic",
            "Tracks",
            LibraryPage::Tracks,
            true,
        ),
        (
            "media-optical-cd-audio-symbolic",
            "Albums",
            LibraryPage::Albums,
            true,
        ),
        (
            "avatar-default-symbolic",
            "Artists",
            LibraryPage::Artists,
            true,
        ),
        (
            "media-playlist-consecutive-symbolic",
            "Playlists",
            LibraryPage::Playlists,
            true,
        ),
        (
            "network-wireless-symbolic",
            "Radio",
            LibraryPage::Radio,
            true,
        ),
    ];

    for (index, (icon, title, _page, enabled)) in rows.iter().enumerate() {
        let row = gtk::ListBoxRow::new();
        let line = gtk::Box::new(Orientation::Horizontal, 9);
        line.set_valign(Align::Center);
        line.append(&gtk::Image::from_icon_name(icon));
        line.append(&label(title, ""));
        let spacer = gtk::Box::new(Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        line.append(&spacer);
        let count = label("-", "count");
        match *title {
            "Tracks" => state.borrow_mut().nav_track_count = Some(count.clone()),
            "Albums" => state.borrow_mut().nav_album_count = Some(count.clone()),
            "Artists" => state.borrow_mut().nav_artist_count = Some(count.clone()),
            "Playlists" => state.borrow_mut().nav_playlist_count = Some(count.clone()),
            "Radio" => state.borrow_mut().nav_radio_count = Some(count.clone()),
            _ => count.set_text("-"),
        }
        line.append(&count);
        row.set_child(Some(&line));
        row.set_selectable(*enabled);
        row.set_activatable(*enabled);
        if !enabled {
            row.set_sensitive(false);
        }
        list.append(&row);
        if index == 0 {
            list.select_row(Some(&row));
        }
    }

    let nav_state = state.clone();
    list.connect_row_selected(move |_, row| {
        let Some(row) = row else {
            return;
        };
        match row.index() {
            0 => set_library_page(&nav_state, LibraryPage::Tracks),
            1 => set_library_page(&nav_state, LibraryPage::Albums),
            2 => set_library_page(&nav_state, LibraryPage::Artists),
            3 => set_library_page(&nav_state, LibraryPage::Playlists),
            4 => set_library_page(&nav_state, LibraryPage::Radio),
            _ => {}
        }
    });

    update_nav_counts(&state);

    list
}

fn label(text: &str, class_name: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_valign(Align::Center);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    if !class_name.is_empty() {
        label.add_css_class(class_name);
    }
    label
}

fn icon_button(icon_name: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::builder()
        .icon_name(icon_name)
        .tooltip_text(tooltip)
        .build();
    button.add_css_class("icon-button");
    button
}

fn next_up_link_button(state: Rc<RefCell<UiState>>) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("flat");
    button.add_css_class("queue-link");
    button.set_halign(Align::Fill);
    button.set_hexpand(true);
    button.set_cursor_from_name(Some("pointer"));
    button.set_tooltip_text(Some("Open the full Next Up queue"));

    let line = gtk::Box::new(Orientation::Horizontal, 8);
    line.set_hexpand(true);
    line.append(&label("Next Up", "rail-title"));
    let spacer = gtk::Box::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    line.append(&spacer);
    line.append(&gtk::Image::from_icon_name("go-next-symbolic"));
    button.set_child(Some(&line));

    button.connect_clicked(move |_| {
        set_library_page(&state, LibraryPage::NextUp);
    });
    button
}

fn cover_art(size: i32) -> gtk::Image {
    let art = gtk::Image::new();
    art.add_css_class("cover");
    art.set_size_request(size, size);
    art.set_pixel_size(size);
    art
}

fn start_playback_timer(state: &Rc<RefCell<UiState>>) {
    let state = state.clone();
    gtk::glib::timeout_add_local(Duration::from_millis(250), move || {
        update_playback_position(&state);
        gtk::glib::ControlFlow::Continue
    });
}

fn update_playback_position(state: &Rc<RefCell<UiState>>) {
    if apply_gapless_transition(state) {
        return;
    }

    if let Some(event) = take_playback_event(state) {
        handle_playback_event(state, event);
        return;
    }

    // Keep the loading spinner in sync with buffering state changes that don't
    // produce a PlaybackEvent (e.g. buffering percent updates mid-stream).
    {
        let ui = state.borrow();
        update_play_button(&ui);
    }

    let (position, duration, area, elapsed, remaining) = {
        let ui = state.borrow();
        let position = ui.playback.as_ref().and_then(PlaybackEngine::position);
        let duration = ui.playback.as_ref().and_then(PlaybackEngine::duration);
        (
            position,
            duration,
            ui.wave_area.clone(),
            ui.elapsed_label.clone(),
            ui.remaining_label.clone(),
        )
    };

    if let Some(position) = position {
        elapsed.set_text(&format_duration(position));
    }

    if let (Some(position), Some(duration)) = (position, duration) {
        let progress = if duration.is_zero() {
            0.0
        } else {
            position.as_secs_f64() / duration.as_secs_f64()
        }
        .clamp(0.0, 1.0);

        {
            let ui = state.borrow();
            ui.waveform.borrow_mut().progress = progress;
        }

        let remaining_time = duration.saturating_sub(position);
        remaining.set_text(&format!("-{}", format_duration(remaining_time)));
        if let Some(area) = area.as_ref() {
            area.queue_draw();
        }
    }

    if let Some(position) = position {
        // Once audio is actually flowing, clear the initial loading state.
        // This handles streams that never emit GStreamer buffering messages.
        if !position.is_zero() {
            let mut ui = state.borrow_mut();
            if let Some(playback) = ui.playback.as_mut() {
                if playback.is_buffering() {
                    playback.clear_initial_loading();
                }
            }
        }
        save_playback_snapshot_if_due(&mut state.borrow_mut());
    }
}

fn take_playback_event(state: &Rc<RefCell<UiState>>) -> Option<PlaybackEvent> {
    state
        .borrow_mut()
        .playback
        .as_mut()
        .and_then(PlaybackEngine::take_playback_event)
}

fn handle_playback_event(state: &Rc<RefCell<UiState>>, event: PlaybackEvent) {
    match event {
        PlaybackEvent::EndOfStream => advance_after_track_end(state),
        PlaybackEvent::Error {
            item_id,
            stream_kind,
            message,
        } => handle_playback_error(state, item_id, stream_kind, message),
    }
}

fn handle_playback_error(
    state: &Rc<RefCell<UiState>>,
    item_id: Option<String>,
    stream_kind: Option<PlaybackStreamKind>,
    message: String,
) {
    let fallback = {
        let ui = state.borrow();
        let fallback_position = ui.playback.as_ref().and_then(PlaybackEngine::position);
        let track = item_id
            .as_deref()
            .and_then(|item_id| {
                ui.tracks
                    .iter()
                    .find(|track| track.item_id.as_deref() == Some(item_id))
                    .or_else(|| {
                        ui.playback_session
                            .queue_tracks
                            .iter()
                            .find(|track| track.item_id.as_deref() == Some(item_id))
                    })
            })
            .or_else(|| current_display_track(&ui));

        if session::can_retry_with_transcode(stream_kind) {
            track.and_then(|track| {
                playback_request_for_track_kind(track, PlaybackStreamKind::Transcode).map(
                    |request| {
                        (
                            track_key(track),
                            track.quality.clone(),
                            request,
                            fallback_position,
                        )
                    },
                )
            })
        } else {
            None
        }
    };

    if let Some((track_key_value, quality, request, fallback_position)) = fallback {
        let mut ui = state.borrow_mut();
        ui.playback_status
            .set_text("Direct play failed; retrying with Jellyfin transcoding");
        if let Some(playback) = ui.playback.as_mut() {
            match playback.play(request) {
                Ok(()) => {
                    let seek_restore = fallback_position
                        .filter(|position| !position.is_zero())
                        .map(|position| match playback.seek(position) {
                            Ok(()) => session::FallbackSeekRestore::Restored(position),
                            Err(error) => session::FallbackSeekRestore::Failed {
                                position,
                                error: error.to_string(),
                            },
                        })
                        .unwrap_or(session::FallbackSeekRestore::NotNeeded);
                    ui.playback_session.start_library_playback(track_key_value);
                    arm_gapless_next(&mut ui);
                    save_playback_snapshot_now(&mut ui);
                    update_now_playing_labels(&ui);
                    ui.playback_status
                        .set_text(&session::fallback_playback_status(
                            &quality,
                            seek_restore,
                            format_duration,
                        ));
                    update_play_button(&ui);
                    sync_external_playback_status(&mut ui);
                    drop(ui);
                    update_list_indicators(state);
                    return;
                }
                Err(error) => {
                    ui.playback_status
                        .set_text(&format!("Transcode fallback failed: {error}"));
                }
            }
        }
    }

    {
        let mut ui = state.borrow_mut();
        if ui.playback_session.mode.is_radio() {
            ui.playback_status
                .set_text(&format!("Radio stream failed: {message}"));
        } else {
            ui.playback_status
                .set_text(&format!("Playback failed: {message}"));
        }
        ui.playback_session.clear_now_playing();
        arm_gapless_next(&mut ui);
        save_playback_snapshot_now(&mut ui);
        update_play_button(&ui);
        sync_external_playback(&mut ui);
    }
    update_list_indicators(state);
}

fn apply_gapless_transition(state: &Rc<RefCell<UiState>>) -> bool {
    let transition = {
        let mut ui = state.borrow_mut();
        ui.playback
            .as_mut()
            .and_then(PlaybackEngine::take_gapless_transition)
    };
    let Some(transition) = transition else {
        return false;
    };

    {
        let mut ui = state.borrow_mut();
        let item_ids_by_index = ui
            .playback_session
            .queue_tracks
            .iter()
            .map(|track| track.item_id.clone())
            .collect::<Vec<_>>();
        let selected_index = ui.selected_index;
        ui.playback_session.apply_gapless_transition(
            &item_ids_by_index,
            selected_index,
            &transition.item_id,
        );

        if let Some(track) = ui
            .playback_session
            .queue_index
            .and_then(|index| ui.playback_session.queue_tracks.get(index))
            .cloned()
        {
            let quality = track.quality.clone();
            ui.playback_session
                .start_library_playback(track_key(&track));
            if let Some(visible_index) = ui
                .tracks
                .iter()
                .position(|visible| track_key(visible) == track_key(&track))
            {
                ui.selected_index = visible_index;
            }
            update_now_playing_labels(&ui);
            ui.playback_status.set_text(&format!("Playing | {quality}"));
        } else {
            ui.playback_session.clear_now_playing();
            ui.playback_status.set_text("Playing next stream");
        }

        arm_gapless_next(&mut ui);
        save_playback_snapshot_now(&mut ui);
        update_play_button(&ui);
        sync_external_playback(&mut ui);
    }

    let selected_index = state.borrow().selected_index;
    select_track_model_row(state, selected_index);
    rebuild_queue_list(state);
    update_list_indicators(state);
    load_selected_cover_art(state);
    load_selected_waveform(state);
    true
}

fn advance_after_track_end(state: &Rc<RefCell<UiState>>) {
    let next_index = {
        let ui = state.borrow();
        next_playback_index(&ui)
    };

    if let Some(next_index) = next_index {
        play_track_at_existing_order(state, next_index);
    } else {
        {
            let mut ui = state.borrow_mut();
            let radio_was_active = ui.playback_session.finish_playback();
            if radio_was_active {
                ui.playback_status.set_text("Radio stream ended");
            } else {
                ui.playback_status.set_text("Up Next finished");
            }
            update_play_button(&ui);
            sync_external_playback_status(&mut ui);
            clear_playback_snapshot();
        }
        update_list_indicators(state);
        refresh_radio_page(state);
    }
}

fn load_selected_waveform(state: &Rc<RefCell<UiState>>) {
    let (key, stream_url, stream_http_headers, area, status, waveform) = {
        let ui = state.borrow();
        let track = current_display_track(&ui);
        let key = track.and_then(|track| {
            Some(WaveformKey {
                item_id: track.item_id.clone()?,
                media_source_id: track.media_source_id.clone()?,
            })
        });
        let stream_url = track.and_then(|track| track.stream_url.clone());
        let stream_http_headers = track
            .map(|track| track.stream_http_headers.clone())
            .unwrap_or_default();
        (
            key,
            stream_url,
            stream_http_headers,
            ui.wave_area.clone(),
            ui.waveform_status.clone(),
            ui.waveform.clone(),
        )
    };

    let Some(key) = key else {
        let mut visual = waveform.borrow_mut();
        visual.peaks.clear();
        visual.loaded_key = None;
        visual.loading_key = None;
        status.set_text("No track selected");
        if let Some(area) = area.as_ref() {
            area.queue_draw();
        }
        return;
    };
    let Some(stream_url) = stream_url else {
        let mut visual = waveform.borrow_mut();
        visual.peaks.clear();
        visual.loaded_key = None;
        visual.loading_key = None;
        status.set_text("No stream URL");
        if let Some(area) = area.as_ref() {
            area.queue_draw();
        }
        return;
    };

    {
        let mut visual = waveform.borrow_mut();
        if visual.loaded_key.as_ref() == Some(&key) || visual.loading_key.as_ref() == Some(&key) {
            return;
        }
        visual.peaks.clear();
        visual.progress = 0.0;
        visual.loaded_key = None;
        visual.loading_key = Some(key.clone());
    }
    status.set_text("Building waveform");
    if let Some(area) = area.as_ref() {
        area.queue_draw();
    }

    let (sender, receiver) = mpsc::channel();
    let request_key = key.clone();
    std::thread::spawn(move || {
        let result =
            crate::waveform::load_or_generate(request_key, &stream_url, &stream_http_headers);
        let _ = sender.send(result);
    });

    let state = state.clone();
    gtk::glib::timeout_add_local(Duration::from_millis(100), move || {
        match receiver.try_recv() {
            Ok(Ok(summary)) => {
                apply_waveform_summary(&state, summary);
                gtk::glib::ControlFlow::Break
            }
            Ok(Err(error)) => {
                let ui = state.borrow();
                ui.waveform.borrow_mut().loading_key = None;
                ui.waveform_status.set_text("Waveform failed");
                if let Some(area) = ui.wave_area.as_ref() {
                    area.queue_draw();
                }
                tracing::warn!(%error, "failed to load waveform");
                gtk::glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => gtk::glib::ControlFlow::Break,
        }
    });
}

fn apply_waveform_summary(state: &Rc<RefCell<UiState>>, summary: WaveformSummary) {
    let ui = state.borrow();
    {
        let mut visual = ui.waveform.borrow_mut();
        if visual.loading_key.as_ref() != Some(&summary.key) {
            return;
        }
        visual.peaks = summary.peaks;
        visual.loaded_key = Some(summary.key);
        visual.loading_key = None;
    }
    ui.waveform_status.set_text("Waveform loaded");
    if let Some(area) = ui.wave_area.as_ref() {
        area.queue_draw();
    }
}

fn seek_waveform(state: &Rc<RefCell<UiState>>, area: &gtk::DrawingArea, x: f64) {
    let width = area.allocated_width().max(1) as f64;
    let progress = (x / width).clamp(0.0, 1.0);
    let mut ui = state.borrow_mut();
    let Some(duration) = ui.playback.as_ref().and_then(PlaybackEngine::duration) else {
        ui.waveform.borrow_mut().progress = progress;
        if let Some(area) = ui.wave_area.as_ref() {
            area.queue_draw();
        }
        return;
    };
    let position = duration.mul_f64(progress);
    let result = ui
        .playback
        .as_mut()
        .expect("playback duration came from playback")
        .seek(position);
    match result {
        Ok(()) => {
            ui.waveform.borrow_mut().progress = progress;
            ui.elapsed_label.set_text(&format_duration(position));
            ui.remaining_label.set_text(&format!(
                "-{}",
                format_duration(duration.saturating_sub(position))
            ));
            save_playback_snapshot_now(&mut ui);
        }
        Err(error) => ui
            .playback_status
            .set_text(&format!("Seek failed: {error}")),
    }
    if let Some(area) = ui.wave_area.as_ref() {
        area.queue_draw();
    }
}

fn format_duration(duration: Duration) -> String {
    let total_seconds = duration.as_secs();
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{minutes}:{seconds:02}")
}

fn waveform_widget(state: Rc<RefCell<UiState>>) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_height(48);
    area.set_hexpand(true);
    area.set_tooltip_text(Some("Seek"));

    let draw_state = state.borrow().waveform.clone();
    area.set_draw_func(move |_, cr, width, height| {
        rounded_rect(cr, 0.0, 0.0, width as f64, height as f64, 6.0);
        cr.set_source_rgba(0.5, 0.5, 0.5, 0.10);
        let _ = cr.fill();

        let visual = draw_state.borrow();
        let samples = visual.peaks.as_slice();
        let progress = visual.progress;
        let progress_x = width as f64 * progress;
        let sample_count = samples.len().max(1);
        let step = width as f64 / sample_count as f64;
        let bar_width = (step - 1.0).max(1.0);
        let center = height as f64 / 2.0;
        let usable = height as f64 * 0.74;

        if samples.is_empty() {
            cr.set_source_rgba(0.5, 0.5, 0.5, 0.28);
            cr.set_line_width(1.0);
            cr.move_to(9.0, center);
            cr.line_to(width as f64 - 9.0, center);
            let _ = cr.stroke();
        }

        for (index, sample) in samples.iter().enumerate() {
            let x = index as f64 * step;
            let bar_height = (*sample as f64 * usable).max(3.0);
            if x <= progress_x {
                cr.set_source_rgb(0.16, 0.56, 0.68);
            } else {
                cr.set_source_rgba(0.5, 0.5, 0.5, 0.32);
            }
            rounded_rect(cr, x, center - bar_height / 2.0, bar_width, bar_height, 2.0);
            let _ = cr.fill();
        }

        cr.set_source_rgba(0.0, 0.0, 0.0, 0.75);
        cr.set_line_width(2.0);
        cr.move_to(progress_x + 0.5, 5.0);
        cr.line_to(progress_x + 0.5, height as f64 - 5.0);
        let _ = cr.stroke();
    });

    let click = gtk::GestureClick::new();
    {
        let state = state.clone();
        let area = area.clone();
        click.connect_pressed(move |_, _, x, _| {
            seek_waveform(&state, &area, x);
        });
    }
    area.add_controller(click);

    let drag = gtk::GestureDrag::new();
    let drag_origin = Rc::new(RefCell::new(0.0));
    {
        let drag_origin = drag_origin.clone();
        drag.connect_drag_begin(move |_, x, _| {
            *drag_origin.borrow_mut() = x;
        });
    }
    {
        let state = state.clone();
        let area = area.clone();
        drag.connect_drag_update(move |_, offset_x, _| {
            seek_waveform(&state, &area, *drag_origin.borrow() + offset_x);
        });
    }
    area.add_controller(drag);

    area
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;

    fn test_session() -> JellyfinSession {
        JellyfinSession {
            server_url: "https://jellyfin.example/".to_string(),
            server_id: Some("server-id".to_string()),
            user_id: "user-id".to_string(),
            username: "eddie".to_string(),
            access_token: "token".to_string(),
        }
    }

    fn test_track() -> UiTrack {
        UiTrack {
            item_id: Some("track-id".to_string()),
            date_last_saved: Some("2026-01-01T00:00:00.0000000Z".to_string()),
            album_id: Some("album-id".to_string()),
            media_source_id: Some("media-source-id".to_string()),
            stream_url: Some("https://jellyfin.example/Audio/track-id/stream".to_string()),
            fallback_stream_url: Some(
                "https://jellyfin.example/Audio/track-id/universal".to_string(),
            ),
            stream_http_headers: Vec::new(),
            artwork_url: None,
            thumbnail_artwork_url: None,
            title: "Song".to_string(),
            artist: "Artist".to_string(),
            album_artist: Some("Artist".to_string()),
            artist_images: Vec::new(),
            album: "Album".to_string(),
            disc_number: Some(1),
            track_number: Some(2),
            album_position: None,
            duration: "3:04".to_string(),
            quality: "FLAC".to_string(),
        }
    }

    fn test_track_with(
        item_id: &str,
        album_id: &str,
        title: &str,
        album: &str,
        artist: &str,
        album_artist: &str,
    ) -> UiTrack {
        UiTrack {
            item_id: Some(item_id.to_string()),
            album_id: Some(album_id.to_string()),
            date_last_saved: Some("2026-01-01T00:00:00.0000000Z".to_string()),
            title: title.to_string(),
            album: album.to_string(),
            artist: artist.to_string(),
            album_artist: Some(album_artist.to_string()),
            ..test_track()
        }
    }

    fn numbered_album_track(
        item_id: &str,
        title: &str,
        artist: &str,
        track_number: i32,
    ) -> UiTrack {
        UiTrack {
            item_id: Some(item_id.to_string()),
            title: title.to_string(),
            artist: artist.to_string(),
            album: "Mixed Artist Album".to_string(),
            album_artist: Some("Main Artist".to_string()),
            disc_number: Some(1),
            track_number: Some(track_number),
            ..test_track()
        }
    }

    fn positioned_album_track(
        item_id: &str,
        title: &str,
        artist: &str,
        album_position: usize,
    ) -> UiTrack {
        UiTrack {
            item_id: Some(item_id.to_string()),
            title: title.to_string(),
            artist: artist.to_string(),
            album: "Mixed Artist Album".to_string(),
            album_artist: Some("Main Artist".to_string()),
            disc_number: None,
            track_number: None,
            album_position: Some(album_position),
            ..test_track()
        }
    }

    #[test]
    fn invisible_search_prioritizes_leading_word_matches() {
        let soundtrack_track = UiTrack {
            title: "I Don't Give".to_string(),
            album: "American Wedding Original Soundtrack".to_string(),
            artist: "Avril Lavigne".to_string(),
            ..test_track()
        };
        let title_track = UiTrack {
            title: "American Idiot".to_string(),
            album: "American Idiot".to_string(),
            artist: "Green Day".to_string(),
            ..test_track()
        };

        assert!(
            invisible_track_search_rank(&title_track, "american")
                < invisible_track_search_rank(&soundtrack_track, "american")
        );
    }

    #[test]
    fn invisible_search_prioritizes_word_boundaries_over_substrings() {
        assert!(
            invisible_text_search_rank("Made in America", "america")
                < invisible_text_search_rank("Panamericana", "america")
        );
    }

    #[test]
    fn collection_summaries_group_tracks_and_count_artist_albums() {
        let tracks = vec![
            test_track_with("track-1", "album-1", "First", "Beta", "Guest", "Artist B"),
            test_track_with("track-2", "album-1", "Second", "Beta", "Guest", "Artist B"),
            test_track_with(
                "track-3", "album-2", "Third", "Alpha", "Artist A", "Artist A",
            ),
        ];

        let albums = album_summaries(&tracks, "");
        assert_eq!(albums.len(), 2);
        assert_eq!(albums[0].name, "Alpha");
        assert_eq!(albums[0].song_count, 1);
        assert_eq!(albums[1].name, "Beta");
        assert_eq!(albums[1].artist, "Artist B");
        assert_eq!(albums[1].song_count, 2);

        let artists = artist_summaries(&tracks, "");
        assert_eq!(artists.len(), 2);
        assert_eq!(artists[0].name, "Artist A");
        assert_eq!(artists[0].album_count, 1);
        assert_eq!(artists[0].song_count, 1);
        assert_eq!(artists[1].name, "Artist B");
        assert_eq!(artists[1].album_count, 1);
        assert_eq!(artists[1].song_count, 2);

        assert_eq!(
            artist_album_song_counts_from(&albums, &artist_key("Artist B"), ""),
            (1, 2)
        );
    }

    #[test]
    fn artist_summaries_merge_separator_variants() {
        let tracks = vec![
            test_track_with(
                "track-1",
                "album-1",
                "First",
                "Alpha",
                "Blink 182",
                "Blink 182",
            ),
            test_track_with(
                "track-2",
                "album-2",
                "Second",
                "Beta",
                "blink-182",
                "blink-182",
            ),
            test_track_with(
                "track-3",
                "album-2",
                "Third",
                "Beta",
                "blink-182",
                "blink-182",
            ),
            test_track_with(
                "track-4",
                "album-3",
                "Fourth",
                "Gamma",
                "blink\u{2010}182",
                "blink\u{2010}182",
            ),
        ];

        assert_eq!(artist_key("Blink 182"), artist_key("blink-182"));
        assert_eq!(artist_key("blink-182"), artist_key("blink\u{2010}182"));

        let artists = artist_summaries(&tracks, "");

        assert_eq!(artists.len(), 1);
        assert_eq!(artists[0].name, "blink-182");
        assert_eq!(artists[0].album_count, 3);
        assert_eq!(artists[0].song_count, 4);
    }

    #[test]
    fn album_track_sort_preserves_track_order_before_artist_order() {
        let mut tracks = vec![
            numbered_album_track("track-3", "Third", "Main Artist", 3),
            numbered_album_track("track-1", "First", "Main Artist", 1),
            numbered_album_track("track-2", "Second", "ZZ Guest", 2),
        ];
        let mut selected_index = 0;

        sort_track_slice(
            &mut tracks,
            SortColumn::Artist,
            true,
            "",
            true,
            None,
            &mut selected_index,
        );

        let track_ids = tracks
            .iter()
            .map(|track| track.item_id.as_deref())
            .collect::<Vec<_>>();
        assert_eq!(
            track_ids,
            vec![Some("track-1"), Some("track-2"), Some("track-3")]
        );
    }

    #[test]
    fn album_track_sort_preserves_source_album_position_without_track_numbers() {
        let mut tracks = vec![
            positioned_album_track("track-3", "Third", "Main Artist", 2),
            positioned_album_track("track-1", "First", "Main Artist", 0),
            positioned_album_track("track-2", "Second", "ZZ Guest", 1),
        ];
        let mut selected_index = 0;

        sort_track_slice(
            &mut tracks,
            SortColumn::Artist,
            true,
            "",
            true,
            None,
            &mut selected_index,
        );

        let track_ids = tracks
            .iter()
            .map(|track| track.item_id.as_deref())
            .collect::<Vec<_>>();
        assert_eq!(
            track_ids,
            vec![Some("track-1"), Some("track-2"), Some("track-3")]
        );
    }

    #[test]
    fn cached_library_without_album_order_metadata_requires_refresh() {
        let library = CachedLibrary {
            tracks: vec![
                UiTrack {
                    album: "Album".to_string(),
                    disc_number: None,
                    track_number: None,
                    album_position: None,
                    ..test_track()
                },
                UiTrack {
                    item_id: Some("track-2".to_string()),
                    album: "Album".to_string(),
                    disc_number: None,
                    track_number: None,
                    album_position: None,
                    ..test_track()
                },
            ],
            playlists: Vec::new(),
        };

        assert!(library_needs_album_order_refresh(&library));
    }

    #[test]
    fn refresh_prefers_currently_playing_track_when_it_still_exists() {
        let tracks = vec![
            test_track_with(
                "track-1", "album-1", "First", "Alpha", "Artist A", "Artist A",
            ),
            test_track_with(
                "track-2", "album-2", "Second", "Beta", "Artist B", "Artist B",
            ),
        ];

        let selected_key = preferred_refresh_track_key(&tracks, Some("track-2"), Some("track-1"));

        assert_eq!(selected_key.as_deref(), Some("track-2"));
    }

    #[test]
    fn refresh_falls_back_to_previous_selection_when_playing_track_is_missing() {
        let tracks = vec![
            test_track_with(
                "track-1", "album-1", "First", "Alpha", "Artist A", "Artist A",
            ),
            test_track_with(
                "track-2", "album-2", "Second", "Beta", "Artist B", "Artist B",
            ),
        ];

        let selected_key = preferred_refresh_track_key(&tracks, Some("missing"), Some("track-1"));

        assert_eq!(selected_key.as_deref(), Some("track-1"));
    }

    #[test]
    fn album_navigation_preserves_current_track_when_it_belongs_to_that_album() {
        let track = test_track_with(
            "track-2", "album-1", "Second", "Alpha", "Artist A", "Artist A",
        );

        let selected_key = track_key_if_same_album(&track, "album-1");

        assert_eq!(selected_key.as_deref(), Some("track-2"));
    }

    #[test]
    fn album_navigation_does_not_force_selection_for_other_albums() {
        let track = test_track_with(
            "track-2", "album-1", "Second", "Alpha", "Artist A", "Artist A",
        );

        let selected_key = track_key_if_same_album(&track, "album-2");

        assert!(selected_key.is_none());
    }

    #[test]
    fn queued_tracks_follow_playback_order_after_current_track() {
        let tracks = vec![
            test_track_with(
                "track-1", "album-1", "First", "Alpha", "Artist A", "Artist A",
            ),
            test_track_with(
                "track-2", "album-1", "Second", "Alpha", "Artist A", "Artist A",
            ),
            test_track_with(
                "track-3", "album-1", "Third", "Alpha", "Artist A", "Artist A",
            ),
        ];

        let queued = queued_tracks_from_order(&tracks, &[0, 1, 2], 1);

        let queued_ids = queued
            .iter()
            .map(|(_, track)| track.item_id.as_deref())
            .collect::<Vec<_>>();
        assert_eq!(queued_ids, vec![Some("track-3")]);
    }

    #[test]
    fn persisted_playback_restore_uses_current_library_tracks() {
        let first = UiTrack {
            item_id: Some("first".to_string()),
            title: "First".to_string(),
            ..test_track()
        };
        let second = UiTrack {
            item_id: Some("second".to_string()),
            title: "Second".to_string(),
            ..test_track()
        };
        let third = UiTrack {
            item_id: Some("third".to_string()),
            title: "Third".to_string(),
            ..test_track()
        };
        let snapshot = session::PersistedPlaybackState {
            version: session::PLAYBACK_STATE_VERSION,
            current_item_id: "second".to_string(),
            ordered_item_ids: vec![
                "first".to_string(),
                "missing".to_string(),
                "second".to_string(),
                "third".to_string(),
            ],
            position_secs: 42,
            shuffle_enabled: true,
        };

        let (tracks, index, order) =
            restore_playback_snapshot_tracks(&[first, second, third], &snapshot)
                .expect("snapshot restores");

        let restored_ids = tracks
            .iter()
            .filter_map(|track| track.item_id.as_deref())
            .collect::<Vec<_>>();
        assert_eq!(restored_ids, vec!["first", "second", "third"]);
        assert_eq!(index, 1);
        assert_eq!(order, vec![0, 1, 2]);
    }

    #[test]
    fn persisted_playback_restore_requires_current_track() {
        let snapshot = session::PersistedPlaybackState {
            version: session::PLAYBACK_STATE_VERSION,
            current_item_id: "missing".to_string(),
            ordered_item_ids: vec!["track-id".to_string()],
            position_secs: 0,
            shuffle_enabled: false,
        };

        assert!(restore_playback_snapshot_tracks(&[test_track()], &snapshot).is_none());
    }

    #[test]
    fn legacy_library_cache_migrates_and_hydrates_stream_headers() {
        let cache = CacheDatabase::open_memory().expect("in-memory cache opens");
        let session = test_session();
        let tracks = vec![test_track()];
        let legacy_key = legacy_library_cache_key_v2(&session);
        cache
            .set_setting(
                &legacy_key,
                &serde_json::to_string(&tracks).expect("serialize"),
            )
            .expect("write legacy cache");

        let mut loaded = load_library_cache(&cache, &session)
            .expect("load library cache")
            .expect("tracks exist");
        hydrate_cached_library(&mut loaded, &session);

        assert_eq!(loaded.tracks.len(), 1);
        assert_eq!(loaded.tracks[0].title, "Song");
        assert!(loaded.playlists.is_empty());
        assert_eq!(
            loaded.tracks[0].fallback_stream_url,
            tracks[0].fallback_stream_url
        );
        assert_eq!(
            loaded.tracks[0].stream_http_headers,
            vec![("X-Emby-Token".to_string(), "token".to_string())]
        );
    }

    #[test]
    fn cached_library_hydrates_sidebar_cover_thumbnail_size() {
        let session = test_session();
        let mut library = CachedLibrary {
            tracks: vec![UiTrack {
                thumbnail_artwork_url: Some(
                    "https://jellyfin.example/Items/album-id/Images/Primary?maxWidth=160&maxHeight=160&quality=80&api_key=token"
                        .to_string(),
                ),
                ..test_track()
            }],
            playlists: Vec::new(),
        };

        hydrate_cached_library(&mut library, &session);

        let thumbnail_url = library.tracks[0]
            .thumbnail_artwork_url
            .as_deref()
            .expect("thumbnail url");
        let parsed = url::Url::parse(thumbnail_url).expect("valid thumbnail url");
        let query = parsed.query_pairs().collect::<HashMap<_, _>>();
        assert_eq!(
            query.get("maxWidth").map(std::borrow::Cow::as_ref),
            Some("220")
        );
        assert_eq!(
            query.get("maxHeight").map(std::borrow::Cow::as_ref),
            Some("220")
        );
        assert_eq!(
            query.get("quality").map(std::borrow::Cow::as_ref),
            Some("80")
        );
        assert_eq!(
            query.get("api_key").map(std::borrow::Cow::as_ref),
            Some("token")
        );
    }

    #[test]
    fn changed_summary_ids_selects_new_and_modified_items() {
        let unchanged = UiTrack {
            item_id: Some("unchanged".to_string()),
            date_last_saved: Some("stamp-a".to_string()),
            ..test_track()
        };
        let changed = UiTrack {
            item_id: Some("changed".to_string()),
            date_last_saved: Some("stamp-b".to_string()),
            ..test_track()
        };
        let deleted = UiTrack {
            item_id: Some("deleted".to_string()),
            date_last_saved: Some("stamp-c".to_string()),
            ..test_track()
        };
        let cached = [unchanged, changed, deleted]
            .into_iter()
            .filter_map(|track| track.item_id.clone().map(|id| (id, track)))
            .collect::<HashMap<_, _>>();
        let summaries = vec![
            JellyfinItemSummary {
                id: "unchanged".to_string(),
                date_last_saved: Some("stamp-a".to_string()),
            },
            JellyfinItemSummary {
                id: "changed".to_string(),
                date_last_saved: Some("stamp-new".to_string()),
            },
            JellyfinItemSummary {
                id: "new".to_string(),
                date_last_saved: Some("stamp-new-item".to_string()),
            },
        ];

        let changed_ids = changed_summary_ids(&summaries, &cached, |track| {
            track.date_last_saved.as_deref()
        });

        assert_eq!(changed_ids, vec!["changed".to_string(), "new".to_string()]);
    }

    #[test]
    fn summaries_missing_change_stamps_detects_unsafe_incremental_refresh() {
        assert!(summaries_missing_change_stamps(&[JellyfinItemSummary {
            id: "track-id".to_string(),
            date_last_saved: None,
        }]));
        assert!(!summaries_missing_change_stamps(&[JellyfinItemSummary {
            id: "track-id".to_string(),
            date_last_saved: Some("stamp".to_string()),
        }]));
    }

    #[test]
    fn empty_library_cache_is_ignored() {
        let cache = CacheDatabase::open_memory().expect("in-memory cache opens");
        let session = test_session();
        cache
            .set_setting(
                &library_cache_key(&session),
                &serde_json::to_string(&CachedLibrary {
                    tracks: Vec::new(),
                    playlists: Vec::new(),
                })
                .expect("serialize"),
            )
            .expect("write empty cache");

        assert!(
            load_library_cache(&cache, &session)
                .expect("load library cache")
                .is_none()
        );
    }

    #[test]
    fn library_cache_round_trips_playlists_and_hydrates_headers() {
        let cache = CacheDatabase::open_memory().expect("in-memory cache opens");
        let session = test_session();
        let track = test_track();
        let playlist = UiPlaylist {
            id: "playlist-id".to_string(),
            name: "Favorites".to_string(),
            date_last_saved: Some("2026-01-01T00:00:00.0000000Z".to_string()),
            artwork_url: None,
            thumbnail_artwork_url: None,
            tracks: vec![track.clone()],
        };
        let payload = ConnectionPayload {
            session: session.clone(),
            tracks: vec![track],
            playlists: vec![playlist],
        };

        save_library_cache(&cache, &session, &payload).expect("save library cache");
        let mut loaded = load_library_cache(&cache, &session)
            .expect("load library cache")
            .expect("library exists");
        hydrate_cached_library(&mut loaded, &session);

        assert_eq!(loaded.tracks.len(), 1);
        assert_eq!(loaded.playlists.len(), 1);
        assert_eq!(loaded.playlists[0].name, "Favorites");
        assert_eq!(loaded.playlists[0].tracks.len(), 1);
        assert_eq!(
            loaded.playlists[0].tracks[0].stream_http_headers,
            vec![("X-Emby-Token".to_string(), "token".to_string())]
        );
    }

    #[test]
    fn radio_source_detection_recognizes_youtube_and_twitch_urls() {
        let youtube = url::Url::parse("https://www.youtube.com/watch?v=abc123").expect("url");
        let youtube_short = url::Url::parse("https://youtu.be/abc123").expect("url");
        let twitch = url::Url::parse("https://www.twitch.tv/channel").expect("url");
        let stream = url::Url::parse("https://radio.example/live.mp3").expect("url");

        assert_eq!(
            radio_source_kind_for_url(&youtube),
            RadioSourceKind::YouTube
        );
        assert_eq!(
            radio_source_kind_for_url(&youtube_short),
            RadioSourceKind::YouTube
        );
        assert_eq!(radio_source_kind_for_url(&twitch), RadioSourceKind::Twitch);
        assert_eq!(radio_source_kind_for_url(&stream), RadioSourceKind::Stream);
    }

    #[test]
    fn legacy_radio_station_source_falls_back_to_url_detection() {
        assert_eq!(
            radio_source_kind_from_station("local", "https://www.youtube.com/live/abc123"),
            RadioSourceKind::YouTube
        );
        assert_eq!(
            radio_source_kind_from_station("local", "https://twitch.tv/channel"),
            RadioSourceKind::Twitch
        );
        assert_eq!(
            radio_source_kind_from_station("local", "https://radio.example/live.mp3"),
            RadioSourceKind::Stream
        );
    }

    #[test]
    fn radio_station_conflicts_ignores_the_edited_station() {
        let stations = vec![RadioStation {
            id: "custom:1".to_string(),
            name: "Loft".to_string(),
            url: "https://radio.example/live.mp3".to_string(),
            source: "stream".to_string(),
            icon: None,
            built_in: false,
        }];

        assert!(!radio_station_conflicts(
            &stations,
            Some("custom:1"),
            "Loft",
            "https://radio.example/live.mp3"
        ));
        assert!(radio_station_conflicts(
            &stations,
            None,
            "Loft",
            "https://radio.example/live.mp3"
        ));
    }

    #[test]
    fn radio_station_deserializes_without_icon_field() {
        let station: RadioStation = serde_json::from_str(
            r#"{"id":"custom:1","name":"Test","url":"https://radio.example/live.mp3","source":"stream","built_in":false}"#,
        )
        .expect("station");

        assert_eq!(station.icon, None);
        assert_eq!(
            station.icon_glyph(),
            default_radio_icon_for_kind(RadioSourceKind::Stream)
        );
    }

    #[test]
    fn playback_requests_can_target_direct_or_transcoded_urls() {
        let track = test_track();

        let direct =
            playback_request_for_track_kind(&track, PlaybackStreamKind::Direct).expect("direct");
        let fallback = playback_request_for_track_kind(&track, PlaybackStreamKind::Transcode)
            .expect("fallback");

        assert_eq!(direct.stream_kind, PlaybackStreamKind::Direct);
        assert_eq!(direct.stream_url.path(), "/Audio/track-id/stream");
        assert_eq!(fallback.stream_kind, PlaybackStreamKind::Transcode);
        assert_eq!(fallback.stream_url.path(), "/Audio/track-id/universal");
    }
}

fn rounded_rect(cr: &gtk::cairo::Context, x: f64, y: f64, width: f64, height: f64, radius: f64) {
    let radius = radius.min(width / 2.0).min(height / 2.0);
    cr.new_sub_path();
    cr.arc(
        x + width - radius,
        y + radius,
        radius,
        -std::f64::consts::FRAC_PI_2,
        0.0,
    );
    cr.arc(
        x + width - radius,
        y + height - radius,
        radius,
        0.0,
        std::f64::consts::FRAC_PI_2,
    );
    cr.arc(
        x + radius,
        y + height - radius,
        radius,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    cr.arc(
        x + radius,
        y + radius,
        radius,
        std::f64::consts::PI,
        std::f64::consts::PI * 1.5,
    );
    cr.close_path();
}

#[allow(dead_code)]
fn set_margin_all(widget: &impl IsA<gtk::Widget>, margin: i32) {
    widget.set_margin_top(margin);
    widget.set_margin_bottom(margin);
    widget.set_margin_start(margin);
    widget.set_margin_end(margin);
}
