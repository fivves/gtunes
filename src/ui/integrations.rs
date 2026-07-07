use super::prelude::*;

pub(crate) fn setup_mpris(state: Rc<RefCell<UiState>>) {
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

pub(crate) fn poll_mpris_events(
    receiver: mpsc::Receiver<MediaControlEvent>,
    state: Rc<RefCell<UiState>>,
) {
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

pub(crate) fn handle_mpris_event(state: &Rc<RefCell<UiState>>, event: MediaControlEvent) {
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

pub(crate) fn update_mpris_status(ui: &mut UiState) {
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

pub(crate) fn update_mpris_metadata(ui: &mut UiState) {
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

pub(crate) fn sync_external_playback_status(ui: &mut UiState) {
    sync_discord_presence(ui);
    update_mpris_status(ui);
}

pub(crate) fn sync_external_playback_metadata(ui: &mut UiState) {
    sync_discord_presence(ui);
    update_mpris_metadata(ui);
}

pub(crate) fn sync_external_playback(ui: &mut UiState) {
    sync_discord_presence(ui);
    update_mpris_metadata(ui);
    update_mpris_status(ui);
}

pub(crate) fn sync_discord_presence(ui: &UiState) {
    let Some(discord) = ui.discord_presence.as_ref() else {
        return;
    };

    let playback_state = match ui.playback.as_ref().map(PlaybackEngine::state) {
        Some(PlaybackState::Playing) => PresencePlaybackState::Playing,
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
