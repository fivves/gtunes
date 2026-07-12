use super::prelude::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub(crate) enum SortColumn {
    Title,
    Artist,
    Album,
    Duration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LibraryPage {
    Tracks,
    Albums,
    Artists,
    Playlists,
    Radio,
    NextUp,
}

pub(crate) struct UiState {
    pub(crate) all_tracks: Vec<UiTrack>,
    pub(crate) tracks: Vec<UiTrack>,
    pub(crate) playlists: Vec<UiPlaylist>,
    pub(crate) radio_stations: Vec<RadioStation>,
    pub(crate) track_filter_signature: TrackFilterSignature,
    pub(crate) library_albums: Vec<AlbumSummary>,
    pub(crate) library_artists: Vec<ArtistSummary>,
    pub(crate) collection_render_generation: u64,
    pub(crate) active_page: LibraryPage,
    pub(crate) album_filter: Option<String>,
    pub(crate) artist_filter: Option<String>,
    pub(crate) playlist_filter: Option<String>,
    pub(crate) collection_detail_title: Option<String>,
    pub(crate) collection_detail_subtitle: Option<String>,
    pub(crate) collection_detail_parent_search_query: Option<String>,
    pub(crate) collection_return_target: Option<CollectionReturnTarget>,
    pub(crate) collection_parent_return_target: Option<CollectionReturnTarget>,
    pub(crate) selected_index: usize,
    pub(crate) search_query: String,
    pub(crate) connection_generation: u64,
    pub(crate) jellyfin_connected: bool,
    pub(crate) sort_column: SortColumn,
    pub(crate) sort_ascending: bool,
    pub(crate) keep_playing_while_closed: bool,
    pub(crate) animations_enabled: bool,
    pub(crate) font_mono: bool,
    pub(crate) playback_session: session::PlaybackSession<UiTrack>,
    pub(crate) track_indicators: Vec<(String, gtk::Image)>,
    pub(crate) last_playback_snapshot_at: Option<Instant>,
    pub(crate) library_stack: Option<gtk::Stack>,
    pub(crate) album_grid: Option<gtk::FlowBox>,
    pub(crate) artist_grid: Option<gtk::FlowBox>,
    pub(crate) playlist_grid: Option<gtk::FlowBox>,
    pub(crate) album_grid_scroll_value: f64,
    pub(crate) artist_grid_scroll_value: f64,
    pub(crate) playlist_grid_scroll_value: f64,
    pub(crate) radio_grid: Option<gtk::FlowBox>,
    pub(crate) detail_header: Option<gtk::Box>,
    pub(crate) detail_title_label: Option<gtk::Label>,
    pub(crate) detail_subtitle_label: Option<gtk::Label>,
    pub(crate) nav_list: Option<gtk::ListBox>,
    pub(crate) nav_track_count: Option<gtk::Label>,
    pub(crate) nav_album_count: Option<gtk::Label>,
    pub(crate) nav_artist_count: Option<gtk::Label>,
    pub(crate) nav_playlist_count: Option<gtk::Label>,
    pub(crate) nav_radio_count: Option<gtk::Label>,
    pub(crate) track_model: gtk::StringList,
    pub(crate) track_selection: Option<gtk::SingleSelection>,
    pub(crate) track_stack: Option<gtk::Stack>,
    pub(crate) track_empty: Option<gtk::Label>,
    pub(crate) track_empty_detail: Option<gtk::Label>,
    pub(crate) now_title: gtk::Label,
    pub(crate) now_meta: gtk::Label,
    pub(crate) playback_status: gtk::Label,
    pub(crate) page_summary: gtk::Label,
    pub(crate) connection_status: gtk::Label,
    pub(crate) connection_detail: gtk::Label,
    pub(crate) sync_spinner: Option<gtk::Spinner>,
    pub(crate) connection_card: Option<gtk::Box>,
    pub(crate) connection_form_status: Option<gtk::Label>,
    pub(crate) connection_server_entry: Option<gtk::Entry>,
    pub(crate) connection_username_entry: Option<gtk::Entry>,
    pub(crate) connection_password_entry: Option<gtk::PasswordEntry>,
    pub(crate) radio_name_entry: Option<gtk::Entry>,
    pub(crate) radio_url_entry: Option<gtk::Entry>,
    pub(crate) radio_icon_entry: Option<gtk::Entry>,
    pub(crate) search_entry: Option<gtk::SearchEntry>,
    pub(crate) cover_art: Option<gtk::Image>,
    pub(crate) play_button: Option<gtk::Button>,
    pub(crate) shuffle_button: Option<gtk::Button>,
    pub(crate) refresh_button: Option<gtk::Button>,
    pub(crate) reconnect_button: Option<gtk::Button>,
    pub(crate) queue_view: Option<Rc<QueueView>>,
    pub(crate) sidebar_queue_card: Option<gtk::Box>,
    pub(crate) next_up_view: Option<Rc<NextUpPageView>>,
    pub(crate) wave_area: Option<gtk::DrawingArea>,
    pub(crate) elapsed_label: gtk::Label,
    pub(crate) remaining_label: gtk::Label,
    pub(crate) waveform_status: gtk::Label,
    pub(crate) waveform: Rc<RefCell<WaveformVisual>>,
    pub(crate) playback: Option<PlaybackEngine>,
    pub(crate) loading_spinner: Option<gtk::Spinner>,
    pub(crate) mpris: Option<MediaControls>,
    pub(crate) discord_presence: Option<DiscordPresence>,
    pub(crate) discord_presence_enabled: bool,
    pub(crate) cast_button: Option<gtk::MenuButton>,
    pub(crate) cast_device_box: Option<gtk::Box>,
    pub(crate) cast_status_label: Option<gtk::Label>,
    pub(crate) cast_scan_spinner: Option<gtk::Spinner>,
    pub(crate) active_cast_device: Option<CastDevice>,
    pub(crate) last_cast_device: Option<CastDevice>,
    pub(crate) last_cast_devices: Vec<CastDevice>,
    pub(crate) cast_session: Option<cast::CastSession>,
    pub(crate) cast_is_playing: bool,
    pub(crate) cast_position_secs: f64,
    pub(crate) cast_duration_secs: f64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TrackFilterSignature {
    pub(crate) album_filter: Option<String>,
    pub(crate) artist_filter: Option<String>,
    pub(crate) playlist_filter: Option<String>,
    pub(crate) search_query: String,
    pub(crate) sort_column: SortColumn,
    pub(crate) sort_ascending: bool,
}

impl UiState {
    pub(crate) fn is_track_list_visible(&self) -> bool {
        self.active_page == LibraryPage::Tracks
            || self.album_filter.is_some()
            || self.playlist_filter.is_some()
    }

    pub(crate) fn current_track_filter_signature(&self) -> TrackFilterSignature {
        TrackFilterSignature {
            album_filter: self.album_filter.clone(),
            artist_filter: self.artist_filter.clone(),
            playlist_filter: self.playlist_filter.clone(),
            search_query: self.search_query.clone(),
            sort_column: self.sort_column,
            sort_ascending: self.sort_ascending,
        }
    }

    pub(crate) fn track_filter_is_current(&self) -> bool {
        self.track_filter_signature == self.current_track_filter_signature()
    }
}

pub(crate) const LEFT_SIDEBAR_CONTENT_WIDTH: i32 = 220;

pub(crate) const SIDEBAR_COVER_ART_IMAGE_SIZE: u32 = LEFT_SIDEBAR_CONTENT_WIDTH as u32;

pub(crate) const LEFT_SIDEBAR_WIDTH: i32 = LEFT_SIDEBAR_CONTENT_WIDTH + 20;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VisibleLibraryContent {
    Tracks,
    Albums,
    Artists,
    Playlists,
    Radio,
    NextUp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NavDirection {
    DrillForward,
    DrillBackward,
    PageForward,
    PageBackward,
}

#[derive(Clone, Debug)]
pub(crate) struct CollectionReturnTarget {
    pub(crate) content: VisibleLibraryContent,
    pub(crate) key: String,
}

pub(crate) fn visible_library_content(ui: &UiState) -> VisibleLibraryContent {
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

pub(crate) fn library_page_order(page: LibraryPage) -> usize {
    match page {
        LibraryPage::Tracks => 0,
        LibraryPage::Albums => 1,
        LibraryPage::Artists => 2,
        LibraryPage::Playlists => 3,
        LibraryPage::Radio => 4,
        LibraryPage::NextUp => 5,
    }
}
