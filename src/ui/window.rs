use adw::prelude::*;
use gtk::glib::object::IsA;
use gtk::{Align, Orientation};
use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, PlatformConfig};
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Mutex, mpsc};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::cache::{CacheDatabase, JellyfinSession};
use crate::config;
use crate::jellyfin::{JellyfinClient, JellyfinTrack};
use crate::playback::{PlaybackEngine, PlaybackRequest, PlaybackState};
use crate::waveform::{WaveformKey, WaveformSummary};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
struct UiTrack {
    item_id: Option<String>,
    #[serde(default)]
    album_id: Option<String>,
    media_source_id: Option<String>,
    stream_url: Option<String>,
    artwork_url: Option<String>,
    thumbnail_artwork_url: Option<String>,
    title: String,
    artist: String,
    #[serde(default)]
    album_artist: Option<String>,
    album: String,
    disc_number: Option<i32>,
    track_number: Option<i32>,
    duration: String,
    quality: String,
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
    active_page: LibraryPage,
    album_filter: Option<String>,
    artist_filter: Option<String>,
    collection_detail_title: Option<String>,
    collection_detail_subtitle: Option<String>,
    selected_index: usize,
    search_query: String,
    connection_generation: u64,
    sort_column: SortColumn,
    sort_ascending: bool,
    shuffle_enabled: bool,
    now_playing_key: Option<String>,
    track_indicators: HashMap<usize, gtk::Image>,
    playback_order: Vec<usize>,
    library_stack: Option<gtk::Stack>,
    album_grid: Option<gtk::FlowBox>,
    artist_grid: Option<gtk::FlowBox>,
    detail_header: Option<gtk::Box>,
    detail_title_label: Option<gtk::Label>,
    detail_subtitle_label: Option<gtk::Label>,
    nav_track_count: Option<gtk::Label>,
    nav_album_count: Option<gtk::Label>,
    nav_artist_count: Option<gtk::Label>,
    track_model: gtk::StringList,
    track_selection: Option<gtk::SingleSelection>,
    track_stack: Option<gtk::Stack>,
    track_empty: Option<gtk::Label>,
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
    search_entry: Option<gtk::SearchEntry>,
    cover_art: Option<gtk::Image>,
    play_button: Option<gtk::Button>,
    shuffle_button: Option<gtk::Button>,
    shuffle_status_label: Option<gtk::Label>,
    queue_view: Option<Rc<QueueView>>,
    wave_area: Option<gtk::DrawingArea>,
    elapsed_label: gtk::Label,
    remaining_label: gtk::Label,
    waveform_status: gtk::Label,
    waveform: Rc<RefCell<WaveformVisual>>,
    playback: Option<PlaybackEngine>,
    mpris: Option<MediaControls>,
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
}

enum ConnectionMessage {
    Progress { loaded: usize, total: Option<usize> },
    Finished(Result<ConnectionPayload, String>),
}

const CONTEXT_RAIL_EXPANDED_WIDTH: i32 = 320;
const LEFT_SIDEBAR_CONTENT_WIDTH: i32 = 220;
const LEFT_SIDEBAR_WIDTH: i32 = LEFT_SIDEBAR_CONTENT_WIDTH + 20;
const ACTION_PANEL_WIDTH: i32 = 220;
const ALBUM_ART_SIZE: i32 = 168;
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

impl UiTrack {
    fn from_jellyfin(track: JellyfinTrack, client: &JellyfinClient) -> Self {
        let artist = if !track.artists.is_empty() {
            track.artists.join(", ")
        } else if !track.artist_items.is_empty() {
            track
                .artist_items
                .into_iter()
                .map(|artist| artist.name)
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            "Unknown Artist".to_string()
        };
        let album_artist = track.album_artist.clone();

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
            .item_stream_url(&track.id)
            .ok()
            .map(|url| url.to_string());
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
            .item_image_url_with_size(artwork_item_id, "Primary", Some(160))
            .ok()
            .map(|url| url.to_string());

        Self {
            item_id: Some(track.id),
            album_id: track.album_id,
            media_source_id: Some(media_source_id),
            stream_url,
            artwork_url,
            thumbnail_artwork_url,
            title: track.name,
            artist,
            album_artist,
            album: track.album.unwrap_or_else(|| "Unknown Album".to_string()),
            disc_number: track.parent_index_number,
            track_number: track.index_number,
            duration: format_runtime(track.run_time_ticks),
            quality,
        }
    }
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
        .title(format!(
            "{} - {}",
            config::APP_CODENAME,
            config::DEVELOPER_NAME
        ))
        .default_width(1240)
        .default_height(760)
        .width_request(860)
        .height_request(560)
        .build();

    let root = gtk::Box::new(Orientation::Vertical, 0);
    root.add_css_class("app-root");
    window.set_content(Some(&root));

    let view_settings = load_library_view_settings();
    let state = Rc::new(RefCell::new(UiState {
        all_tracks: Vec::new(),
        tracks: Vec::new(),
        active_page: LibraryPage::Tracks,
        album_filter: None,
        artist_filter: None,
        collection_detail_title: None,
        collection_detail_subtitle: None,
        selected_index: 0,
        search_query: String::new(),
        connection_generation: CONNECTION_GENERATION.load(AtomicOrdering::SeqCst),
        sort_column: view_settings.sort_column,
        sort_ascending: view_settings.sort_ascending,
        shuffle_enabled: false,
        now_playing_key: None,
        track_indicators: HashMap::new(),
        playback_order: Vec::new(),
        library_stack: None,
        album_grid: None,
        artist_grid: None,
        detail_header: None,
        detail_title_label: None,
        detail_subtitle_label: None,
        nav_track_count: None,
        nav_album_count: None,
        nav_artist_count: None,
        track_model: gtk::StringList::new(&[]),
        track_selection: None,
        track_stack: None,
        track_empty: None,
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
        search_entry: None,
        cover_art: None,
        play_button: None,
        shuffle_button: None,
        shuffle_status_label: None,
        queue_view: None,
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
        mpris: None,
    }));

    let (context_toggle, context_revealer) = context_rail_toggle_button();

    setup_mpris(state.clone());
    root.append(&build_player_bar(state.clone(), context_toggle.clone()));
    root.append(&build_body(state.clone(), context_revealer, context_toggle));
    root.append(&build_bottom_bar(state.clone()));
    load_selected_waveform(&state);
    start_playback_timer(&state);

