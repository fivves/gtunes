use super::prelude::*;

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
