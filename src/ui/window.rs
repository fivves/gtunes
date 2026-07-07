use super::prelude::*;

#[derive(Clone, Debug)]
pub(crate) struct WaveformVisual {
    pub(crate) peaks: Vec<f32>,
    pub(crate) progress: f64,
    pub(crate) loaded_key: Option<WaveformKey>,
    pub(crate) loading_key: Option<WaveformKey>,
}

pub(crate) const PLAYER_ACTION_EDGE_INSET: i32 = 0;

pub(crate) const ACTION_PANEL_WIDTH: i32 = 130;

pub(crate) const ALBUM_ART_SIZE: i32 = 168;

pub(crate) const COLLECTION_TILE_WIDTH: i32 = 184;

pub(crate) const ARTIST_ART_SIZE: i32 = 148;

pub(crate) const RADIO_CARD_CONTENT_WIDTH: i32 = 154;

pub(crate) const RADIO_GRID_COLUMN_GAP: i32 = 14;

pub(crate) const COLLECTION_TILE_INITIAL_BATCH: usize = 24;

pub(crate) const COLLECTION_TILE_IDLE_BATCH: usize = 24;

pub(crate) const COLLECTION_RETURN_HIGHLIGHT_MS: u64 = 850;

pub(crate) const RADIO_STATIONS_KEY: &str = "radio.stations";

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
        cast_button: None,
        cast_device_box: None,
        cast_status_label: None,
        cast_scan_spinner: None,
        active_cast_device: None,
        last_cast_device: None,
        last_cast_devices: Vec::new(),
        cast_session: None,
        cast_is_playing: false,
        cast_position_secs: 0.0,
        cast_duration_secs: 0.0,
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