    window
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
        MediaControlEvent::Next => play_next_track(state),
        MediaControlEvent::Previous => play_previous_track(state),
        MediaControlEvent::Stop => {
            let mut ui = state.borrow_mut();
            stop_playback(&mut ui);
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

fn build_player_bar(state: Rc<RefCell<UiState>>, context_toggle: gtk::Button) -> gtk::Box {
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
    transport.append(&play);
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
    state.borrow().playback_status.set_xalign(0.5);
    state.borrow().playback_status.set_halign(Align::Center);
    state.borrow().playback_status.set_single_line_mode(true);
    state.borrow().playback_status.set_lines(1);
    {
        let state_click = state.clone();
        let click = gtk::GestureClick::new();
        click.connect_pressed(move |_, _, _, _| {
            scroll_to_now_playing(&state_click);
        });
        state.borrow().now_title.add_controller(click);
        state
            .borrow()
            .now_title
            .set_cursor_from_name(Some("pointer"));
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

    let actions = gtk::Box::new(Orientation::Vertical, 8);
    actions.set_valign(Align::Center);
    actions.set_halign(Align::End);
    actions.set_hexpand(false);
    actions.set_size_request(ACTION_PANEL_WIDTH, -1);
    let search = gtk::SearchEntry::new();
    search.add_css_class("search");
    search.set_hexpand(true);
    search.set_halign(Align::Fill);
    search.set_size_request(ACTION_PANEL_WIDTH, -1);
    search.set_placeholder_text(Some("Search library"));
    {
        let state = state.clone();
        search.connect_search_changed(move |entry| {
            set_search_query(&state, entry.text().trim());
        });
    }
    state.borrow_mut().search_entry = Some(search.clone());
    actions.append(&search);

    let action_row = gtk::Box::new(Orientation::Horizontal, 8);
    action_row.add_css_class("action-strip");
    action_row.set_halign(Align::Fill);
    action_row.set_hexpand(true);
    action_row.set_size_request(ACTION_PANEL_WIDTH, -1);
    let (shuffle, shuffle_status) = shuffle_button();
    {
        let state = state.clone();
        shuffle.connect_clicked(move |_| {
            toggle_shuffle(&state);
        });
    }
    state.borrow_mut().shuffle_button = Some(shuffle.clone());
    state.borrow_mut().shuffle_status_label = Some(shuffle_status);
    action_row.append(&shuffle);

    let dice = icon_button("media-playback-start-symbolic", "Shuffle and play");
    dice.add_css_class("toolbar-button");
    {
        let state = state.clone();
        dice.connect_clicked(move |_| {
            shuffle_and_play(&state);
        });
    }
    action_row.append(&dice);

    let queue = icon_button("view-list-symbolic", "Up Next");
    queue.add_css_class("toolbar-button");
    queue.set_sensitive(false);
    queue.set_tooltip_text(Some("Up Next controls coming soon"));
    action_row.append(&queue);

    context_toggle.add_css_class("toolbar-button");
    action_row.append(&context_toggle);

    let settings = icon_button("emblem-system-symbolic", "Settings");
    settings.add_css_class("toolbar-button");
    settings.set_tooltip_text(Some("Reset database and cache"));
    {
        let state = state.clone();
        settings.connect_clicked(move |button| {
            if let Some(window) = button
                .root()
                .and_then(|root| root.downcast::<gtk::Window>().ok())
            {
                confirm_database_reset(&window, state.clone());
            }
        });
    }
    action_row.append(&settings);
    actions.append(&action_row);
    player.append(&actions);

    player
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
            ui.cover_art.clone(),
            ui.wave_area.clone(),
            ui.sync_spinner.clone(),
        )
    };

    {
        let mut ui = state.borrow_mut();
        ui.all_tracks.clear();
        ui.tracks.clear();
        ui.active_page = LibraryPage::Tracks;
        ui.album_filter = None;
        ui.artist_filter = None;
        ui.collection_detail_title = None;
        ui.collection_detail_subtitle = None;
        ui.selected_index = 0;
        ui.search_query.clear();
        ui.sort_column = LibraryViewSettings::default().sort_column;
        ui.sort_ascending = LibraryViewSettings::default().sort_ascending;
        ui.shuffle_enabled = false;
        ui.now_playing_key = None;
        ui.track_indicators.clear();
        ui.playback_order.clear();
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
        update_mpris_status(&mut ui);
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
    update_content_view(state);
}

fn build_body(
    state: Rc<RefCell<UiState>>,
    context_revealer: gtk::Revealer,
    context_toggle: gtk::Button,
) -> gtk::Box {
    let outer = gtk::Box::new(Orientation::Horizontal, 0);
    outer.add_css_class("main-paned");
    outer.set_hexpand(true);
    outer.set_vexpand(true);

    let sidebar = build_sidebar(state.clone());
    sidebar.set_size_request(LEFT_SIDEBAR_WIDTH, -1);
    sidebar.set_hexpand(false);
    outer.append(&sidebar);

    let inner = gtk::Box::new(Orientation::Horizontal, 0);
    inner.set_hexpand(true);
    inner.set_vexpand(true);
    inner.append(&build_content(state.clone()));
    let context_rail = build_context_rail(state);
    context_revealer.set_child(Some(&context_rail));
    context_revealer.set_reveal_child(false);
    inner.append(&context_revealer);

    outer.append(&inner);
    connect_context_rail_toggle(&context_toggle, &context_revealer, &context_rail);
    outer
}

fn build_sidebar(state: Rc<RefCell<UiState>>) -> gtk::Box {
    let sidebar = gtk::Box::new(Orientation::Vertical, 4);
    sidebar.add_css_class("sidebar");

    sidebar.append(&label("Library", "section-title"));
    sidebar.append(&nav_list(state.clone()));

    let spacer = gtk::Box::new(Orientation::Vertical, 0);
    spacer.set_vexpand(true);
    sidebar.append(&spacer);

    sidebar.append(&queue_card(state.clone()));
    sidebar.append(&sidebar_cover_art(state));

    sidebar
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
    stack.add_named(&track_table(state.clone()), Some("tracks"));
    stack.add_named(&album_grid_page(state.clone()), Some("albums"));
    stack.add_named(&artist_grid_page(state.clone()), Some("artists"));
    stack.set_visible_child_name("tracks");
    state.borrow_mut().library_stack = Some(stack.clone());

    content.append(&stack);
    refresh_collection_grids(&state);
    update_content_view(&state);
    content
}

fn detail_header(state: Rc<RefCell<UiState>>) -> gtk::Box {
    let detail_header = gtk::Box::new(Orientation::Horizontal, 10);
    detail_header.add_css_class("detail-header");
    detail_header.set_visible(false);

    let back = icon_button("go-previous-symbolic", "Back");
    back.add_css_class("toolbar-button");
    {
        let state = state.clone();
        back.connect_clicked(move |_| {
            return_to_collection_grid(&state);
        });
    }
    detail_header.append(&back);

    let detail_text = gtk::Box::new(Orientation::Vertical, 2);
    detail_text.set_halign(Align::Fill);
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

    if let Ok(Some(session)) =
        CacheDatabase::open_default().and_then(|db| db.load_jellyfin_session())
    {
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
            set_library_loaded(&state);
            return gtk::glib::ControlFlow::Break;
        }

        match receiver.try_recv() {
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
                if let Some(card) = state.borrow().connection_card.as_ref() {
                    card.set_visible(false);
                }
                gtk::glib::ControlFlow::Break
            }
            Ok(ConnectionMessage::Finished(Err(error))) => {
                if let Some(button) = button.as_ref() {
                    button.set_sensitive(true);
                }
                status.set_text("Connection failed");
                set_library_loaded(&state);
                state
                    .borrow()
                    .connection_status
                    .set_text("Connection failed");
                state.borrow().connection_detail.set_text(&error);
                gtk::glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => {
                if let Some(button) = button.as_ref() {
                    button.set_sensitive(true);
                }
                status.set_text("Connection worker stopped");
                set_library_loaded(&state);
                gtk::glib::ControlFlow::Break
            }
        }
    });
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

fn build_context_rail(_state: Rc<RefCell<UiState>>) -> gtk::Box {
    let rail = gtk::Box::new(Orientation::Vertical, 0);
    rail.add_css_class("context-rail");
    rail.set_hexpand(false);
    rail.set_size_request(320, -1);

    let header = gtk::Box::new(Orientation::Vertical, 8);
    header.add_css_class("rail-header");
    let title_row = gtk::Box::new(Orientation::Horizontal, 8);
    title_row.append(&label("Lyrics", "rail-title"));
    let spacer = gtk::Box::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    title_row.append(&spacer);
    title_row.append(&pill("Jellyfin"));
    header.append(&title_row);
    rail.append(&header);

    let placeholder = gtk::Box::new(Orientation::Vertical, 8);
    placeholder.add_css_class("placeholder");
    let microphone = gtk::Image::from_icon_name("audio-input-microphone-symbolic");
    microphone.add_css_class("placeholder-icon");
    microphone.set_pixel_size(28);
    microphone.set_halign(Align::Start);
    placeholder.append(&microphone);
    placeholder.append(&label("Lyrics coming soon", "rail-title"));
    let copy = label(
        "Jellyfin lyrics and LRC sync will land after playback.",
        "meta",
    );
    copy.set_wrap(true);
    copy.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    copy.set_width_chars(24);
    copy.set_max_width_chars(28);
    placeholder.append(&copy);
    rail.append(&placeholder);

    let spacer = gtk::Box::new(Orientation::Vertical, 0);
    spacer.set_vexpand(true);
    rail.append(&spacer);
    rail
}

#[derive(Clone, Debug)]
struct AlbumSummary {
    key: String,
    name: String,
    artist: String,
    artwork_url: Option<String>,
    song_count: usize,
}

#[derive(Clone, Debug)]
struct ArtistSummary {
    key: String,
    name: String,
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
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow.set_min_children_per_line(2);
    flow.set_max_children_per_line(8);
    flow.set_row_spacing(16);
    flow.set_column_spacing(16);
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
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow.set_min_children_per_line(2);
    flow.set_max_children_per_line(8);
    flow.set_row_spacing(16);
    flow.set_column_spacing(16);
    flow.set_homogeneous(true);
    flow.set_valign(Align::Start);
    scroll.set_child(Some(&flow));

