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
