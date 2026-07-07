use super::prelude::*;

pub(crate) const RADIO_CARD_CONTENT_WIDTH: i32 = 154;

pub(crate) const RADIO_GRID_COLUMN_GAP: i32 = 14;

pub(crate) const RADIO_STATIONS_KEY: &str = "radio.stations";

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