    state.borrow_mut().artist_grid = Some(flow);
    scroll
}

fn refresh_collection_grids(state: &Rc<RefCell<UiState>>) {
    refresh_album_grid(state);
    refresh_artist_grid(state);
    update_nav_counts(state);
}

fn refresh_album_grid(state: &Rc<RefCell<UiState>>) {
    let (grid, albums) = {
        let ui = state.borrow();
        let albums = if ui.active_page == LibraryPage::Artists {
            ui.artist_filter
                .as_deref()
                .map(|selected_artist_key| {
                    album_summaries_for_artist(
                        &ui.all_tracks,
                        selected_artist_key,
                        &ui.search_query,
                    )
                })
                .unwrap_or_else(|| album_summaries(&ui.all_tracks, &ui.search_query))
        } else {
            album_summaries(&ui.all_tracks, &ui.search_query)
        };
        (ui.album_grid.clone(), albums)
    };
    let Some(grid) = grid else {
        return;
    };
    clear_flow_box(&grid);

    if albums.is_empty() {
        grid.insert(&collection_empty_state("No albums found"), -1);
        return;
    }

    for album in albums {
        let tile = album_tile(album, state.clone());
        grid.insert(&tile, -1);
    }
}

fn refresh_artist_grid(state: &Rc<RefCell<UiState>>) {
    let (grid, artists) = {
        let ui = state.borrow();
        (
            ui.artist_grid.clone(),
            artist_summaries(&ui.all_tracks, &ui.search_query),
        )
    };
    let Some(grid) = grid else {
        return;
    };
    clear_flow_box(&grid);

    if artists.is_empty() {
        grid.insert(&collection_empty_state("No artists found"), -1);
        return;
    }

    for artist in artists {
        let tile = artist_tile(artist, state.clone());
        grid.insert(&tile, -1);
    }
}

fn clear_flow_box(flow: &gtk::FlowBox) {
    while let Some(child) = flow.first_child() {
        flow.remove(&child);
    }
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

fn album_tile(album: AlbumSummary, state: Rc<RefCell<UiState>>) -> gtk::Button {
    let button = collection_tile_button(&album.name);
    button.add_css_class("album-tile");
    button.set_halign(Align::Start);
    button.set_hexpand(false);
    button.set_size_request(ALBUM_ART_SIZE, ALBUM_ART_SIZE);

    let frame = gtk::Box::new(Orientation::Vertical, 0);
    frame.add_css_class("album-art-frame");
    frame.set_size_request(ALBUM_ART_SIZE, ALBUM_ART_SIZE);
    frame.set_halign(Align::Fill);
    frame.set_valign(Align::Fill);
    frame.set_overflow(gtk::Overflow::Hidden);

    let art = cover_art(ALBUM_ART_SIZE);
    art.add_css_class("collection-art");
    art.add_css_class("album-art");
    art.set_halign(Align::Fill);
    art.set_valign(Align::Fill);
    art.set_icon_name(Some("audio-x-generic-symbolic"));
    if let Some(url) = album.artwork_url.clone() {
        let current_url = Rc::new(RefCell::new(Some(url.clone())));
        load_queue_art(Some(url), art.clone(), current_url);
    }
    frame.append(&art);
    button.set_child(Some(&frame));

    button.connect_clicked(move |_| {
        show_album_tracks(&state, &album);
    });
    button
}

fn artist_tile(artist: ArtistSummary, state: Rc<RefCell<UiState>>) -> gtk::Button {
    let button = collection_tile_button(&artist.name);
    let layout = gtk::Box::new(Orientation::Vertical, 7);
    layout.set_halign(Align::Fill);

    let avatar = cover_art(148);
    avatar.add_css_class("collection-art");
    avatar.add_css_class("artist-placeholder");
    avatar.set_icon_name(Some("avatar-default-symbolic"));
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

fn collection_tile_button(title: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("collection-tile");
    button.add_css_class("flat");
    button.set_halign(Align::Fill);
    button.set_valign(Align::Start);
    button.set_tooltip_text(Some(title));
    button.set_cursor_from_name(Some("pointer"));
    button
}

fn collection_tile_label(text: &str, class_name: &str) -> gtk::Label {
    let title = label(text, class_name);
    title.set_single_line_mode(true);
    title.set_lines(1);
    title.set_halign(Align::Fill);
    title
}

fn album_summaries(tracks: &[UiTrack], query: &str) -> Vec<AlbumSummary> {
    let mut albums = Vec::<AlbumAccumulator>::new();
    for (track_index, track) in tracks.iter().enumerate() {
        let key = album_key(track);
        if let Some(album) = albums.iter_mut().find(|album| album.key == key) {
            album.song_count += 1;
            if album.artwork_url.is_none() {
                album.artwork_url = track.thumbnail_artwork_url.clone();
            }
            add_artist_vote(
                &mut album.explicit_artist_votes,
                track.album_artist.as_deref(),
                track_index,
            );
            add_artist_vote(
                &mut album.fallback_artist_votes,
                Some(&track.artist),
                track_index,
            );
            continue;
        }

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
            track_index,
        );
        add_artist_vote(
            &mut album.fallback_artist_votes,
            Some(&track.artist),
            track_index,
        );
        albums.push(album);
    }

    let mut albums = albums
        .into_iter()
        .map(|album| AlbumSummary {
            key: album.key,
            name: album.name,
            artist: preferred_album_artist(
                &album.explicit_artist_votes,
                &album.fallback_artist_votes,
            ),
            artwork_url: album.artwork_url,
            song_count: album.song_count,
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

fn album_summaries_for_artist(
    tracks: &[UiTrack],
    selected_artist_key: &str,
    query: &str,
) -> Vec<AlbumSummary> {
    album_summaries(tracks, query)
        .into_iter()
        .filter(|album| artist_key(&album.artist) == selected_artist_key)
        .collect()
}

fn artist_album_count(tracks: &[UiTrack], selected_artist_key: &str) -> usize {
    album_summaries(tracks, "")
        .into_iter()
        .filter(|album| artist_key(&album.artist) == selected_artist_key)
        .count()
}

fn artist_summaries(tracks: &[UiTrack], query: &str) -> Vec<ArtistSummary> {
    let mut artists = Vec::<ArtistSummary>::new();
    for album in album_summaries(tracks, "") {
        let key = artist_key(&album.artist);
        if let Some(artist) = artists.iter_mut().find(|artist| artist.key == key) {
            artist.album_count += 1;
            artist.song_count += album.song_count;
            continue;
        }

        artists.push(ArtistSummary {
            key,
            name: album.artist,
            album_count: 1,
            song_count: album.song_count,
        });
    }

    let query = query.trim().to_lowercase();
    if !query.is_empty() {
        artists.retain(|artist| artist.name.to_lowercase().contains(&query));
    }

    artists.sort_by(|left, right| compare_text(&left.name, &right.name));
    artists
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
    normalized_key(artist)
}

fn normalized_key(value: &str) -> String {
    value.trim().to_lowercase()
}

fn add_artist_vote(votes: &mut Vec<ArtistVote>, artist: Option<&str>, first_seen: usize) {
    let Some(artist) = artist.map(str::trim).filter(|artist| !artist.is_empty()) else {
        return;
    };
    let key = artist_key(artist);
    if let Some(vote) = votes.iter_mut().find(|vote| vote.key == key) {
        vote.count += 1;
        return;
    }

    votes.push(ArtistVote {
        key,
        name: artist.to_string(),
        count: 1,
        first_seen,
    });
}

fn preferred_album_artist(explicit_votes: &[ArtistVote], fallback_votes: &[ArtistVote]) -> String {
    preferred_artist_vote(explicit_votes)
        .or_else(|| preferred_artist_vote(fallback_votes))
        .map(|vote| vote.name.clone())
        .unwrap_or_else(|| "Unknown Artist".to_string())
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
        header: "TITLE",
        width: TITLE_WIDTH,
        expand: true,
        xalign: 0.0,
        sort_column: SortColumn::Title,
        class_name: Some("track-title"),
    },
    TrackColumn {
        header: "ARTIST",
        width: ARTIST_WIDTH,
        expand: true,
        xalign: 0.0,
        sort_column: SortColumn::Artist,
        class_name: None,
    },
    TrackColumn {
        header: "ALBUM",
        width: ALBUM_WIDTH,
        expand: true,
        xalign: 0.0,
        sort_column: SortColumn::Album,
        class_name: None,
    },
    TrackColumn {
        header: "TIME",
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
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
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

    let empty = label("Connect to Jellyfin to load your music", "meta");
    empty.set_margin_top(18);
    empty.set_margin_start(12);
    empty.set_margin_end(12);
    empty.set_valign(Align::Start);
    stack.add_named(&empty, Some("empty"));
    stack.set_visible_child_name("empty");

    {
        let mut ui = state.borrow_mut();
        ui.track_selection = Some(selection);
        ui.track_stack = Some(stack.clone());
        ui.track_empty = Some(empty);
    }
    refresh_track_model(&state);

    wrapper.append(&stack);
    wrapper
}

fn track_column_view(column: TrackColumn, state: Rc<RefCell<UiState>>) -> gtk::ColumnViewColumn {
    let factory = gtk::SignalListItemFactory::new();

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
            let is_now_playing = ui.now_playing_key.as_deref() == Some(key.as_str());
            (track, is_now_playing)
        };

        if column.sort_column == SortColumn::Title {
            bind_title_cell(list_item, &track.title, is_now_playing);
            if let Some(indicator) = get_indicator_image(list_item) {
                state_bind
                    .borrow_mut()
                    .track_indicators
                    .insert(position, indicator);
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
            let position = list_item.position() as usize;
            state.borrow_mut().track_indicators.remove(&position);
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
    let (model, selection, stack, empty, track_count, selected_index, empty_text) = {
        let ui = state.borrow();
        let empty_text = if ui.search_query.is_empty() {
            "Connect to Jellyfin to load your music".to_string()
        } else {
            format!("No tracks match \"{}\"", ui.search_query)
        };
        (
            ui.track_model.clone(),
            ui.track_selection.clone(),
            ui.track_stack.clone(),
            ui.track_empty.clone(),
            ui.tracks.len(),
            ui.selected_index,
            empty_text,
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
    let now_playing_key = ui.now_playing_key.clone();

    for (pos, indicator) in &ui.track_indicators {
        let is_playing = if let Some(track) = ui.tracks.get(*pos) {
            Some(track_key(track)) == now_playing_key
        } else {
            false
        };
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

fn sort_track_slice(
    tracks: &mut [UiTrack],
    column: SortColumn,
    ascending: bool,
    search_query: &str,
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

        let ordering = match column {
            SortColumn::Title => compare_text(&left.title, &right.title),
            SortColumn::Artist => compare_artist_album_track(left, right),
            SortColumn::Album => compare_text(&left.album, &right.album),
            SortColumn::Duration => {
                duration_seconds(&left.duration).cmp(&duration_seconds(&right.duration))
            }
        };

        let ordering = exact_match_ordering
            .then(ordering)
            .then_with(|| compare_text(&left.title, &right.title))
            .then_with(|| compare_text(&left.artist, &right.artist))
            .then_with(|| compare_text(&left.album, &right.album));

        if ascending {
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
    {
        let mut ui = state.borrow_mut();
        if ui.search_query == query {
            return;
        }
        let selected_key = ui.tracks.get(ui.selected_index).map(track_key);
        ui.search_query = query.to_string();
        apply_track_filter(&mut ui, selected_key.as_deref());
        update_now_playing_labels(&ui);
        update_play_button(&ui);
        update_page_summary(&ui);
    }
    refresh_track_model(state);
    refresh_collection_grids(state);
    update_content_view(state);
    load_selected_cover_art(state);
    load_selected_waveform(state);
}

fn set_library_page(state: &Rc<RefCell<UiState>>, page: LibraryPage) {
    {
        let mut ui = state.borrow_mut();
        ui.active_page = page;
        ui.album_filter = None;
        ui.artist_filter = None;
        ui.collection_detail_title = None;
        ui.collection_detail_subtitle = None;
        let selected_key = ui.tracks.get(ui.selected_index).map(track_key);
        apply_track_filter(&mut ui, selected_key.as_deref());
        update_now_playing_labels(&ui);
        update_play_button(&ui);
        update_page_summary(&ui);
    }
    refresh_track_model(state);
    refresh_collection_grids(state);
    update_content_view(state);
    load_selected_cover_art(state);
    load_selected_waveform(state);
}

fn show_album_tracks(state: &Rc<RefCell<UiState>>, album: &AlbumSummary) {
    {
        let mut ui = state.borrow_mut();
        let selected_artist = if ui.active_page == LibraryPage::Artists {
            ui.artist_filter.clone()
        } else {
            None
        };
        ui.active_page = if selected_artist.is_some() {
            LibraryPage::Artists
        } else {
            LibraryPage::Albums
        };
        ui.album_filter = Some(album.key.clone());
        ui.artist_filter = selected_artist;
        ui.collection_detail_title = Some(album.name.clone());
        ui.collection_detail_subtitle = Some(format!(
            "{} | {}",
            album.artist,
            count_text(album.song_count, "song", "songs")
        ));
        ui.selected_index = 0;
        apply_track_filter(&mut ui, None);
        update_now_playing_labels(&ui);
        update_play_button(&ui);
        update_page_summary(&ui);
    }
    refresh_track_model(state);
    update_content_view(state);
    load_selected_cover_art(state);
    load_selected_waveform(state);
}

fn show_artist_albums(state: &Rc<RefCell<UiState>>, artist: &ArtistSummary) {
    {
        let mut ui = state.borrow_mut();
        ui.active_page = LibraryPage::Artists;
        ui.album_filter = None;
        ui.artist_filter = Some(artist.key.clone());
        ui.collection_detail_title = Some(artist.name.clone());
        ui.collection_detail_subtitle =
            Some(artist_count_text(artist.album_count, artist.song_count));
        ui.selected_index = 0;
        apply_track_filter(&mut ui, None);
        update_now_playing_labels(&ui);
        update_play_button(&ui);
        update_page_summary(&ui);
    }
    refresh_track_model(state);
    refresh_collection_grids(state);
    update_content_view(state);
    load_selected_cover_art(state);
    load_selected_waveform(state);
}

fn return_to_collection_grid(state: &Rc<RefCell<UiState>>) {
    {
        let mut ui = state.borrow_mut();
        if ui.active_page == LibraryPage::Artists && ui.album_filter.is_some() {
            ui.album_filter = None;
            if let Some(artist_key_value) = ui.artist_filter.clone()
                && let Some(artist) = artist_summaries(&ui.all_tracks, "")
                    .into_iter()
                    .find(|artist| artist.key == artist_key_value)
            {
                ui.collection_detail_title = Some(artist.name);
                ui.collection_detail_subtitle =
                    Some(artist_count_text(artist.album_count, artist.song_count));
            }
        } else {
            ui.album_filter = None;
            ui.artist_filter = None;
            ui.collection_detail_title = None;
            ui.collection_detail_subtitle = None;
        }
        ui.selected_index = 0;
        apply_track_filter(&mut ui, None);
        update_now_playing_labels(&ui);
        update_play_button(&ui);
        update_page_summary(&ui);
    }
    refresh_track_model(state);
    refresh_collection_grids(state);
    update_content_view(state);
    load_selected_cover_art(state);
    load_selected_waveform(state);
}

fn update_content_view(state: &Rc<RefCell<UiState>>) {
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
        let show_detail = ui.album_filter.is_some() || ui.artist_filter.is_some();
        let visible_child = match (ui.active_page, show_detail) {
            (LibraryPage::Tracks, _) => "tracks",
            (LibraryPage::Albums, false) => "albums",
            (LibraryPage::Albums, true) => "tracks",
            (LibraryPage::Artists, false) => "artists",
            (LibraryPage::Artists, true) if ui.album_filter.is_none() => "albums",
            (LibraryPage::Artists, true) => "tracks",
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
        stack.set_visible_child_name(&visible_child);
    }
    if let Some(header) = detail_header.as_ref() {
        header.set_visible(show_detail);
    }
    if let Some(label) = title_label.as_ref() {
        label.set_text(&title);
    }
    if let Some(label) = subtitle_label.as_ref() {
        label.set_text(&subtitle);
    }
}

fn update_nav_counts(state: &Rc<RefCell<UiState>>) {
    let (track_label, album_label, artist_label, tracks, albums, artists) = {
        let ui = state.borrow();
        (
            ui.nav_track_count.clone(),
            ui.nav_album_count.clone(),
            ui.nav_artist_count.clone(),
            ui.all_tracks.len(),
            album_summaries(&ui.all_tracks, "").len(),
            artist_summaries(&ui.all_tracks, "").len(),
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
}

fn apply_track_filter(ui: &mut UiState, selected_key: Option<&str>) {
    let query = ui.search_query.to_lowercase();
    let artist_album_keys = ui.artist_filter.as_deref().map(|selected_artist_key| {
        album_summaries(&ui.all_tracks, "")
            .into_iter()
            .filter(|album| artist_key(&album.artist) == selected_artist_key)
            .map(|album| album.key)
            .collect::<HashSet<_>>()
    });
    ui.tracks = ui
        .all_tracks
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
        selected_key,
        &mut ui.selected_index,
    );
    let selected_index = ui.selected_index;
    rebuild_playback_order(ui, selected_index);
}

fn update_page_summary(ui: &UiState) {
    if let Some(title) = ui.collection_detail_title.as_deref() {
        if ui.active_page == LibraryPage::Artists && ui.album_filter.is_none() {
            let album_count = ui
                .artist_filter
                .as_deref()
                .map(|artist_key| artist_album_count(&ui.all_tracks, artist_key))
                .unwrap_or(0);
            ui.page_summary.set_text(&format!(
                "{title} | {}",
                artist_count_text(album_count, ui.tracks.len())
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
            ui.page_summary.set_text(&format!(
                "Albums | {} albums",
                album_summaries(&ui.all_tracks, "").len()
            ));
        }
        LibraryPage::Artists => {
            ui.page_summary.set_text(&format!(
                "Artists | {} artists",
                artist_summaries(&ui.all_tracks, "").len()
            ));
        }
    }
}

fn rebuild_playback_order(ui: &mut UiState, start_index: usize) {
    if ui.tracks.is_empty() {
        ui.playback_order.clear();
        return;
    }

    let start_index = start_index.min(ui.tracks.len().saturating_sub(1));
    if ui.shuffle_enabled {
        let mut remaining = (0..ui.tracks.len())
            .filter(|index| *index != start_index)
            .collect::<Vec<_>>();
        shuffle_indices(&mut remaining);
        ui.playback_order = std::iter::once(start_index).chain(remaining).collect();
    } else {
        ui.playback_order = (0..ui.tracks.len()).collect();
    }
}

fn shuffle_indices(indices: &mut [usize]) {
    let mut seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0x9e37_79b9_7f4a_7c15);

    for index in (1..indices.len()).rev() {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let swap_index = (seed as usize) % (index + 1);
        indices.swap(index, swap_index);
    }
}

fn next_playback_index(ui: &UiState) -> Option<usize> {
    let order_position = ui
        .playback_order
        .iter()
        .position(|index| *index == ui.selected_index)?;
    ui.playback_order.get(order_position + 1).copied()
}

fn previous_playback_index(ui: &UiState) -> Option<usize> {
    let order_position = ui
        .playback_order
        .iter()
        .position(|index| *index == ui.selected_index)?;
    order_position
        .checked_sub(1)
        .and_then(|position| ui.playback_order.get(position).copied())
}

fn queued_tracks(ui: &UiState) -> Vec<(usize, UiTrack)> {
    let Some(order_position) = ui
        .playback_order
        .iter()
        .position(|index| *index == ui.selected_index)
    else {
        return Vec::new();
    };

    ui.playback_order
        .iter()
        .skip(order_position + 1)
        .take(QUEUE_PREVIEW_LIMIT)
        .filter_map(|index| ui.tracks.get(*index).cloned().map(|track| (*index, track)))
        .collect()
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

fn current_display_track(state: &UiState) -> Option<&UiTrack> {
    state
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
    {
        let mut ui = state.borrow_mut();
        ui.all_tracks = payload.tracks;
        ui.selected_index = 0;
        ui.now_playing_key = None;
        ui.active_page = LibraryPage::Tracks;
        ui.album_filter = None;
        ui.artist_filter = None;
        ui.collection_detail_title = None;
        ui.collection_detail_subtitle = None;
        apply_track_filter(&mut ui, None);
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
    }
    refresh_track_model(state);
    refresh_collection_grids(state);
    update_content_view(state);
    load_selected_cover_art(state);
    load_selected_waveform(state);
}

fn update_now_playing_labels(state: &UiState) {
    if let Some(track) = current_display_track(state) {
        state.now_title.set_text(&track.title);
        state
            .now_meta
            .set_text(&format!("{} - {}", track.artist, track.album));
        if track.stream_url.is_some() {
            state
                .playback_status
                .set_text(&format!("Ready to stream | {}", track.quality));
        } else {
            state
                .playback_status
                .set_text("Track is missing a Jellyfin stream URL");
        }
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

fn update_play_button(state: &UiState) {
    let Some(button) = state.play_button.as_ref() else {
        return;
    };

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

    if state.shuffle_enabled {
        button.add_css_class("suggested-action");
        button.add_css_class("shuffle-on");
        button.remove_css_class("shuffle-off");
        button.set_tooltip_text(Some("Shuffle on"));
        if let Some(label) = state.shuffle_status_label.as_ref() {
            label.set_text("On");
        }
    } else {
        button.remove_css_class("suggested-action");
        button.remove_css_class("shuffle-on");
        button.add_css_class("shuffle-off");
        button.set_tooltip_text(Some("Shuffle"));
        if let Some(label) = state.shuffle_status_label.as_ref() {
            label.set_text("Off");
        }
    }
}

fn toggle_shuffle(state: &Rc<RefCell<UiState>>) {
    {
        let mut ui = state.borrow_mut();
        ui.shuffle_enabled = !ui.shuffle_enabled;
        let selected_index = ui.selected_index;
        rebuild_playback_order(&mut ui, selected_index);
        arm_gapless_next(&mut ui);
        update_shuffle_button(&ui);
    }
    rebuild_queue_list(state);
}

fn shuffle_and_play(state: &Rc<RefCell<UiState>>) {
    let first_index = {
        let mut ui = state.borrow_mut();
        if ui.tracks.is_empty() {
            ui.playback_status.set_text("No tracks to shuffle");
            return;
        }

        ui.shuffle_enabled = true;
        let mut order = (0..ui.tracks.len()).collect::<Vec<_>>();
        shuffle_indices(&mut order);
        let first_index = order.first().copied().unwrap_or(0);
        ui.playback_order = order;
        update_shuffle_button(&ui);
        first_index
    };

    play_track_at(state, first_index);
}

fn pause_playback(state: &Rc<RefCell<UiState>>) {
    let mut ui = state.borrow_mut();

    if let Some(playback) = ui.playback.as_mut() {
        match playback.pause() {
            Ok(()) => ui.playback_status.set_text("Paused"),
            Err(error) => ui
                .playback_status
                .set_text(&format!("Pause failed: {error}")),
        }
        update_play_button(&ui);
        update_mpris_status(&mut ui);
    }
}

fn resume_playback(state: &Rc<RefCell<UiState>>) {
    let mut ui = state.borrow_mut();

    match ui.playback.as_ref().map(PlaybackEngine::state).cloned() {
        Some(PlaybackState::Paused) => {
            let result = ui.playback.as_mut().expect("playback was present").resume();
            match result {
                Ok(()) => {
                    let quality = ui
                        .tracks
                        .get(ui.selected_index)
                        .map(|track| track.quality.as_str())
                        .unwrap_or("stream");
                    ui.playback_status.set_text(&format!("Playing | {quality}"));
                }
                Err(error) => ui
                    .playback_status
                    .set_text(&format!("Resume failed: {error}")),
            }
            update_play_button(&ui);
            update_mpris_status(&mut ui);
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
        previous_playback_index(&ui).unwrap_or(ui.selected_index)
    };
    play_track_at_existing_order(state, previous_index);
}

fn play_next_track(state: &Rc<RefCell<UiState>>) {
    let next_index = {
        let ui = state.borrow();
        next_playback_index(&ui).unwrap_or(ui.selected_index)
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
    let selected_index = {
        let mut ui = state.borrow_mut();
        ui.selected_index = index.min(ui.tracks.len().saturating_sub(1));
        if rebuild_order {
            let selected_index = ui.selected_index;
            rebuild_playback_order(&mut ui, selected_index);
        }
        update_now_playing_labels(&ui);
        update_play_button(&ui);
        ui.selected_index
    };
    select_track_model_row(state, selected_index);
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
    let Some(track) = ui.tracks.get(ui.selected_index).cloned() else {
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
        ui.now_playing_key = None;
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
        title: track.title.clone(),
    };
    let mut refresh_now_playing = false;
    match playback.play(request) {
        Ok(()) => {
            ui.now_playing_key = Some(track_key(&track));
            arm_gapless_next(&mut ui);
            update_now_playing_labels(&ui);
            ui.playback_status
                .set_text(&format!("Playing | {}", track.quality));
            update_mpris_metadata(&mut ui);
            update_mpris_status(&mut ui);
            refresh_now_playing = true;
        }
        Err(error) => {
            ui.now_playing_key = None;
            ui.playback_status
                .set_text(&format!("Playback failed: {error}"));
            update_mpris_status(&mut ui);
        }
    }
    update_play_button(&ui);
    drop(ui);
    update_list_indicators(state);
    if refresh_now_playing {
        load_selected_cover_art(state);
    }
}

fn playback_request_for_track(track: &UiTrack) -> Option<PlaybackRequest> {
    Some(PlaybackRequest {
        item_id: track.item_id.clone().unwrap_or_default(),
        stream_url: track.stream_url.as_deref()?.parse().ok()?,
        title: track.title.clone(),
    })
}

fn next_gapless_request(ui: &UiState) -> Option<PlaybackRequest> {
    let next_index = next_playback_index(ui)?;
    playback_request_for_track(ui.tracks.get(next_index)?)
}

fn arm_gapless_next(ui: &mut UiState) {
    let request = next_gapless_request(ui);
    if let Some(playback) = ui.playback.as_mut() {
        playback.set_next(request);
    }
}

fn stop_playback(ui: &mut UiState) {
    ui.now_playing_key = None;
    if let Some(playback) = ui.playback.as_mut()
        && let Err(error) = playback.stop()
    {
        ui.playback_status
            .set_text(&format!("Stop failed: {error}"));
    }
    update_mpris_status(ui);
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
                    update_mpris_metadata(&mut ui);
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

fn fetch_image_file(url: &str) -> Result<PathBuf, String> {
    let bytes = reqwest::blocking::get(url)
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .bytes()
        .map_err(|error| error.to_string())?;

    let path = artwork_cache_path(url);
    std::fs::write(&path, bytes).map_err(|error| error.to_string())?;
    Ok(path)
}

fn fetch_cached_image_file(url: &str) -> Result<PathBuf, String> {
    let path = artwork_cache_path(url);
    if path.exists() {
        Ok(path)
    } else {
        fetch_image_file(url)
    }
}

fn artwork_cache_path(url: &str) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);
    std::env::temp_dir().join(format!("gtunes-artwork-{:x}", hasher.finish()))
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

fn connect_and_fetch_payload(
    server_url: &str,
    username: &str,
    password: &str,
    sender: Option<mpsc::Sender<ConnectionMessage>>,
    generation: u64,
) -> Result<ConnectionPayload, String> {
    let (client, auth) = JellyfinClient::authenticate(server_url, username, password)
        .map_err(|error| error.to_string())?;
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
        if let Err(error) = save_library_cache(&cache, &payload.session, &payload.tracks) {
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

fn fetch_saved_session_payload(
    session: JellyfinSession,
    sender: Option<mpsc::Sender<ConnectionMessage>>,
    generation: u64,
) -> Result<ConnectionPayload, String> {
    let cache = CacheDatabase::open_default().map_err(|error| error.to_string())?;
    match load_library_cache(&cache, &session) {
        Ok(Some(tracks)) => return Ok(ConnectionPayload { session, tracks }),
        Ok(None) => {}
        Err(error) => tracing::warn!(%error, "failed to load cached Jellyfin library"),
    }

    let client = JellyfinClient::new(&session.server_url, Some(session.access_token.clone()))
        .map_err(|error| error.to_string())?;

    let payload = fetch_library_for_session(client, session, sender)
        .map_err(|error| format!("{error}; enter your password to refresh the session"))?;
    {
        let _guard = CACHE_RESET_LOCK
            .lock()
            .map_err(|_| "cache reset lock is poisoned".to_string())?;
        ensure_connection_generation_current(generation)?;
        if let Err(error) = save_library_cache(&cache, &payload.session, &payload.tracks) {
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
    let tracks = client
        .music_tracks_with_progress(&session.user_id, |loaded, total| {
            if let Some(sender) = sender.as_ref() {
                let _ = sender.send(ConnectionMessage::Progress { loaded, total });
            }
        })
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|track| UiTrack::from_jellyfin(track, &client))
        .collect::<Vec<_>>();

    Ok(ConnectionPayload { session, tracks })
}

fn save_library_cache(
    cache: &CacheDatabase,
    session: &JellyfinSession,
    tracks: &[UiTrack],
) -> Result<(), crate::cache::CacheError> {
    let json = serde_json::to_string(tracks)?;
    cache.set_setting(&library_cache_key(session), &json)
}

fn load_library_cache(
    cache: &CacheDatabase,
    session: &JellyfinSession,
) -> Result<Option<Vec<UiTrack>>, crate::cache::CacheError> {
    if let Some(tracks) = load_cached_tracks(cache, &library_cache_key(session))? {
        if !tracks.is_empty() {
            return Ok(Some(tracks));
        }
        tracing::warn!("ignoring empty Jellyfin library cache");
    }

    if let Some(tracks) = load_cached_tracks(cache, &legacy_library_cache_key(session))? {
        if !tracks.is_empty() {
            if let Err(error) = save_library_cache(cache, session, &tracks) {
                tracing::warn!(%error, "failed to migrate legacy Jellyfin library cache");
            }
            return Ok(Some(tracks));
        }
        tracing::warn!("ignoring empty legacy Jellyfin library cache");
    }

    Ok(None)
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

fn library_cache_key(session: &JellyfinSession) -> String {
    format!(
        "jellyfin.library.v2.{}.{}",
        library_cache_server_key(session),
        session.user_id
    )
}

fn legacy_library_cache_key(session: &JellyfinSession) -> String {
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
    card.append(&label("Next Up", "rail-title"));

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
        row.set_hexpand(true);
        row.set_size_request(0, -1);

        let art = cover_art(28);
        art.set_icon_name(Some("audio-x-generic-symbolic"));
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
    let Some(queue_view) = queue_view else {
        return;
    };

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
                    Ok(texture) => image.set_paintable(Some(&texture)),
                    Err(error) => tracing::warn!(%error, "failed to decode queue artwork"),
                }
                gtk::glib::ControlFlow::Break
            }
            Ok(Err(error)) => {
                tracing::warn!(%error, "failed to fetch queue artwork");
                gtk::glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => gtk::glib::ControlFlow::Break,
        }
    });
}

fn scroll_to_now_playing(state: &Rc<RefCell<UiState>>) {
    let (idx, stack) = {
        let ui = state.borrow();
        let idx = ui
            .tracks
            .iter()
            .position(|t| Some(track_key(t)) == ui.now_playing_key);
        (idx, ui.track_stack.clone())
    };

    let Some(idx) = idx else {
        return;
    };

    let scroll = stack
        .as_ref()
        .and_then(|s| s.child_by_name("list"))
        .and_then(|c| c.downcast::<gtk::ScrolledWindow>().ok());

    if let Some(scroll) = scroll {
        let adj = scroll.vadjustment();
        let track_count = {
            let ui = state.borrow();
            ui.tracks.len()
        };

        if track_count > 0 {
            // Use the adjustment's upper value to calculate height.
            // This matches GTK's internal estimation and prevents compounding errors.
            let row_height = adj.upper() / track_count as f64;
            let target = idx as f64 * row_height;
            adj.set_value(target.clamp(0.0, adj.upper() - adj.page_size()));
        }
    }
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
    bar
}

fn context_rail_toggle_button() -> (gtk::Button, gtk::Revealer) {
    let button = icon_button("go-previous-symbolic", "Show lyrics panel");
    let revealer = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideLeft)
        .transition_duration(180)
        .build();
    (button, revealer)
}

fn connect_context_rail_toggle(button: &gtk::Button, revealer: &gtk::Revealer, rail: &gtk::Box) {
    let button = button.clone();
    let closure_button = button.clone();
    let revealer = revealer.clone();
    let rail = rail.clone();
    let closure_rail = rail.clone();
    button.connect_clicked(move |_| {
        let expanded = !revealer.reveals_child();
        revealer.set_reveal_child(expanded);
        closure_rail.set_size_request(
            if expanded {
                CONTEXT_RAIL_EXPANDED_WIDTH
            } else {
                0
            },
            -1,
        );
        update_context_rail_toggle_button(&closure_button, expanded);
    });

    rail.set_size_request(0, -1);
    update_context_rail_toggle_button(&button, false);
}

fn update_context_rail_toggle_button(button: &gtk::Button, expanded: bool) {
    let icon_name = if expanded {
        "go-next-symbolic"
    } else {
        "go-previous-symbolic"
    };
    let tooltip = if expanded {
        "Hide lyrics panel"
    } else {
        "Show lyrics panel"
    };
    button.set_icon_name(icon_name);
    button.set_tooltip_text(Some(tooltip));
}

fn nav_list(state: Rc<RefCell<UiState>>) -> gtk::ListBox {
    let list = gtk::ListBox::new();
    list.add_css_class("nav-list");
    list.set_selection_mode(gtk::SelectionMode::Single);
    let rows = [
        (
            "audio-x-generic-symbolic",
            "Tracks",
            LibraryPage::Tracks,
            true,
        ),
        ("folder-music-symbolic", "Albums", LibraryPage::Albums, true),
        (
            "avatar-default-symbolic",
            "Artists",
            LibraryPage::Artists,
            true,
        ),
        (
            "media-playlist-consecutive-symbolic",
            "Playlists",
            LibraryPage::Tracks,
            false,
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
            _ => count.set_text("-"),
        }
        line.append(&count);
        row.set_child(Some(&line));
        row.set_selectable(*enabled);
        row.set_activatable(*enabled);
        if !enabled {
            row.set_sensitive(false);
            row.set_tooltip_text(Some("Playlists coming soon"));
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

fn pill(text: &str) -> gtk::Label {
    let pill = label(text, "wave-marker");
    pill.set_xalign(0.5);
    pill
}

fn icon_button(icon_name: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::builder()
        .icon_name(icon_name)
        .tooltip_text(tooltip)
        .build();
    button.add_css_class("icon-button");
    button
}

fn shuffle_button() -> (gtk::Button, gtk::Label) {
    let button = gtk::Button::builder()
        .icon_name("media-playlist-shuffle-symbolic")
        .tooltip_text("Shuffle")
        .build();
    button.add_css_class("icon-button");
    button.add_css_class("toolbar-button");
    button.add_css_class("shuffle-toggle");
    button.add_css_class("shuffle-off");

    let status = gtk::Label::new(Some("Off"));
    status.add_css_class("shuffle-state-label");

    (button, status)
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

    if playback_reached_end(state) {
        advance_after_track_end(state);
        return;
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
}

fn playback_reached_end(state: &Rc<RefCell<UiState>>) -> bool {
    state
        .borrow_mut()
        .playback
        .as_mut()
        .map(PlaybackEngine::take_end_of_stream)
        .unwrap_or(false)
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
        let transition_index = ui
            .tracks
            .iter()
            .position(|track| track.item_id.as_deref() == Some(transition.item_id.as_str()))
            .or_else(|| next_playback_index(&ui));

        if let Some(index) = transition_index {
            ui.selected_index = index;
        }

        if let Some(track) = ui.tracks.get(ui.selected_index).cloned() {
            let quality = track.quality.clone();
            ui.now_playing_key = Some(track_key(&track));
            update_now_playing_labels(&ui);
            ui.playback_status.set_text(&format!("Playing | {quality}"));
        } else {
            ui.now_playing_key = None;
            ui.playback_status.set_text("Playing next stream");
        }

        arm_gapless_next(&mut ui);
        update_play_button(&ui);
        update_mpris_metadata(&mut ui);
        update_mpris_status(&mut ui);
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
            ui.now_playing_key = None;
            ui.playback_status.set_text("Up Next finished");
            update_play_button(&ui);
            update_mpris_status(&mut ui);
        }
        update_list_indicators(state);
    }
}

fn load_selected_waveform(state: &Rc<RefCell<UiState>>) {
    let (key, stream_url, area, status, waveform) = {
        let ui = state.borrow();
        let track = ui.tracks.get(ui.selected_index);
        let key = track.and_then(|track| {
            Some(WaveformKey {
                item_id: track.item_id.clone()?,
                media_source_id: track.media_source_id.clone()?,
            })
        });
        let stream_url = track.and_then(|track| track.stream_url.clone());
        (
            key,
            stream_url,
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
        let result = crate::waveform::load_or_generate(request_key, &stream_url);
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