pub(crate) fn connect_window_close_request(
    window: &adw::ApplicationWindow,
    state: Rc<RefCell<UiState>>,
) {
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

pub(crate) fn focus_collection_item(state: &Rc<RefCell<UiState>>, index: usize) {
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

pub(crate) fn has_collection_detail_open(state: &Rc<RefCell<UiState>>) -> bool {
    let ui = state.borrow();
    ui.album_filter.is_some() || ui.artist_filter.is_some() || ui.playlist_filter.is_some()
}

pub(crate) fn scroll_active_collection_grid_to_top(state: &Rc<RefCell<UiState>>) {
    let Some(grid) = active_collection_grid(state) else {
        return;
    };

    if let Some(scroll) = collection_grid_scroll(&grid) {
        scroll.vadjustment().set_value(0.0);
    }
}

pub(crate) fn collection_grid_scroll(grid: &gtk::FlowBox) -> Option<gtk::ScrolledWindow> {
    let mut parent = grid.parent();
    while let Some(widget) = parent {
        if let Ok(scroll) = widget.clone().downcast::<gtk::ScrolledWindow>() {
            return Some(scroll);
        }
        parent = widget.parent();
    }
    None
}

pub(crate) fn activate_focused_collection_item(state: &Rc<RefCell<UiState>>) -> bool {
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

pub(crate) fn collection_child_button(child: &gtk::FlowBoxChild) -> Option<gtk::Button> {
    child
        .first_child()
        .and_then(|widget| widget.downcast::<gtk::Button>().ok())
}

pub(crate) fn active_collection_grid(state: &Rc<RefCell<UiState>>) -> Option<gtk::FlowBox> {
    let ui = state.borrow();
    match visible_library_content(&ui) {
        VisibleLibraryContent::Albums => ui.album_grid.clone(),
        VisibleLibraryContent::Artists => ui.artist_grid.clone(),
        VisibleLibraryContent::Playlists => ui.playlist_grid.clone(),
        VisibleLibraryContent::Radio => None,
        VisibleLibraryContent::Tracks | VisibleLibraryContent::NextUp => None,
    }
}

pub(crate) fn active_collection_grid_and_content(
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

pub(crate) fn collection_return_target_for_key(
    state: &Rc<RefCell<UiState>>,
    key: String,
) -> Option<CollectionReturnTarget> {
    let (content, _) = active_collection_grid_and_content(state)?;
    Some(CollectionReturnTarget { content, key })
}

pub(crate) fn save_active_collection_scroll_position(state: &Rc<RefCell<UiState>>) {
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

pub(crate) fn restore_active_collection_scroll_position(state: &Rc<RefCell<UiState>>) {
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

pub(crate) fn set_collection_scroll_value(
    ui: &mut UiState,
    content: VisibleLibraryContent,
    value: f64,
) {
    match content {
        VisibleLibraryContent::Albums => ui.album_grid_scroll_value = value,
        VisibleLibraryContent::Artists => ui.artist_grid_scroll_value = value,
        VisibleLibraryContent::Playlists => ui.playlist_grid_scroll_value = value,
        VisibleLibraryContent::Radio
        | VisibleLibraryContent::Tracks
        | VisibleLibraryContent::NextUp => {}
    }
}

pub(crate) fn collection_scroll_value(ui: &UiState, content: VisibleLibraryContent) -> f64 {
    match content {
        VisibleLibraryContent::Albums => ui.album_grid_scroll_value,
        VisibleLibraryContent::Artists => ui.artist_grid_scroll_value,
        VisibleLibraryContent::Playlists => ui.playlist_grid_scroll_value,
        VisibleLibraryContent::Radio
        | VisibleLibraryContent::Tracks
        | VisibleLibraryContent::NextUp => 0.0,
    }
}

pub(crate) fn restore_collection_scroll(scroll: gtk::ScrolledWindow, value: f64) {
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

pub(crate) fn pulse_collection_return_target(
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

pub(crate) fn collection_return_target_button(
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

pub(crate) fn collection_grid_for_content(
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

pub(crate) fn collection_return_target_index(
    ui: &UiState,
    target: &CollectionReturnTarget,
) -> Option<usize> {
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

pub(crate) fn build_player_bar(state: Rc<RefCell<UiState>>) -> gtk::Box {
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

    let cast = cast_menu_button(state.clone());
    utility_row.append(&cast);

    let settings = settings_menu_button(state.clone());
    utility_row.append(&settings);

    actions.add_overlay(&utility_row);

    player.append(&actions);
    connect_player_bar_responsive_layout(&player, &actions, &state.borrow().playback_status);

    player
}

pub(crate) fn settings_menu_button(state: Rc<RefCell<UiState>>) -> gtk::MenuButton {
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

pub(crate) fn cast_menu_button(state: Rc<RefCell<UiState>>) -> gtk::MenuButton {
    let btn = gtk::MenuButton::builder()
        .icon_name("send-to-symbolic")
        .tooltip_text("Cast to device")
        .build();
    btn.add_css_class("icon-button");
    btn.add_css_class("toolbar-button");
    btn.add_css_class("cast-menu-button");

    let popover = gtk::Popover::new();
    popover.add_css_class("cast-popover");

    let outer = gtk::Box::new(Orientation::Vertical, 0);
    outer.set_margin_top(8);
    outer.set_margin_bottom(8);
    outer.set_margin_start(8);
    outer.set_margin_end(8);
    outer.set_width_request(260);

    // Header row
    let header = gtk::Box::new(Orientation::Horizontal, 8);
    header.set_margin_bottom(6);
    let header_label = label("Cast Audio", "cast-popover-title");
    header_label.set_hexpand(true);
    header_label.set_halign(Align::Start);
    header.append(&header_label);

    let spinner = gtk::Spinner::new();
    spinner.add_css_class("cast-scan-spinner");
    header.append(&spinner);
    outer.append(&header);

    // Status label (shown when casting is active)
    let status = label("", "cast-status");
    status.set_halign(Align::Start);
    status.set_margin_bottom(4);
    status.set_visible(false);
    outer.append(&status);

    // Device list box
    let device_box = gtk::Box::new(Orientation::Vertical, 2);
    outer.append(&device_box);

    // Placeholder while scanning
    let scanning = label("Scanning for devices…", "meta");
    scanning.set_margin_top(6);
    scanning.set_margin_bottom(4);
    device_box.append(&scanning);

    {
        let mut ui = state.borrow_mut();
        ui.cast_button = Some(btn.clone());
        ui.cast_device_box = Some(device_box.clone());
        ui.cast_status_label = Some(status.clone());
        ui.cast_scan_spinner = Some(spinner.clone());
    }

    popover.set_child(Some(&outer));
    btn.set_popover(Some(&popover));

    // When popover opens, start scanning
    {
        let state = state.clone();
        popover.connect_show(move |_| {
            start_cast_scan(&state);
        });
    }

    // Right-click = toggle: disconnect if active, reconnect to last device if not
    {
        let right_click = gtk::GestureClick::new();
        right_click.set_button(3);
        let state = state.clone();
        right_click.connect_pressed(move |gesture, _, _, _| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            let (active, last) = {
                let ui = state.borrow();
                (ui.active_cast_device.clone(), ui.last_cast_device.clone())
            };
            if let Some(device) = active {
                stop_cast(&state, &device);
            } else if let Some(device) = last {
                start_cast(&state, device);
            }
        });
        btn.add_controller(right_click);
    }

    btn
}

pub(crate) fn start_cast_scan(state: &Rc<RefCell<UiState>>) {
    // Clear device list and show scanning indicator
    {
        let ui = state.borrow();
        if let Some(device_box) = ui.cast_device_box.as_ref() {
            while let Some(child) = device_box.first_child() {
                device_box.remove(&child);
            }
            let scanning = label("Scanning for devices…", "meta");
            scanning.set_margin_top(6);
            scanning.set_margin_bottom(4);
            device_box.append(&scanning);
        }
        if let Some(spinner) = ui.cast_scan_spinner.as_ref() {
            spinner.start();
        }
        // Hide status label during scan
        if let Some(s) = ui.cast_status_label.as_ref() {
            s.set_visible(false);
        }
    }

    let (tx, rx) = mpsc::channel::<Vec<CastDevice>>();
    std::thread::spawn(move || {
        let devices = cast::discover_devices();
        let _ = tx.send(devices);
    });

    let state = state.clone();
    gtk::glib::timeout_add_local(Duration::from_millis(100), move || {
        match rx.try_recv() {
            Ok(devices) => {
                populate_cast_device_list(&state, devices);
                gtk::glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => {
                // Discovery thread panicked - show empty state
                populate_cast_device_list(&state, vec![]);
                gtk::glib::ControlFlow::Break
            }
        }
    });
}

pub(crate) fn populate_cast_device_list(state: &Rc<RefCell<UiState>>, devices: Vec<CastDevice>) {
    let (device_box, active_id) = {
        let mut ui = state.borrow_mut();
        if let Some(spinner) = ui.cast_scan_spinner.as_ref() {
            spinner.stop();
        }
        ui.last_cast_devices = devices.clone();
        (
            ui.cast_device_box.clone(),
            ui.active_cast_device.as_ref().map(|d| d.id.clone()),
        )
    };
    let Some(device_box) = device_box else {
        return;
    };
    render_cast_device_list(state, &device_box, &devices, active_id.as_deref());
}

pub(crate) fn render_cast_device_list(
    state: &Rc<RefCell<UiState>>,
    device_box: &gtk::Box,
    devices: &[CastDevice],
    active_id: Option<&str>,
) {
    while let Some(child) = device_box.first_child() {
        device_box.remove(&child);
    }

    if devices.is_empty() {
        let empty = label("No devices found on network", "meta");
        empty.set_margin_top(6);
        empty.set_margin_bottom(4);
        device_box.append(&empty);
        return;
    }

    // Group by kind
    let upnp: Vec<_> = devices
        .iter()
        .filter(|d| d.kind == CastDeviceKind::UPnP)
        .collect();
    let chromecasts: Vec<_> = devices
        .iter()
        .filter(|d| d.kind == CastDeviceKind::Chromecast)
        .collect();
    let had_upnp = !upnp.is_empty();

    if had_upnp {
        let section = label("UPnP / DLNA", "cast-section-label");
        section.set_margin_top(4);
        section.set_margin_bottom(2);
        device_box.append(&section);
        for device in upnp {
            let row = cast_device_row(state, device, active_id);
            device_box.append(&row);
        }
    }

    if !chromecasts.is_empty() {
        let section = label("Chromecast", "cast-section-label");
        section.set_margin_top(if had_upnp { 10 } else { 4 });
        section.set_margin_bottom(2);
        device_box.append(&section);
        for device in chromecasts {
            let row = cast_device_row(state, device, active_id);
            device_box.append(&row);
        }
    }
}

pub(crate) fn cast_device_row(
    state: &Rc<RefCell<UiState>>,
    device: &CastDevice,
    active_id: Option<&str>,
) -> gtk::Box {
    let row = gtk::Box::new(Orientation::Horizontal, 8);
    row.add_css_class("cast-device-row");
    row.set_margin_top(2);
    row.set_margin_bottom(2);

    let icon_name = match device.kind {
        CastDeviceKind::UPnP => "audio-speakers-symbolic",
        CastDeviceKind::Chromecast => "send-to-symbolic",
    };
    let icon = gtk::Image::from_icon_name(icon_name);
    icon.add_css_class("cast-device-icon");
    row.append(&icon);

    let name_label = label(&device.name, "cast-device-name");
    name_label.set_hexpand(true);
    name_label.set_halign(Align::Start);
    name_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    row.append(&name_label);

    let is_active = active_id == Some(device.id.as_str());
    let action_btn = gtk::Button::new();
    action_btn.add_css_class("flat");
    action_btn.add_css_class("cast-action-btn");

    if is_active {
        action_btn.set_label("Stop");
        action_btn.add_css_class("destructive-action");
        let state = state.clone();
        let device = device.clone();
        action_btn.connect_clicked(move |_| {
            stop_cast(&state, &device);
        });
    } else {
        action_btn.set_label("Cast");
        let state = state.clone();
        let device = device.clone();
        action_btn.connect_clicked(move |_| {
            start_cast(&state, device.clone());
        });
    }

    row.append(&action_btn);
    row
}

pub(crate) fn cast_content_type(quality: &str) -> &'static str {
    let q = quality.to_uppercase();
    if q.contains("FLAC") {
        "audio/flac"
    } else if q.contains("MP3") {
        "audio/mpeg"
    } else if q.contains("AAC") || q.contains("M4A") {
        "audio/aac"
    } else if q.contains("OGG") || q.contains("OPUS") {
        "audio/ogg"
    } else if q.contains("WAV") {
        "audio/wav"
    } else {
        "audio/mpeg"
    }
}

pub(crate) fn radio_stream_content_type(url: &url::Url) -> &'static str {
    let path = url.path().to_lowercase();
    if path.ends_with(".m3u8") || path.ends_with(".m3u") {
        return "application/x-mpegURL";
    }
    if path.ends_with(".aac") {
        return "audio/aac";
    }
    if path.ends_with(".ogg") || path.ends_with(".opus") {
        return "audio/ogg";
    }
    if path.ends_with(".flac") {
        return "audio/flac";
    }
    // YouTube URLs encode MIME in the query string (e.g. mime=audio%2Fmp4)
    let query = url.query().unwrap_or("").to_lowercase();
    if query.contains("mime=audio%2fmp4") || query.contains("mime=audio/mp4") {
        return "audio/mp4";
    }
    if query.contains("mime=audio%2fwebm") || query.contains("mime=audio/webm") {
        return "audio/webm";
    }
    "audio/mpeg"
}

pub(crate) fn start_cast(state: &Rc<RefCell<UiState>>, device: CastDevice) {
    // Gather track info and current local playback position
    let (stream_url, content_type, duration_secs, local_position_secs) = {
        let ui = state.borrow();
        let track = ui
            .playback_session
            .queue_index
            .and_then(|i| ui.playback_session.queue_tracks.get(i));
        let Some(track) = track else {
            show_cast_status(state, "No track selected");
            return;
        };
        let url = track
            .stream_url
            .clone()
            .or_else(|| track.fallback_stream_url.clone());
        let Some(url) = url else {
            show_cast_status(state, "Stream URL unavailable");
            return;
        };
        let ct = cast_content_type(&track.quality).to_string();
        let dur = parse_duration_str(&track.duration);
        let pos = ui
            .playback
            .as_ref()
            .and_then(|p| p.position())
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        (url, ct, dur, pos)
    };

    // Stop local GStreamer playback
    {
        let mut ui = state.borrow_mut();
        if let Some(playback) = ui.playback.as_mut() {
            let _ = playback.stop();
        }
    }

    show_cast_status(state, &format!("Connecting to {}…", device.name));

    match device.kind {
        CastDeviceKind::UPnP => {
            // UPnP: fire-and-forget (no session tracking)
            let (tx, rx) = mpsc::channel::<Result<(), String>>();
            let dev = device.clone();
            let url = stream_url.clone();
            std::thread::spawn(move || {
                let _ = tx.send(cast::upnp_play(&dev, &url, ""));
            });
            let state = state.clone();
            let dev_name = device.name.clone();
            gtk::glib::timeout_add_local(Duration::from_millis(100), move || match rx.try_recv() {
                Ok(Ok(())) => {
                    {
                        let mut ui = state.borrow_mut();
                        ui.active_cast_device = Some(device.clone());
                        if let Some(btn) = ui.cast_button.as_ref() {
                            btn.add_css_class("cast-active");
                        }
                    }
                    show_cast_status(&state, &format!("Casting to {dev_name}"));
                    refresh_cast_device_list(&state);
                    gtk::glib::ControlFlow::Break
                }
                Ok(Err(e)) => {
                    show_cast_status(&state, &format!("Error: {e}"));
                    gtk::glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
                Err(_) => {
                    show_cast_status(&state, "Connection failed");
                    gtk::glib::ControlFlow::Break
                }
            });
        }
        CastDeviceKind::Chromecast => {
            match cast::CastSession::connect(device.clone()) {
                Ok(session) => {
                    session.load(stream_url, content_type, local_position_secs);
                    {
                        let mut ui = state.borrow_mut();
                        ui.cast_session = Some(session);
                        ui.active_cast_device = Some(device.clone());
                        ui.cast_is_playing = true;
                        ui.cast_position_secs = local_position_secs;
                        ui.cast_duration_secs = duration_secs;
                        if let Some(btn) = ui.cast_button.as_ref() {
                            btn.add_css_class("cast-active");
                        }
                        update_play_button(&ui);
                    }
                    show_cast_status(state, &format!("Casting to {}", device.name));
                    refresh_cast_device_list(state);
                }
                Err(e) => {
                    show_cast_status(state, &format!("Error: {e}"));
                    // Restart local playback since we stopped it
                    play_selected_track(state);
                }
            }
        }
    }
}

pub(crate) fn stop_cast(state: &Rc<RefCell<UiState>>, device: &CastDevice) {
    let resume_secs = {
        let mut ui = state.borrow_mut();
        let pos = ui.cast_position_secs;
        // Send Stop command (Chromecast) or fire-and-forget stop (UPnP)
        if let Some(session) = ui.cast_session.take() {
            session.stop();
        } else if device.kind == CastDeviceKind::UPnP {
            let dev = device.clone();
            std::thread::spawn(move || {
                let _ = cast::upnp_stop(&dev);
            });
        }
        ui.last_cast_device = ui.active_cast_device.take();
        ui.cast_is_playing = false;
        ui.cast_position_secs = 0.0;
        ui.cast_duration_secs = 0.0;
        if let Some(btn) = ui.cast_button.as_ref() {
            btn.remove_css_class("cast-active");
        }
        if let Some(s) = ui.cast_status_label.as_ref() {
            s.set_visible(false);
        }
        update_play_button(&ui);
        pos
    };
    refresh_cast_device_list(state);
    // Resume local playback at the position where Cast left off
    play_selected_track(state);
    if resume_secs > 0.5 {
        let mut ui = state.borrow_mut();
        if let Some(playback) = ui.playback.as_mut() {
            let _ = playback.seek(Duration::from_secs_f64(resume_secs));
        }
    }
}

pub(crate) fn show_cast_status(state: &Rc<RefCell<UiState>>, msg: &str) {
    let ui = state.borrow();
    if let Some(s) = ui.cast_status_label.as_ref() {
        s.set_text(msg);
        s.set_visible(!msg.is_empty());
    }
}

pub(crate) fn refresh_cast_device_list(state: &Rc<RefCell<UiState>>) {
    let (device_box, devices, active_id) = {
        let ui = state.borrow();
        (
            ui.cast_device_box.clone(),
            ui.last_cast_devices.clone(),
            ui.active_cast_device.as_ref().map(|d| d.id.clone()),
        )
    };
    let Some(device_box) = device_box else {
        return;
    };
    // Nothing scanned yet — keep the current placeholder untouched.
    if devices.is_empty() {
        return;
    }
    render_cast_device_list(state, &device_box, &devices, active_id.as_deref());
}

#[allow(deprecated)]
pub(crate) fn show_keyboard_shortcuts(parent: &gtk::Window) {
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
pub(crate) fn shortcut_row(title: &str, accelerator: &str) -> gtk::ListBoxRow {
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

pub(crate) fn show_about_window(parent: &gtk::Window) {
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

pub(crate) fn quit_application(parent: &gtk::Window, state: &Rc<RefCell<UiState>>) {
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

pub(crate) fn connect_player_bar_responsive_layout(
    player: &gtk::Box,
    actions: &gtk::Overlay,
    playback_status: &gtk::Label,
) {
    // A tick callback would keep the frame clock running at the monitor
    // refresh rate even while idle; a coarse timer is enough to follow
    // window resizes.
    let player = player.downgrade();
    let actions = actions.clone();
    let playback_status = playback_status.clone();
    let last_width = Cell::new(0);
    gtk::glib::timeout_add_local(Duration::from_millis(200), move || {
        let Some(player) = player.upgrade() else {
            return gtk::glib::ControlFlow::Break;
        };
        let width = player.width();
        if width > 0 && width != last_width.get() {
            last_width.set(width);
            actions.set_visible(width >= 560);
            playback_status.set_visible(width >= 720);
        }
        gtk::glib::ControlFlow::Continue
    });
}

pub(crate) fn build_body(state: Rc<RefCell<UiState>>) -> gtk::Box {
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

pub(crate) fn build_sidebar(state: Rc<RefCell<UiState>>) -> (gtk::Box, gtk::Box, gtk::Box) {
    let sidebar = gtk::Box::new(Orientation::Vertical, 4);
    sidebar.add_css_class("sidebar");

    let library_header = label("Library", "section-title");
    library_header.set_cursor_from_name(Some("pointer"));
    library_header.set_tooltip_text(Some("Double-click to play random"));
    {
        let header_state = state.clone();
        let header_gesture = gtk::GestureClick::new();
        header_gesture.connect_pressed(move |_, n_press, _, _| {
            if n_press == 2 {
                let page = header_state.borrow().active_page;
                play_random_for_page(&header_state, page);
            }
        });
        library_header.add_controller(header_gesture);
    }
    sidebar.append(&library_header);
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

pub(crate) fn sidebar_cover_art(state: Rc<RefCell<UiState>>) -> gtk::Box {
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

pub(crate) fn build_content(state: Rc<RefCell<UiState>>) -> gtk::Box {
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

pub(crate) fn detail_header(state: Rc<RefCell<UiState>>) -> gtk::Box {
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

pub(crate) fn album_grid_page(state: Rc<RefCell<UiState>>) -> gtk::ScrolledWindow {
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

pub(crate) fn artist_grid_page(state: Rc<RefCell<UiState>>) -> gtk::ScrolledWindow {
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

pub(crate) fn playlist_grid_page(state: Rc<RefCell<UiState>>) -> gtk::ScrolledWindow {
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

pub(crate) fn radio_page(state: Rc<RefCell<UiState>>) -> gtk::ScrolledWindow {
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

pub(crate) fn refresh_collection_grids(state: &Rc<RefCell<UiState>>) {
    refresh_album_grid(state);
    refresh_artist_grid(state);
    refresh_playlist_grid(state);
    update_nav_counts(state);
}

pub(crate) fn refresh_visible_collection_grid(state: &Rc<RefCell<UiState>>) {
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

pub(crate) fn refresh_album_grid(state: &Rc<RefCell<UiState>>) {
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

pub(crate) fn refresh_artist_grid(state: &Rc<RefCell<UiState>>) {
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

pub(crate) fn refresh_playlist_grid(state: &Rc<RefCell<UiState>>) {
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

pub(crate) fn clear_flow_box(flow: &gtk::FlowBox) {
    while let Some(child) = flow.first_child() {
        flow.remove(&child);
    }
}

pub(crate) fn next_collection_render_generation(state: &Rc<RefCell<UiState>>) -> u64 {
    let mut ui = state.borrow_mut();
    ui.collection_render_generation = ui.collection_render_generation.wrapping_add(1);
    ui.collection_render_generation
}

pub(crate) fn collection_render_generation(state: &Rc<RefCell<UiState>>) -> u64 {
    state.borrow().collection_render_generation
}

pub(crate) fn render_album_tiles_batched(
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

pub(crate) fn render_artist_tiles_batched(
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

pub(crate) fn render_playlist_tiles_batched(
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

pub(crate) fn collection_empty_state(text: &str) -> gtk::Box {
    let empty = gtk::Box::new(Orientation::Vertical, 8);
    empty.add_css_class("collection-empty");
    empty.append(&label(text, "rail-title"));
    empty.append(&label(
        "Try a different search or connect to Jellyfin.",
        "meta",
    ));
    empty
}

pub(crate) fn album_tile(
    album: AlbumSummary,
    state: Rc<RefCell<UiState>>,
    tile_index: usize,
) -> gtk::Button {
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

pub(crate) fn artist_tile(
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

pub(crate) fn playlist_tile(
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

pub(crate) fn collection_tile_button(title: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("collection-tile");
    button.set_halign(Align::Fill);
    button.set_valign(Align::Start);
    button.set_size_request(COLLECTION_TILE_WIDTH, -1);
    button.set_tooltip_text(Some(title));
    button.set_cursor_from_name(Some("pointer"));
    button
}

pub(crate) fn collection_tile_label(text: &str, class_name: &str) -> gtk::Label {
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

pub(crate) fn set_search_query(state: &Rc<RefCell<UiState>>, query: &str) {
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

pub(crate) fn set_library_page(state: &Rc<RefCell<UiState>>, page: LibraryPage) {
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

pub(crate) fn show_album_tracks(state: &Rc<RefCell<UiState>>, album: &AlbumSummary) {
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

pub(crate) fn show_playlist_tracks(state: &Rc<RefCell<UiState>>, playlist: &UiPlaylist) {
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

pub(crate) fn show_artist_albums(state: &Rc<RefCell<UiState>>, artist: &ArtistSummary) {
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

pub(crate) fn focus_active_collection_grid(state: &Rc<RefCell<UiState>>) {
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

pub(crate) fn return_to_collection_grid(state: &Rc<RefCell<UiState>>) {
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

pub(crate) fn navigate_to_now_playing_artist(state: &Rc<RefCell<UiState>>) {
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

pub(crate) fn navigate_to_now_playing_album(state: &Rc<RefCell<UiState>>) {
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

pub(crate) fn update_content_view(state: &Rc<RefCell<UiState>>, direction: NavDirection) {
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

pub(crate) fn update_nav_counts(state: &Rc<RefCell<UiState>>) {
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

pub(crate) fn update_nav_selection(state: &Rc<RefCell<UiState>>) {
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

pub(crate) fn built_in_radio_stations() -> Vec<RadioStation> {
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

pub(crate) fn radio_stations_for_display(state: &Rc<RefCell<UiState>>) -> Vec<RadioStation> {
    let ui = state.borrow();
    radio_stations_for_display_from(&ui.radio_stations)
}

pub(crate) fn radio_stations_for_display_from(
    custom_stations: &[RadioStation],
) -> Vec<RadioStation> {
    let mut stations = built_in_radio_stations();
    stations.extend(
        custom_stations
            .iter()
            .filter(|station| !station.built_in)
            .cloned(),
    );
    stations
}

pub(crate) fn load_radio_stations() -> Vec<RadioStation> {
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

pub(crate) fn save_radio_stations(stations: &[RadioStation]) {
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

pub(crate) fn current_radio_station(ui: &UiState) -> Option<RadioStation> {
    let station_id = ui.playback_session.mode.radio_station_id()?;
    radio_stations_for_display_from(&ui.radio_stations)
        .into_iter()
        .find(|station| station.id == station_id)
}

pub(crate) fn persist_custom_radio_station(
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

pub(crate) fn update_custom_radio_station(
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

pub(crate) fn radio_station_conflicts(
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

pub(crate) fn refresh_radio_page(state: &Rc<RefCell<UiState>>) {
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

pub(crate) fn radio_station_card(
    state: Rc<RefCell<UiState>>,
    station: RadioStation,
) -> gtk::Overlay {
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
            // Each right-click builds a fresh popover; drop it from the widget
            // tree when dismissed instead of accumulating them on the card.
            popover.connect_closed(|popover| {
                let popover = popover.clone();
                gtk::glib::idle_add_local_once(move || {
                    popover.unparent();
                });
            });
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

pub(crate) fn radio_station_subtitle(station: &RadioStation) -> String {
    if station.built_in {
        return "Stream Preset".to_string();
    }

    match station.source_kind() {
        RadioSourceKind::Stream => "Custom Stream".to_string(),
        RadioSourceKind::YouTube => "YouTube live".to_string(),
        RadioSourceKind::Twitch => "Twitch live".to_string(),
    }
}

pub(crate) fn radio_station_form_popover<F>(
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

pub(crate) fn radio_station_edit_popover(
    state: Rc<RefCell<UiState>>,
    station: RadioStation,
) -> gtk::Popover {
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

pub(crate) fn radio_status_badge(text: &str) -> gtk::Label {
    let badge = label(text, "radio-playing-badge");
    badge.set_halign(Align::End);
    badge.set_valign(Align::Start);
    badge
}

pub(crate) fn radio_icon(size: i32) -> gtk::DrawingArea {
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

pub(crate) fn radio_station_icon(icon: &str) -> gtk::Label {
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

pub(crate) fn play_radio_station(state: &Rc<RefCell<UiState>>, station: &RadioStation) {
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

pub(crate) fn resolve_and_play_radio_station(
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

pub(crate) fn play_resolved_radio_station(
    state: &Rc<RefCell<UiState>>,
    station: &RadioStation,
    stream_url: url::Url,
) {
    // When a Cast session is active, send the radio stream to the device
    {
        let ui = state.borrow();
        if let Some(session) = ui.cast_session.as_ref() {
            let content_type = radio_stream_content_type(&stream_url);
            session.load_live(stream_url.to_string(), content_type.into());
            let device_name = ui
                .active_cast_device
                .as_ref()
                .map(|d| d.name.clone())
                .unwrap_or_else(|| "device".into());
            drop(ui);
            let mut ui = state.borrow_mut();
            ui.cast_is_playing = true;
            ui.cast_position_secs = 0.0;
            ui.cast_duration_secs = 0.0; // live — no known duration
            let status = format!("Casting radio to {device_name}");
            set_active_radio_station_ui(&mut ui, station, Some(&status));
            drop(ui);
            update_list_indicators(state);
            refresh_radio_page(state);
            return;
        }
    }

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

pub(crate) fn set_active_radio_station_ui(
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

pub(crate) fn resume_radio_station(state: &Rc<RefCell<UiState>>) -> bool {
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

pub(crate) fn clear_track_visuals_for_radio(ui: &mut UiState) {
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

pub(crate) fn update_page_summary(ui: &UiState) {
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

pub(crate) fn update_now_playing_labels(state: &UiState) {
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

pub(crate) fn radio_playback_status_text(state: &UiState, station: &RadioStation) -> String {
    if let Some(device) = state.active_cast_device.as_ref() {
        return if state.cast_is_playing {
            format!("▶ Casting radio to {}", device.name)
        } else {
            format!("⏸ Paused on {}", device.name)
        };
    }
    match state.playback.as_ref().map(PlaybackEngine::state) {
        Some(PlaybackState::Playing) => format!("Playing radio | {}", station.name),
        Some(PlaybackState::Paused) => format!("Paused radio | {}", station.name),
        Some(PlaybackState::Error(error)) => format!("Radio stream failed: {error}"),
        _ => format!("Radio stream | {}", station.name),
    }
}

pub(crate) fn playback_status_text(state: &UiState, track: &UiTrack) -> String {
    if let Some(device) = state.active_cast_device.as_ref() {
        return if state.cast_is_playing {
            format!("▶ Casting to {} | {}", device.name, track.quality)
        } else {
            format!("⏸ Paused on {}", device.name)
        };
    }

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

pub(crate) fn update_play_button(state: &UiState) {
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

    let is_playing = if state.cast_session.is_some() {
        state.cast_is_playing
    } else {
        matches!(
            state.playback.as_ref().map(PlaybackEngine::state),
            Some(PlaybackState::Playing)
        )
    };

    if is_playing {
        button.set_icon_name("media-playback-pause-symbolic");
        button.set_tooltip_text(Some("Pause"));
    } else {
        button.set_icon_name("media-playback-start-symbolic");
        button.set_tooltip_text(Some("Play"));
    }
}

pub(crate) fn update_shuffle_button(state: &UiState) {
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

pub(crate) fn build_bottom_bar(state: Rc<RefCell<UiState>>) -> gtk::Box {
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

pub(crate) fn nav_list(state: Rc<RefCell<UiState>>) -> gtk::ListBox {
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

    let double_click_state = state.clone();
    let double_click = gtk::GestureClick::new();
    double_click.connect_pressed(move |_, n_press, _, _| {
        if n_press == 2 {
            let page = double_click_state.borrow().active_page;
            play_random_for_page(&double_click_state, page);
        }
    });
    list.add_controller(double_click);

    update_nav_counts(&state);

    list
}

pub(crate) fn update_cast_playback_position(state: &Rc<RefCell<UiState>>) -> bool {
    let has_cast = state.borrow().cast_session.is_some();
    if !has_cast {
        return false;
    }

    // Drain all pending events, keeping the most relevant position update
    let events: Vec<CastEvent> = {
        let ui = state.borrow();
        let mut evs = Vec::new();
        if let Some(session) = ui.cast_session.as_ref() {
            while let Some(ev) = session.try_recv_event() {
                evs.push(ev);
            }
        }
        evs
    };

    let mut track_finished = false;
    let mut disconnected = false;

    for event in events {
        match event {
            CastEvent::Playing { current_time } => {
                let mut ui = state.borrow_mut();
                ui.cast_position_secs = current_time;
                if !ui.cast_is_playing {
                    ui.cast_is_playing = true;
                    update_play_button(&ui);
                    update_now_playing_labels(&ui);
                }
                let pos = current_time;
                let dur = ui.cast_duration_secs;
                update_cast_progress_labels(&ui, pos, dur);
            }
            CastEvent::Paused { current_time } => {
                let mut ui = state.borrow_mut();
                ui.cast_position_secs = current_time;
                if ui.cast_is_playing {
                    ui.cast_is_playing = false;
                    update_play_button(&ui);
                    update_now_playing_labels(&ui);
                }
                let pos = current_time;
                let dur = ui.cast_duration_secs;
                update_cast_progress_labels(&ui, pos, dur);
            }
            CastEvent::TrackFinished => {
                track_finished = true;
            }
            CastEvent::Error(e) => {
                let ui = state.borrow();
                if let Some(s) = ui.cast_status_label.as_ref() {
                    s.set_text(&format!("Cast error: {e}"));
                    s.set_visible(true);
                }
            }
            CastEvent::Disconnected => {
                disconnected = true;
            }
        }
    }

    if disconnected {
        // Cast session ended unexpectedly - clean up without calling stop_cast
        // (which would try to send another Stop command)
        {
            let mut ui = state.borrow_mut();
            ui.cast_session = None;
            ui.active_cast_device = None;
            ui.cast_is_playing = false;
            ui.cast_position_secs = 0.0;
            ui.cast_duration_secs = 0.0;
            if let Some(btn) = ui.cast_button.as_ref() {
                btn.remove_css_class("cast-active");
            }
            if let Some(s) = ui.cast_status_label.as_ref() {
                s.set_visible(false);
            }
            update_play_button(&ui);
        }
        refresh_cast_device_list(state);
        play_selected_track(state);
        return true;
    }

    if track_finished {
        advance_after_track_end(state);
        return true;
    }

    // No event received this tick — advance local timer if playing
    {
        let mut ui = state.borrow_mut();
        if ui.cast_is_playing {
            ui.cast_position_secs += 0.25;
            let pos = ui.cast_position_secs;
            let dur = ui.cast_duration_secs;
            update_cast_progress_labels(&ui, pos, dur);
        }
    }

    true
}

pub(crate) fn update_cast_progress_labels(ui: &UiState, position_secs: f64, duration_secs: f64) {
    let pos = Duration::from_secs_f64(position_secs.max(0.0));
    ui.elapsed_label.set_text(&format_duration(pos));
    if duration_secs > 0.0 {
        let progress = (position_secs / duration_secs).clamp(0.0, 1.0);
        ui.waveform.borrow_mut().progress = progress;
        let remaining = (duration_secs - position_secs).max(0.0);
        ui.remaining_label.set_text(&format!(
            "-{}",
            format_duration(Duration::from_secs_f64(remaining))
        ));
        if let Some(area) = ui.wave_area.as_ref() {
            area.queue_draw();
        }
    }
}

pub(crate) fn load_selected_waveform(state: &Rc<RefCell<UiState>>) {
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

pub(crate) fn apply_waveform_summary(state: &Rc<RefCell<UiState>>, summary: WaveformSummary) {
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

pub(crate) fn seek_waveform(state: &Rc<RefCell<UiState>>, area: &gtk::DrawingArea, x: f64) {
    let width = area.allocated_width().max(1) as f64;
    let progress = (x / width).clamp(0.0, 1.0);
    let mut ui = state.borrow_mut();

    // Cast mode: seek on the Cast device
    if let Some(session) = ui.cast_session.as_ref() {
        let pos_secs = ui.cast_duration_secs * progress;
        session.seek(pos_secs);
        ui.cast_position_secs = pos_secs;
        let dur = ui.cast_duration_secs;
        ui.waveform.borrow_mut().progress = progress;
        ui.elapsed_label
            .set_text(&format_duration(Duration::from_secs_f64(pos_secs)));
        ui.remaining_label.set_text(&format!(
            "-{}",
            format_duration(Duration::from_secs_f64((dur - pos_secs).max(0.0)))
        ));
        if let Some(area) = ui.wave_area.as_ref() {
            area.queue_draw();
        }
        return;
    }

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
            // Scrubbing fires this per drag event; the throttled save avoids a
            // sqlite write for every pixel of movement.
            save_playback_snapshot_if_due(&mut ui);
        }
        Err(error) => ui
            .playback_status
            .set_text(&format!("Seek failed: {error}")),
    }
    if let Some(area) = ui.wave_area.as_ref() {
        area.queue_draw();
    }
}

pub(crate) fn waveform_widget(state: Rc<RefCell<UiState>>) -> gtk::DrawingArea {
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
