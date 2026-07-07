use super::prelude::*;

#[derive(Clone, Debug)]
pub(crate) struct ConnectionPayload {
    pub(crate) session: JellyfinSession,
    pub(crate) tracks: Vec<UiTrack>,
    pub(crate) playlists: Vec<UiPlaylist>,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub(crate) struct CachedLibrary {
    pub(crate) tracks: Vec<UiTrack>,
    #[serde(default)]
    pub(crate) playlists: Vec<UiPlaylist>,
}

pub(crate) enum ConnectionMessage {
    Authenticated(JellyfinSession),
    Status(String),
    Progress { loaded: usize, total: Option<usize> },
    Finished(Result<ConnectionPayload, String>),
}

pub(crate) static CONNECTION_GENERATION: AtomicU64 = AtomicU64::new(0);

pub(crate) static CACHE_RESET_LOCK: Mutex<()> = Mutex::new(());

#[allow(deprecated)]
pub(crate) fn confirm_database_reset(parent: &gtk::Window, state: Rc<RefCell<UiState>>) {
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

pub(crate) fn reset_database_and_cache(state: Rc<RefCell<UiState>>) {
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

pub(crate) fn apply_first_time_setup_state(state: &Rc<RefCell<UiState>>) {
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

pub(crate) fn connection_card(state: Rc<RefCell<UiState>>) -> gtk::Box {
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

pub(crate) fn poll_connection_result(
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

pub(crate) fn refresh_jellyfin_library(state: Rc<RefCell<UiState>>, button: gtk::Button) {
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

pub(crate) fn set_refresh_button_connected_state(state: &Rc<RefCell<UiState>>) {
    let (button, connected) = {
        let ui = state.borrow();
        (ui.refresh_button.clone(), ui.jellyfin_connected)
    };
    if let Some(button) = button.as_ref() {
        button.set_sensitive(connected);
    }
}

pub(crate) fn set_reconnect_button_needed(state: &Rc<RefCell<UiState>>, needed: bool) {
    if let Some(button) = state.borrow().reconnect_button.as_ref() {
        button.set_visible(needed);
        button.set_sensitive(needed);
    }
}

pub(crate) fn set_reconnect_button_error_state(state: &Rc<RefCell<UiState>>, error: &str) {
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

pub(crate) fn error_needs_reconnect(error: &str) -> bool {
    error.contains("reconnect")
}

pub(crate) fn set_library_loading(state: &Rc<RefCell<UiState>>, message: &str) {
    let ui = state.borrow();
    ui.connection_status.set_text("Loading library");
    ui.connection_detail.set_text(message);
    ui.page_summary.set_text(message);
    if let Some(spinner) = ui.sync_spinner.as_ref() {
        spinner.set_visible(true);
        spinner.start();
    }
}

pub(crate) fn set_library_loaded(state: &Rc<RefCell<UiState>>) {
    let ui = state.borrow();
    if let Some(spinner) = ui.sync_spinner.as_ref() {
        spinner.stop();
        spinner.set_visible(false);
    }
}

pub(crate) fn library_progress_text(loaded: usize, total: Option<usize>) -> String {
    match total {
        Some(total) if total > 0 => format!("Loading full library: {loaded} of {total} tracks"),
        _ => format!("Loading full library: {loaded} tracks"),
    }
}

pub(crate) fn apply_connection_payload(state: &Rc<RefCell<UiState>>, payload: ConnectionPayload) {
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
        let ui = &mut *ui;
        ui.playback_session
            .reconcile_library_refresh(&ui.all_tracks, track_key);
        apply_track_filter(ui, selected_key.as_deref());
        update_now_playing_labels(ui);
        update_play_button(ui);
        update_page_summary(ui);
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

pub(crate) fn connect_and_fetch(
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

pub(crate) fn reconnect_and_fetch(
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

pub(crate) fn reconnect_and_fetch_payload(
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

pub(crate) fn connect_and_fetch_payload(
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

pub(crate) fn fetch_saved_session(
    session: JellyfinSession,
    sender: mpsc::Sender<ConnectionMessage>,
    generation: u64,
) {
    let result = fetch_saved_session_payload(session, Some(sender.clone()), generation);
    let _ = sender.send(ConnectionMessage::Finished(result));
}

pub(crate) fn refresh_saved_session(sender: mpsc::Sender<ConnectionMessage>, generation: u64) {
    let result = refresh_saved_session_payload(Some(sender.clone()), generation);
    let _ = sender.send(ConnectionMessage::Finished(result));
}

pub(crate) fn refresh_saved_session_payload(
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

pub(crate) fn fetch_saved_session_payload(
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

pub(crate) fn ensure_connection_generation_current(generation: u64) -> Result<(), String> {
    if CONNECTION_GENERATION.load(AtomicOrdering::SeqCst) == generation {
        Ok(())
    } else {
        Err("Connection was reset".to_string())
    }
}

pub(crate) fn fetch_library_for_session(
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

pub(crate) fn fetch_incremental_library_for_session(
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

pub(crate) fn merge_incremental_playlists(
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

pub(crate) fn summaries_missing_change_stamps(summaries: &[JellyfinItemSummary]) -> bool {
    summaries
        .iter()
        .any(|summary| summary.date_last_saved.is_none())
}

pub(crate) fn changed_summary_ids<T>(
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

pub(crate) fn send_connection_status(
    sender: Option<&mpsc::Sender<ConnectionMessage>>,
    message: &str,
) {
    if let Some(sender) = sender {
        let _ = sender.send(ConnectionMessage::Status(message.to_string()));
    }
}

pub(crate) fn describe_jellyfin_error(error: JellyfinClientError) -> String {
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

pub(crate) fn save_library_cache(
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

pub(crate) fn load_library_cache(
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

pub(crate) fn load_cached_library(
    cache: &CacheDatabase,
    key: &str,
) -> Result<Option<CachedLibrary>, crate::cache::CacheError> {
    cache
        .get_setting(key)?
        .map(|json| serde_json::from_str(&json).map_err(crate::cache::CacheError::from))
        .transpose()
}

pub(crate) fn load_cached_tracks(
    cache: &CacheDatabase,
    key: &str,
) -> Result<Option<Vec<UiTrack>>, crate::cache::CacheError> {
    cache
        .get_setting(key)?
        .map(|json| serde_json::from_str(&json).map_err(crate::cache::CacheError::from))
        .transpose()
}

pub(crate) fn hydrate_stream_http_headers(tracks: &mut [UiTrack], session: &JellyfinSession) {
    let headers = stream_http_headers_for_token(Some(&session.access_token));
    for track in tracks {
        track.stream_http_headers = headers.clone();
    }
}

pub(crate) fn hydrate_cached_library(library: &mut CachedLibrary, session: &JellyfinSession) {
    hydrate_stream_http_headers(&mut library.tracks, session);
    hydrate_sidebar_cover_thumbnail_urls(&mut library.tracks);
    for playlist in &mut library.playlists {
        normalize_sidebar_cover_thumbnail_url(&mut playlist.thumbnail_artwork_url);
        hydrate_stream_http_headers(&mut playlist.tracks, session);
        hydrate_sidebar_cover_thumbnail_urls(&mut playlist.tracks);
    }
}

pub(crate) fn hydrate_sidebar_cover_thumbnail_urls(tracks: &mut [UiTrack]) {
    for track in tracks {
        normalize_sidebar_cover_thumbnail_url(&mut track.thumbnail_artwork_url);
    }
}

pub(crate) fn normalize_sidebar_cover_thumbnail_url(url: &mut Option<String>) {
    let Some(existing_url) = url.as_mut() else {
        return;
    };
    if let Some(normalized_url) =
        resized_jellyfin_image_url(existing_url, SIDEBAR_COVER_ART_IMAGE_SIZE)
    {
        *existing_url = normalized_url;
    }
}

pub(crate) fn resized_jellyfin_image_url(url: &str, max_size: u32) -> Option<String> {
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

pub(crate) fn library_needs_album_order_refresh(library: &CachedLibrary) -> bool {
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

pub(crate) fn library_cache_key(session: &JellyfinSession) -> String {
    format!(
        "jellyfin.library.v4.{}.{}",
        library_cache_server_key(session),
        session.user_id
    )
}

pub(crate) fn legacy_library_cache_key_v3(session: &JellyfinSession) -> String {
    format!(
        "jellyfin.library.v3.{}.{}",
        library_cache_server_key(session),
        session.user_id
    )
}

pub(crate) fn legacy_library_cache_key_v2(session: &JellyfinSession) -> String {
    format!(
        "jellyfin.library.{}.{}",
        library_cache_server_key(session),
        session.user_id
    )
}

pub(crate) fn library_cache_server_key(session: &JellyfinSession) -> &str {
    session
        .server_id
        .as_deref()
        .unwrap_or(session.server_url.as_str())
}

#[allow(deprecated)]
pub(crate) fn show_reconnect_dialog(parent: &gtk::Window, state: Rc<RefCell<UiState>>) {
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

pub(crate) fn reconnect_summary_row(name: &str, value: &str) -> gtk::Box {
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
pub(crate) fn poll_reconnect_result(
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
