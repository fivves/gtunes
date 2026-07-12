use super::prelude::*;

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
    let discord_presence_enabled = load_discord_presence_enabled();
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
        discord_presence: if discord_presence_enabled {
            DiscordPresence::from_env()
        } else {
            None
        },
        discord_presence_enabled,
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
