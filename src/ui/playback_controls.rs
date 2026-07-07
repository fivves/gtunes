use super::prelude::*;

pub(crate) fn restore_persisted_playback(state: &Rc<RefCell<UiState>>) {
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
        let ui = &mut *ui;
        let fallback_selected_index = ui.selected_index;
        let Some(selection) = ui.playback_session.restore_library_playback(
            session::RestoredPlayback {
                tracks: playback_tracks,
                current_index: playback_index,
                playback_order,
                shuffle_enabled: snapshot.shuffle_enabled,
            },
            &ui.tracks,
            fallback_selected_index,
            same_track,
            track_key,
        ) else {
            return;
        };
        ui.selected_index = selection.selected_index;
        update_shuffle_button(ui);
        update_now_playing_labels(ui);
        update_play_button(ui);
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

pub(crate) fn rebuild_playback_order(ui: &mut UiState, start_index: usize) {
    ui.playback_session
        .rebuild_order_for_library(ui.tracks.len(), start_index);
}

pub(crate) fn next_playback_index(ui: &UiState) -> Option<usize> {
    let current_index = ui.playback_session.current_index_or(ui.selected_index);
    ui.playback_session.next_index(current_index)
}

pub(crate) fn previous_playback_index(ui: &UiState) -> Option<usize> {
    let current_index = ui.playback_session.current_index_or(ui.selected_index);
    ui.playback_session.previous_index(current_index)
}

pub(crate) fn random_index(len: usize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_usize(len);
    Some((hasher.finish() as usize) % len)
}

pub(crate) fn play_random_for_page(state: &Rc<RefCell<UiState>>, page: LibraryPage) {
    match page {
        LibraryPage::Tracks => {
            let count = state.borrow().tracks.len();
            if let Some(index) = random_index(count) {
                play_track_at(state, index);
            }
        }
        LibraryPage::Albums => {
            let album = {
                let ui = state.borrow();
                random_index(ui.library_albums.len())
                    .and_then(|i| ui.library_albums.get(i))
                    .cloned()
            };
            if let Some(album) = album {
                show_album_tracks(state, &album);
                play_track_at(state, 0);
            }
        }
        LibraryPage::Artists => {
            let album = {
                let ui = state.borrow();
                random_index(ui.library_artists.len()).and_then(|i| {
                    let artist = &ui.library_artists[i];
                    let albums =
                        album_summaries_for_artist_from(&ui.library_albums, &artist.key, "");
                    random_index(albums.len()).and_then(|j| albums.into_iter().nth(j))
                })
            };
            if let Some(album) = album {
                show_album_tracks(state, &album);
                play_track_at(state, 0);
            }
        }
        LibraryPage::Playlists => {
            let playlist = {
                let ui = state.borrow();
                random_index(ui.playlists.len())
                    .and_then(|i| ui.playlists.get(i))
                    .cloned()
            };
            if let Some(playlist) = playlist {
                show_playlist_tracks(state, &playlist);
                play_track_at(state, 0);
            }
        }
        LibraryPage::Radio => {
            let stations = radio_stations_for_display(state);
            if let Some(idx) = random_index(stations.len()) {
                play_radio_station(state, &stations[idx]);
            }
        }
        LibraryPage::NextUp => {}
    }
}

pub(crate) fn toggle_shuffle(state: &Rc<RefCell<UiState>>) {
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

pub(crate) fn pause_playback(state: &Rc<RefCell<UiState>>) {
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

pub(crate) fn resume_playback(state: &Rc<RefCell<UiState>>) {
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

pub(crate) fn toggle_play_pause(state: &Rc<RefCell<UiState>>) {
    // In Cast mode, route play/pause to the Cast device
    {
        let mut ui = state.borrow_mut();
        if let Some(session) = ui.cast_session.as_ref() {
            if ui.cast_is_playing {
                session.pause();
                ui.cast_is_playing = false;
            } else {
                session.play();
                ui.cast_is_playing = true;
            }
            update_play_button(&ui);
            return;
        }
    }

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

pub(crate) fn play_track_at_selected_index(state: &Rc<RefCell<UiState>>) {
    let index = state.borrow().selected_index;
    play_track_at(state, index);
}

pub(crate) fn play_previous_track(state: &Rc<RefCell<UiState>>) {
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

pub(crate) fn play_next_track(state: &Rc<RefCell<UiState>>) {
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

pub(crate) fn play_track_at(state: &Rc<RefCell<UiState>>, index: usize) {
    play_track_at_with_order(state, index, true);
}

pub(crate) fn play_track_at_existing_order(state: &Rc<RefCell<UiState>>, index: usize) {
    play_track_at_with_order(state, index, false);
}

pub(crate) fn play_track_at_with_order(
    state: &Rc<RefCell<UiState>>,
    index: usize,
    rebuild_order: bool,
) {
    let (selected_index, visible_index) = {
        let mut ui = state.borrow_mut();
        let ui = &mut *ui;
        let selection =
            ui.playback_session
                .select_library_track(&ui.tracks, index, rebuild_order, same_track);
        ui.selected_index = selection.selected_index;

        update_now_playing_labels(ui);
        update_play_button(ui);
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

pub(crate) fn play_selected_track(state: &Rc<RefCell<UiState>>) {
    // If a Chromecast session is active, load the new track on the device instead
    let cast_url_and_type: Option<(String, String, f64)> = {
        let ui = state.borrow();
        if ui.cast_session.is_some() {
            let track = ui
                .playback_session
                .queue_index
                .and_then(|i| ui.playback_session.queue_tracks.get(i));
            track.and_then(|t| {
                let url = t
                    .stream_url
                    .clone()
                    .or_else(|| t.fallback_stream_url.clone())?;
                let ct = cast_content_type(&t.quality).to_string();
                let dur = parse_duration_str(&t.duration);
                Some((url, ct, dur))
            })
        } else {
            None
        }
    };

    if let Some((url, ct, dur)) = cast_url_and_type {
        let mut ui = state.borrow_mut();
        if let Some(session) = ui.cast_session.as_ref() {
            session.load(url, ct, 0.0);
            ui.cast_position_secs = 0.0;
            ui.cast_duration_secs = dur;
            ui.cast_is_playing = true;
        }
        // Update now_playing_key so the header shows the new track
        if let Some(track) = ui
            .playback_session
            .queue_index
            .and_then(|i| ui.playback_session.queue_tracks.get(i))
            .cloned()
        {
            ui.playback_session
                .start_library_playback(track_key(&track));
        }
        update_play_button(&ui);
        update_now_playing_labels(&ui);
        drop(ui);
        update_list_indicators(state);
        load_selected_cover_art(state);
        load_selected_waveform(state);
        return;
    }

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

pub(crate) fn playback_request_for_track(track: &UiTrack) -> Option<PlaybackRequest> {
    playback_request_for_track_kind(track, PlaybackStreamKind::Direct)
}

pub(crate) fn playback_request_for_track_kind(
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

pub(crate) fn next_gapless_request(ui: &UiState) -> Option<PlaybackRequest> {
    let next_index = next_playback_index(ui)?;
    playback_request_for_track(ui.playback_session.queue_tracks.get(next_index)?)
}

pub(crate) fn arm_gapless_next(ui: &mut UiState) {
    let request = next_gapless_request(ui);
    if let Some(playback) = ui.playback.as_mut() {
        playback.set_next(request);
    }
}

pub(crate) fn stop_playback(ui: &mut UiState) {
    ui.playback_session.reset_to_library();
    if let Some(playback) = ui.playback.as_mut()
        && let Err(error) = playback.stop()
    {
        ui.playback_status
            .set_text(&format!("Stop failed: {error}"));
    }
    sync_external_playback_status(ui);
}

pub(crate) fn start_playback_timer(state: &Rc<RefCell<UiState>>) {
    let state = state.clone();
    gtk::glib::timeout_add_local(Duration::from_millis(250), move || {
        update_playback_position(&state);
        gtk::glib::ControlFlow::Continue
    });
}

pub(crate) fn update_playback_position(state: &Rc<RefCell<UiState>>) {
    // In Cast mode, drain Cast events and update progress from device position
    if update_cast_playback_position(state) {
        return;
    }

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
            if let Some(playback) = ui.playback.as_mut()
                && playback.is_buffering()
            {
                playback.clear_initial_loading();
            }
        }
        save_playback_snapshot_if_due(&mut state.borrow_mut());
    }
}

pub(crate) fn take_playback_event(state: &Rc<RefCell<UiState>>) -> Option<PlaybackEvent> {
    state
        .borrow_mut()
        .playback
        .as_mut()
        .and_then(PlaybackEngine::take_playback_event)
}

pub(crate) fn handle_playback_event(state: &Rc<RefCell<UiState>>, event: PlaybackEvent) {
    match event {
        PlaybackEvent::EndOfStream => advance_after_track_end(state),
        PlaybackEvent::Error {
            item_id,
            stream_kind,
            message,
        } => handle_playback_error(state, item_id, stream_kind, message),
    }
}

pub(crate) fn handle_playback_error(
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

pub(crate) fn apply_gapless_transition(state: &Rc<RefCell<UiState>>) -> bool {
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
                .position(|visible| same_track(visible, &track))
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

pub(crate) fn advance_after_track_end(state: &Rc<RefCell<UiState>>) {
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
