use super::prelude::*;

#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize)]
pub(crate) struct LibraryViewSettings {
    pub(crate) sort_column: SortColumn,
    pub(crate) sort_ascending: bool,
}

impl Default for LibraryViewSettings {
    fn default() -> Self {
        Self {
            sort_column: SortColumn::Title,
            sort_ascending: true,
        }
    }
}

pub(crate) fn load_library_view_settings() -> LibraryViewSettings {
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

pub(crate) fn save_library_view_settings(settings: LibraryViewSettings) {
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

pub(crate) const LIBRARY_VIEW_SETTINGS_KEY: &str = "library.view.settings";

pub(crate) const KEEP_PLAYING_WHILE_CLOSED_KEY: &str = "player.keep_playing_while_closed";

pub(crate) const ANIMATIONS_ENABLED_KEY: &str = "ui.animations.enabled";

pub(crate) const FONT_MONO_KEY: &str = "ui.font.mono";

pub(crate) const PLAYBACK_STATE_KEY: &str = "player.playback.state";

pub(crate) const PLAYBACK_SNAPSHOT_INTERVAL: Duration = Duration::from_secs(5);

pub(crate) fn load_keep_playing_while_closed() -> bool {
    match CacheDatabase::open_default()
        .and_then(|cache| cache.get_setting(KEEP_PLAYING_WHILE_CLOSED_KEY))
    {
        Ok(Some(value)) => value == "true",
        Ok(None) => false,
        Err(error) => {
            tracing::warn!(%error, "failed to load close behavior setting");
            false
        }
    }
}

pub(crate) fn set_keep_playing_while_closed(state: &Rc<RefCell<UiState>>, enabled: bool) {
    state.borrow_mut().keep_playing_while_closed = enabled;
    if let Err(error) = CacheDatabase::open_default().and_then(|cache| {
        cache.set_setting(
            KEEP_PLAYING_WHILE_CLOSED_KEY,
            if enabled { "true" } else { "false" },
        )
    }) {
        tracing::warn!(%error, "failed to save close behavior setting");
    }
}

pub(crate) fn load_animations_enabled() -> bool {
    match CacheDatabase::open_default().and_then(|cache| cache.get_setting(ANIMATIONS_ENABLED_KEY))
    {
        Ok(Some(value)) => value != "false",
        Ok(None) => true,
        Err(error) => {
            tracing::warn!(%error, "failed to load animations setting");
            true
        }
    }
}

pub(crate) fn set_animations_enabled(state: &Rc<RefCell<UiState>>, enabled: bool) {
    state.borrow_mut().animations_enabled = enabled;
    if let Some(gtk_settings) = gtk::Settings::default() {
        gtk_settings.set_gtk_enable_animations(enabled);
    }
    if let Err(error) = CacheDatabase::open_default().and_then(|cache| {
        cache.set_setting(
            ANIMATIONS_ENABLED_KEY,
            if enabled { "true" } else { "false" },
        )
    }) {
        tracing::warn!(%error, "failed to save animations setting");
    }
}

pub(crate) fn load_font_mono() -> bool {
    match CacheDatabase::open_default().and_then(|cache| cache.get_setting(FONT_MONO_KEY)) {
        Ok(Some(value)) => value == "true",
        Ok(None) => false,
        Err(error) => {
            tracing::warn!(%error, "failed to load font style setting");
            false
        }
    }
}

pub(crate) fn set_font_mono(state: &Rc<RefCell<UiState>>, mono: bool) {
    state.borrow_mut().font_mono = mono;
    if let Err(error) = CacheDatabase::open_default()
        .and_then(|cache| cache.set_setting(FONT_MONO_KEY, if mono { "true" } else { "false" }))
    {
        tracing::warn!(%error, "failed to save font style setting");
    }
}

pub(crate) fn playback_snapshot(ui: &UiState) -> Option<session::PersistedPlaybackState> {
    if ui.playback_session.mode.is_radio() {
        return None;
    }

    let playback_index = ui.playback_session.queue_index?;
    let current_track = ui.playback_session.queue_tracks.get(playback_index)?;
    let current_item_id = current_track.item_id.clone()?;
    if current_item_id.is_empty() {
        return None;
    }

    let position_secs = ui
        .playback
        .as_ref()
        .and_then(PlaybackEngine::position)
        .map(|position| position.as_secs())
        .unwrap_or(0);
    let item_ids_by_index = ui
        .playback_session
        .queue_tracks
        .iter()
        .map(|track| track.item_id.clone())
        .collect::<Vec<_>>();

    session::playback_snapshot(
        current_item_id,
        &item_ids_by_index,
        &ui.playback_session.playback_order,
        position_secs,
        ui.playback_session.shuffle_enabled,
    )
}

pub(crate) fn save_playback_snapshot_now(ui: &mut UiState) {
    ui.last_playback_snapshot_at = Some(Instant::now());
    let Some(snapshot) = playback_snapshot(ui) else {
        return;
    };

    let result = serde_json::to_string(&snapshot)
        .map_err(crate::cache::CacheError::from)
        .and_then(|json| {
            CacheDatabase::open_default()
                .and_then(|cache| cache.set_setting(PLAYBACK_STATE_KEY, &json))
        });

    if let Err(error) = result {
        tracing::warn!(%error, "failed to save playback queue state");
    }
}

pub(crate) fn save_playback_snapshot_if_due(ui: &mut UiState) {
    if ui
        .last_playback_snapshot_at
        .is_some_and(|last_saved| last_saved.elapsed() < PLAYBACK_SNAPSHOT_INTERVAL)
    {
        return;
    }
    save_playback_snapshot_now(ui);
}

pub(crate) fn clear_playback_snapshot() {
    if let Err(error) = CacheDatabase::open_default().and_then(|cache| {
        cache
            .connection()
            .execute(
                "DELETE FROM app_settings WHERE key = ?1",
                [PLAYBACK_STATE_KEY],
            )
            .map(|_| ())
            .map_err(crate::cache::CacheError::from)
    }) {
        tracing::warn!(%error, "failed to clear playback queue state");
    }
}

pub(crate) fn load_playback_snapshot() -> Option<session::PersistedPlaybackState> {
    let result = CacheDatabase::open_default()
        .and_then(|cache| cache.get_setting(PLAYBACK_STATE_KEY))
        .and_then(|json| {
            json.map(|json| {
                serde_json::from_str::<session::PersistedPlaybackState>(&json)
                    .map_err(crate::cache::CacheError::from)
            })
            .transpose()
        });

    match result {
        Ok(Some(snapshot)) if snapshot.version == session::PLAYBACK_STATE_VERSION => Some(snapshot),
        Ok(Some(_)) | Ok(None) => None,
        Err(error) => {
            tracing::warn!(%error, "failed to load playback queue state");
            None
        }
    }
}

pub(crate) fn restore_playback_snapshot_tracks(
    library_tracks: &[UiTrack],
    snapshot: &session::PersistedPlaybackState,
) -> Option<(Vec<UiTrack>, usize, Vec<usize>)> {
    let tracks_by_id = library_tracks
        .iter()
        .filter_map(|track| track.item_id.as_deref().map(|item_id| (item_id, track)))
        .collect::<HashMap<_, _>>();
    let library_item_ids = library_tracks
        .iter()
        .filter_map(|track| track.item_id.clone())
        .collect::<Vec<_>>();
    let restored = session::restore_ordered_item_ids(
        &library_item_ids,
        &snapshot.ordered_item_ids,
        &snapshot.current_item_id,
    )?;
    let playback_tracks = restored
        .item_ids
        .iter()
        .filter_map(|item_id| {
            tracks_by_id
                .get(item_id.as_str())
                .map(|track| (*track).clone())
        })
        .collect::<Vec<_>>();

    Some((
        playback_tracks,
        restored.current_index,
        restored.playback_order,
    ))
}
