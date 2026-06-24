use std::collections::HashSet;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::PlaybackStreamKind;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RestoredPlaybackItems {
    pub item_ids: Vec<String>,
    pub current_index: usize,
    pub playback_order: Vec<usize>,
}

#[derive(Clone, Debug)]
pub(crate) struct PlaybackSession<T> {
    pub mode: PlaybackMode,
    pub now_playing_key: Option<String>,
    pub queue_tracks: Vec<T>,
    pub queue_index: Option<usize>,
    pub playback_order: Vec<usize>,
    pub shuffle_enabled: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PlaybackSelection {
    pub selected_index: usize,
    pub visible_index: Option<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RestoredPlayback<T> {
    pub tracks: Vec<T>,
    pub current_index: usize,
    pub playback_order: Vec<usize>,
    pub shuffle_enabled: bool,
}

impl<T> Default for PlaybackSession<T> {
    fn default() -> Self {
        Self {
            mode: PlaybackMode::default(),
            now_playing_key: None,
            queue_tracks: Vec::new(),
            queue_index: None,
            playback_order: Vec::new(),
            shuffle_enabled: false,
        }
    }
}

impl<T> PlaybackSession<T> {
    pub(crate) fn clear_queue(&mut self) {
        self.queue_tracks.clear();
        self.queue_index = None;
        self.playback_order.clear();
    }

    pub(crate) fn reset_to_library(&mut self) {
        self.mode.set_library();
        self.now_playing_key = None;
        self.clear_queue();
    }

    pub(crate) fn activate_radio(&mut self, station_id: String) {
        self.mode.set_radio(station_id);
        self.now_playing_key = None;
        self.clear_queue();
    }

    pub(crate) fn start_library_playback(&mut self, now_playing_key: String) {
        self.now_playing_key = Some(now_playing_key);
        self.mode.set_library();
    }

    pub(crate) fn clear_now_playing(&mut self) {
        self.now_playing_key = None;
    }

    pub(crate) fn finish_playback(&mut self) -> bool {
        let was_radio = self.mode.is_radio();
        self.now_playing_key = None;
        self.clear_queue();
        was_radio
    }

    pub(crate) fn rebuild_order(&mut self, track_count: usize, start_index: usize) {
        self.playback_order = build_playback_order(track_count, start_index, self.shuffle_enabled);
    }

    pub(crate) fn current_index_or(&self, fallback_index: usize) -> usize {
        self.queue_index.unwrap_or(fallback_index)
    }

    pub(crate) fn next_index(&self, current_index: usize) -> Option<usize> {
        next_playback_index(&self.playback_order, current_index)
    }

    pub(crate) fn previous_index(&self, current_index: usize) -> Option<usize> {
        previous_playback_index(&self.playback_order, current_index)
    }

    pub(crate) fn queued_indices_with_limit(
        &self,
        current_index: usize,
        limit: usize,
    ) -> Vec<usize> {
        queued_indices_with_limit(&self.playback_order, current_index, limit)
    }

    pub(crate) fn upcoming_count(&self, current_index: usize) -> usize {
        upcoming_track_count(&self.playback_order, current_index)
    }

    pub(crate) fn move_upcoming_track(
        &mut self,
        current_index: usize,
        from: usize,
        to_slot: usize,
        visible_limit: usize,
    ) -> bool {
        move_upcoming_track_in_playback_order(
            &mut self.playback_order,
            current_index,
            from,
            to_slot,
            visible_limit,
        )
    }

    pub(crate) fn queue_track_next(&mut self, current_index: usize, target_index: usize) -> bool {
        queue_track_next_in_playback_order(&mut self.playback_order, current_index, target_index)
    }

    pub(crate) fn order_needs_rebuild_for(&self, current_index: usize) -> bool {
        self.playback_order.is_empty() || !self.playback_order.contains(&current_index)
    }

    pub(crate) fn toggle_shuffle(&mut self, library_track_count: usize, fallback_index: usize) {
        self.shuffle_enabled = !self.shuffle_enabled;
        let playback_index = self.current_index_or(fallback_index);
        let track_count = if self.queue_tracks.is_empty() {
            library_track_count
        } else {
            self.queue_tracks.len()
        };
        self.rebuild_order(track_count, playback_index);
    }

    pub(crate) fn apply_gapless_transition(
        &mut self,
        item_ids_by_index: &[Option<String>],
        fallback_index: usize,
        transition_item_id: &str,
    ) -> Option<usize> {
        let current_index = self.current_index_or(fallback_index);
        let transition_index = gapless_transition_index(
            item_ids_by_index,
            &self.playback_order,
            current_index,
            transition_item_id,
        )?;
        self.queue_index = Some(transition_index);
        Some(transition_index)
    }
}

impl<T: Clone> PlaybackSession<T> {
    pub(crate) fn select_library_track(
        &mut self,
        library_tracks: &[T],
        requested_index: usize,
        rebuild_order: bool,
        same_track: impl Fn(&T, &T) -> bool,
    ) -> PlaybackSelection {
        let selected_index = requested_index.min(library_tracks.len().saturating_sub(1));

        if rebuild_order || self.queue_tracks.is_empty() {
            self.queue_tracks = library_tracks.to_vec();
            self.queue_index = Some(selected_index);
            self.rebuild_order(self.queue_tracks.len(), selected_index);
        } else {
            self.queue_index = Some(requested_index.min(self.queue_tracks.len().saturating_sub(1)));
        }

        let visible_index = self
            .queue_index
            .and_then(|queue_index| self.queue_tracks.get(queue_index))
            .and_then(|queued_track| {
                library_tracks
                    .iter()
                    .position(|library_track| same_track(queued_track, library_track))
            });
        let selected_index = visible_index.unwrap_or(selected_index);

        PlaybackSelection {
            selected_index,
            visible_index,
        }
    }

    pub(crate) fn queue_library_track_next(
        &mut self,
        library_tracks: &[T],
        selected_index: usize,
        target_track: T,
        same_track: impl Fn(&T, &T) -> bool,
    ) -> bool {
        if self.mode.is_radio() {
            return false;
        }

        if self.queue_tracks.is_empty() {
            self.queue_tracks = library_tracks.to_vec();
        }
        if self.queue_tracks.is_empty() {
            return false;
        }

        let current_index = self
            .current_index_or(selected_index)
            .min(self.queue_tracks.len().saturating_sub(1));
        if self.order_needs_rebuild_for(current_index) {
            self.rebuild_order(self.queue_tracks.len(), current_index);
        }

        let target_index = if let Some(index) = self
            .queue_tracks
            .iter()
            .position(|queued_track| same_track(queued_track, &target_track))
        {
            index
        } else {
            self.queue_tracks.push(target_track);
            self.queue_tracks.len() - 1
        };

        self.queue_track_next(current_index, target_index)
    }

    pub(crate) fn restore_library_playback(
        &mut self,
        restored: RestoredPlayback<T>,
        visible_tracks: &[T],
        fallback_selected_index: usize,
        same_track: impl Fn(&T, &T) -> bool,
        track_key: impl Fn(&T) -> String,
    ) -> Option<PlaybackSelection> {
        let current_track = restored.tracks.get(restored.current_index)?;
        let now_playing_key = track_key(current_track);
        let visible_index = visible_tracks
            .iter()
            .position(|visible_track| same_track(current_track, visible_track));
        let selected_index = visible_index
            .unwrap_or(fallback_selected_index)
            .min(visible_tracks.len().saturating_sub(1));

        self.shuffle_enabled = restored.shuffle_enabled;
        self.queue_tracks = restored.tracks;
        self.queue_index = Some(restored.current_index);
        self.playback_order = restored.playback_order;
        self.now_playing_key = Some(now_playing_key);
        self.mode.set_library();

        Some(PlaybackSelection {
            selected_index,
            visible_index,
        })
    }

    pub(crate) fn reconcile_library_refresh(
        &mut self,
        library_tracks: &[T],
        track_key: impl Fn(&T) -> String,
    ) {
        let Some(now_playing_key) = self.now_playing_key.clone() else {
            self.clear_queue();
            return;
        };

        let still_available = library_tracks
            .iter()
            .any(|track| track_key(track) == now_playing_key);
        if !still_available {
            self.now_playing_key = None;
            self.clear_queue();
            return;
        }

        if !self.queue_tracks.is_empty() {
            self.queue_tracks = self
                .queue_tracks
                .iter()
                .map(|queued_track| {
                    let queued_key = track_key(queued_track);
                    library_tracks
                        .iter()
                        .find(|library_track| track_key(library_track) == queued_key)
                        .cloned()
                        .unwrap_or_else(|| queued_track.clone())
                })
                .collect();
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum FallbackSeekRestore {
    NotNeeded,
    Restored(Duration),
    Failed { position: Duration, error: String },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) enum PlaybackMode {
    #[default]
    Library,
    Radio {
        station_id: String,
    },
}

impl PlaybackMode {
    pub(crate) fn is_radio(&self) -> bool {
        matches!(self, Self::Radio { .. })
    }

    pub(crate) fn radio_station_id(&self) -> Option<&str> {
        match self {
            Self::Radio { station_id } => Some(station_id),
            Self::Library => None,
        }
    }

    pub(crate) fn set_library(&mut self) {
        *self = Self::Library;
    }

    pub(crate) fn set_radio(&mut self, station_id: String) {
        *self = Self::Radio { station_id };
    }
}

pub(crate) const PLAYBACK_STATE_VERSION: u8 = 1;

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub(crate) struct PersistedPlaybackState {
    pub version: u8,
    pub current_item_id: String,
    pub ordered_item_ids: Vec<String>,
    pub position_secs: u64,
    pub shuffle_enabled: bool,
}

pub(crate) fn build_playback_order(
    track_count: usize,
    start_index: usize,
    shuffle_enabled: bool,
) -> Vec<usize> {
    if track_count == 0 {
        return Vec::new();
    }

    let start_index = start_index.min(track_count.saturating_sub(1));
    if shuffle_enabled {
        let mut remaining = (0..track_count)
            .filter(|index| *index != start_index)
            .collect::<Vec<_>>();
        shuffle_indices(&mut remaining);
        std::iter::once(start_index).chain(remaining).collect()
    } else {
        (0..track_count).collect()
    }
}

pub(crate) fn next_playback_index(playback_order: &[usize], current_index: usize) -> Option<usize> {
    let order_position = playback_order
        .iter()
        .position(|index| *index == current_index)?;
    playback_order.get(order_position + 1).copied()
}

pub(crate) fn previous_playback_index(
    playback_order: &[usize],
    current_index: usize,
) -> Option<usize> {
    let order_position = playback_order
        .iter()
        .position(|index| *index == current_index)?;
    order_position
        .checked_sub(1)
        .and_then(|position| playback_order.get(position).copied())
}

pub(crate) fn queued_indices_with_limit(
    playback_order: &[usize],
    current_index: usize,
    limit: usize,
) -> Vec<usize> {
    let Some(order_position) = playback_order
        .iter()
        .position(|index| *index == current_index)
    else {
        return Vec::new();
    };

    playback_order
        .iter()
        .skip(order_position + 1)
        .take(limit)
        .copied()
        .collect()
}

pub(crate) fn upcoming_track_count(playback_order: &[usize], current_index: usize) -> usize {
    playback_order
        .iter()
        .position(|index| *index == current_index)
        .map(|position| playback_order.len().saturating_sub(position + 1))
        .unwrap_or(0)
}

pub(crate) fn move_upcoming_track_in_playback_order(
    playback_order: &mut Vec<usize>,
    current_index: usize,
    from: usize,
    to_slot: usize,
    visible_limit: usize,
) -> bool {
    let Some(order_position) = playback_order
        .iter()
        .position(|index| *index == current_index)
    else {
        return false;
    };

    let upcoming_start = order_position + 1;
    let visible_len = playback_order
        .len()
        .saturating_sub(upcoming_start)
        .min(visible_limit);
    if visible_len <= 1 || from >= visible_len {
        return false;
    }

    let to_slot = to_slot.min(visible_len);
    if from == to_slot || from + 1 == to_slot {
        return false;
    }

    let from_index = upcoming_start + from;
    let moved = playback_order.remove(from_index);
    let mut insert_index = upcoming_start + to_slot;
    if from_index < insert_index {
        insert_index -= 1;
    }
    playback_order.insert(insert_index, moved);
    true
}

pub(crate) fn queue_track_next_in_playback_order(
    playback_order: &mut Vec<usize>,
    current_index: usize,
    target_index: usize,
) -> bool {
    if current_index == target_index {
        return false;
    }

    let Some(current_position) = playback_order
        .iter()
        .position(|index| *index == current_index)
    else {
        return false;
    };

    if let Some(existing_position) = playback_order
        .iter()
        .position(|index| *index == target_index)
    {
        if existing_position == current_position + 1 {
            return false;
        }
        let moved = playback_order.remove(existing_position);
        let insert_position = if existing_position < current_position + 1 {
            current_position
        } else {
            current_position + 1
        };
        playback_order.insert(insert_position, moved);
    } else {
        playback_order.insert(current_position + 1, target_index);
    }

    true
}

pub(crate) fn can_retry_with_transcode(stream_kind: Option<PlaybackStreamKind>) -> bool {
    stream_kind == Some(PlaybackStreamKind::Direct)
}

pub(crate) fn fallback_playback_status(
    quality: &str,
    seek_restore: FallbackSeekRestore,
    format_position: impl Fn(Duration) -> String,
) -> String {
    match seek_restore {
        FallbackSeekRestore::Restored(position) => {
            format!(
                "Playing transcoded stream | {quality} | resumed at {}",
                format_position(position)
            )
        }
        FallbackSeekRestore::Failed { position, error } => {
            format!(
                "Playing transcoded stream | seek restore to {} failed: {error}",
                format_position(position)
            )
        }
        FallbackSeekRestore::NotNeeded => format!("Playing transcoded stream | {quality}"),
    }
}

pub(crate) fn gapless_transition_index(
    item_ids_by_index: &[Option<String>],
    playback_order: &[usize],
    current_index: usize,
    transition_item_id: &str,
) -> Option<usize> {
    item_ids_by_index
        .iter()
        .position(|item_id| item_id.as_deref() == Some(transition_item_id))
        .or_else(|| next_playback_index(playback_order, current_index))
}

pub(crate) fn playback_snapshot(
    current_item_id: String,
    item_ids_by_index: &[Option<String>],
    playback_order: &[usize],
    position_secs: u64,
    shuffle_enabled: bool,
) -> Option<PersistedPlaybackState> {
    if current_item_id.is_empty() {
        return None;
    }

    let ordered_item_ids = ordered_item_ids(item_ids_by_index, playback_order);
    if ordered_item_ids.is_empty() {
        return None;
    }

    Some(PersistedPlaybackState {
        version: PLAYBACK_STATE_VERSION,
        current_item_id,
        ordered_item_ids,
        position_secs,
        shuffle_enabled,
    })
}

fn ordered_item_ids(item_ids_by_index: &[Option<String>], playback_order: &[usize]) -> Vec<String> {
    let mut seen = HashSet::new();
    playback_order
        .iter()
        .filter_map(|index| item_ids_by_index.get(*index))
        .filter_map(Clone::clone)
        .filter(|item_id| seen.insert(item_id.clone()))
        .collect()
}

pub(crate) fn restore_ordered_item_ids(
    library_item_ids: &[String],
    ordered_item_ids: &[String],
    current_item_id: &str,
) -> Option<RestoredPlaybackItems> {
    if current_item_id.is_empty() || ordered_item_ids.is_empty() {
        return None;
    }

    let available_item_ids = library_item_ids
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let mut seen = HashSet::new();
    let item_ids = ordered_item_ids
        .iter()
        .filter(|item_id| available_item_ids.contains(item_id.as_str()))
        .filter(|item_id| seen.insert((*item_id).clone()))
        .cloned()
        .collect::<Vec<_>>();
    let current_index = item_ids
        .iter()
        .position(|item_id| item_id == current_item_id)?;
    let playback_order = (0..item_ids.len()).collect::<Vec<_>>();

    Some(RestoredPlaybackItems {
        item_ids,
        current_index,
        playback_order,
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playback_order_starts_with_current_track_when_shuffled() {
        let order = build_playback_order(5, 3, true);

        assert_eq!(order.first(), Some(&3));
        assert_eq!(order.len(), 5);
        for index in 0..5 {
            assert!(order.contains(&index));
        }
    }

    #[test]
    fn playback_mode_tracks_library_or_radio_state() {
        let mut mode = PlaybackMode::default();

        assert!(!mode.is_radio());
        assert_eq!(mode.radio_station_id(), None);

        mode.set_radio("station-1".to_string());
        assert!(mode.is_radio());
        assert_eq!(mode.radio_station_id(), Some("station-1"));

        mode.set_library();
        assert!(!mode.is_radio());
        assert_eq!(mode.radio_station_id(), None);
    }

    #[test]
    fn playback_session_defaults_to_empty_library_queue() {
        let session = PlaybackSession::<&str>::default();

        assert!(!session.mode.is_radio());
        assert_eq!(session.now_playing_key, None);
        assert!(session.queue_tracks.is_empty());
        assert_eq!(session.queue_index, None);
        assert!(session.playback_order.is_empty());
        assert!(!session.shuffle_enabled);
    }

    #[test]
    fn playback_session_clear_queue_keeps_mode_and_current_track() {
        let mut session = PlaybackSession {
            mode: PlaybackMode::Radio {
                station_id: "station-1".to_string(),
            },
            now_playing_key: Some("track-1".to_string()),
            queue_tracks: vec!["track-1", "track-2"],
            queue_index: Some(1),
            playback_order: vec![0, 1],
            shuffle_enabled: true,
        };

        session.clear_queue();

        assert_eq!(session.mode.radio_station_id(), Some("station-1"));
        assert_eq!(session.now_playing_key.as_deref(), Some("track-1"));
        assert!(session.queue_tracks.is_empty());
        assert_eq!(session.queue_index, None);
        assert!(session.playback_order.is_empty());
        assert!(session.shuffle_enabled);
    }

    #[test]
    fn playback_session_reset_to_library_clears_active_queue_without_touching_shuffle() {
        let mut session = PlaybackSession {
            mode: PlaybackMode::Radio {
                station_id: "station-1".to_string(),
            },
            now_playing_key: Some("track-1".to_string()),
            queue_tracks: vec!["track-1"],
            queue_index: Some(0),
            playback_order: vec![0],
            shuffle_enabled: true,
        };

        session.reset_to_library();

        assert!(!session.mode.is_radio());
        assert_eq!(session.now_playing_key, None);
        assert!(session.queue_tracks.is_empty());
        assert_eq!(session.queue_index, None);
        assert!(session.playback_order.is_empty());
        assert!(session.shuffle_enabled);
    }

    #[test]
    fn playback_session_activate_radio_clears_library_playback_without_touching_shuffle() {
        let mut session = PlaybackSession {
            mode: PlaybackMode::Library,
            now_playing_key: Some("track-1".to_string()),
            queue_tracks: vec!["track-1", "track-2"],
            queue_index: Some(1),
            playback_order: vec![0, 1],
            shuffle_enabled: true,
        };

        session.activate_radio("station-1".to_string());

        assert_eq!(session.mode.radio_station_id(), Some("station-1"));
        assert_eq!(session.now_playing_key, None);
        assert!(session.queue_tracks.is_empty());
        assert_eq!(session.queue_index, None);
        assert!(session.playback_order.is_empty());
        assert!(session.shuffle_enabled);
    }

    #[test]
    fn playback_session_start_library_playback_sets_key_and_library_mode() {
        let mut session = PlaybackSession {
            mode: PlaybackMode::Radio {
                station_id: "station-1".to_string(),
            },
            now_playing_key: None,
            queue_tracks: vec!["track-1"],
            queue_index: Some(0),
            playback_order: vec![0],
            shuffle_enabled: true,
        };

        session.start_library_playback("track-1".to_string());

        assert!(!session.mode.is_radio());
        assert_eq!(session.now_playing_key.as_deref(), Some("track-1"));
        assert_eq!(session.queue_tracks, vec!["track-1"]);
        assert_eq!(session.queue_index, Some(0));
        assert_eq!(session.playback_order, vec![0]);
        assert!(session.shuffle_enabled);
    }

    #[test]
    fn playback_session_clear_now_playing_keeps_queue_and_mode() {
        let mut session = PlaybackSession {
            mode: PlaybackMode::Radio {
                station_id: "station-1".to_string(),
            },
            now_playing_key: Some("track-1".to_string()),
            queue_tracks: vec!["track-1"],
            queue_index: Some(0),
            playback_order: vec![0],
            shuffle_enabled: true,
        };

        session.clear_now_playing();

        assert_eq!(session.mode.radio_station_id(), Some("station-1"));
        assert_eq!(session.now_playing_key, None);
        assert_eq!(session.queue_tracks, vec!["track-1"]);
        assert_eq!(session.queue_index, Some(0));
        assert_eq!(session.playback_order, vec![0]);
        assert!(session.shuffle_enabled);
    }

    #[test]
    fn playback_session_finish_playback_clears_library_queue_and_reports_mode() {
        let mut session = PlaybackSession {
            mode: PlaybackMode::Library,
            now_playing_key: Some("track-1".to_string()),
            queue_tracks: vec!["track-1"],
            queue_index: Some(0),
            playback_order: vec![0],
            shuffle_enabled: true,
        };

        let was_radio = session.finish_playback();

        assert!(!was_radio);
        assert!(!session.mode.is_radio());
        assert_eq!(session.now_playing_key, None);
        assert!(session.queue_tracks.is_empty());
        assert_eq!(session.queue_index, None);
        assert!(session.playback_order.is_empty());
        assert!(session.shuffle_enabled);
    }

    #[test]
    fn playback_session_finish_playback_preserves_radio_mode() {
        let mut session = PlaybackSession {
            mode: PlaybackMode::Radio {
                station_id: "station-1".to_string(),
            },
            now_playing_key: Some("track-1".to_string()),
            queue_tracks: vec!["track-1"],
            queue_index: Some(0),
            playback_order: vec![0],
            shuffle_enabled: true,
        };

        let was_radio = session.finish_playback();

        assert!(was_radio);
        assert_eq!(session.mode.radio_station_id(), Some("station-1"));
        assert_eq!(session.now_playing_key, None);
        assert!(session.queue_tracks.is_empty());
        assert_eq!(session.queue_index, None);
        assert!(session.playback_order.is_empty());
        assert!(session.shuffle_enabled);
    }

    #[test]
    fn playback_session_rebuilds_and_navigates_order() {
        let mut session = PlaybackSession::<&str>::default();

        session.rebuild_order(4, 2);

        assert_eq!(session.playback_order, vec![0, 1, 2, 3]);
        assert_eq!(session.current_index_or(2), 2);
        assert_eq!(session.next_index(2), Some(3));
        assert_eq!(session.previous_index(2), Some(1));
        assert_eq!(session.queued_indices_with_limit(1, 2), vec![2, 3]);
        assert_eq!(session.upcoming_count(1), 2);
        assert!(!session.order_needs_rebuild_for(2));
        assert!(session.order_needs_rebuild_for(99));
    }

    #[test]
    fn playback_session_mutates_upcoming_order() {
        let mut session = PlaybackSession {
            playback_order: vec![0, 1, 2, 3],
            ..PlaybackSession::<&str>::default()
        };

        assert!(session.queue_track_next(1, 3));
        assert_eq!(session.playback_order, vec![0, 1, 3, 2]);

        assert!(session.move_upcoming_track(1, 1, 0, 50));
        assert_eq!(session.playback_order, vec![0, 1, 2, 3]);
    }

    #[test]
    fn playback_session_toggle_shuffle_rebuilds_order_from_queue() {
        let mut session = PlaybackSession {
            queue_tracks: vec!["first", "second", "third"],
            queue_index: Some(2),
            playback_order: vec![0, 1, 2],
            ..PlaybackSession::<&str>::default()
        };

        session.toggle_shuffle(99, 0);

        assert!(session.shuffle_enabled);
        assert_eq!(session.playback_order.first(), Some(&2));
        assert_eq!(session.playback_order.len(), 3);
        for index in 0..3 {
            assert!(session.playback_order.contains(&index));
        }
    }

    #[test]
    fn playback_session_toggle_shuffle_uses_library_count_without_queue() {
        let mut session = PlaybackSession {
            shuffle_enabled: true,
            playback_order: vec![2, 0, 1],
            ..PlaybackSession::<&str>::default()
        };

        session.toggle_shuffle(3, 1);

        assert!(!session.shuffle_enabled);
        assert_eq!(session.playback_order, vec![0, 1, 2]);
    }

    #[test]
    fn playback_session_gapless_transition_sets_matching_queue_index() {
        let mut session = PlaybackSession {
            queue_index: Some(0),
            playback_order: vec![0, 1, 2],
            ..PlaybackSession::<&str>::default()
        };
        let item_ids = vec![
            Some("first".to_string()),
            Some("second".to_string()),
            Some("third".to_string()),
        ];

        let index = session.apply_gapless_transition(&item_ids, 0, "third");

        assert_eq!(index, Some(2));
        assert_eq!(session.queue_index, Some(2));
    }

    #[test]
    fn playback_session_gapless_transition_falls_back_to_next_ordered_index() {
        let mut session = PlaybackSession {
            queue_index: Some(0),
            playback_order: vec![0, 2, 1],
            ..PlaybackSession::<&str>::default()
        };
        let item_ids = vec![Some("first".to_string()), Some("second".to_string())];

        let index = session.apply_gapless_transition(&item_ids, 0, "missing");

        assert_eq!(index, Some(2));
        assert_eq!(session.queue_index, Some(2));
    }

    #[test]
    fn playback_session_gapless_transition_keeps_index_when_target_missing() {
        let mut session = PlaybackSession {
            queue_index: Some(0),
            playback_order: vec![0],
            ..PlaybackSession::<&str>::default()
        };
        let item_ids = vec![Some("first".to_string())];

        let index = session.apply_gapless_transition(&item_ids, 0, "missing");

        assert_eq!(index, None);
        assert_eq!(session.queue_index, Some(0));
    }

    #[test]
    fn playback_session_selects_library_track_and_rebuilds_queue() {
        let mut session = PlaybackSession::<&str>::default();

        let selection =
            session.select_library_track(&["first", "second", "third"], 1, true, |a, b| a == b);

        assert_eq!(
            selection,
            PlaybackSelection {
                selected_index: 1,
                visible_index: Some(1),
            }
        );
        assert_eq!(session.queue_tracks, vec!["first", "second", "third"]);
        assert_eq!(session.queue_index, Some(1));
        assert_eq!(session.playback_order, vec![0, 1, 2]);
    }

    #[test]
    fn playback_session_selects_existing_queue_track_visible_in_library() {
        let mut session = PlaybackSession {
            queue_tracks: vec!["queued-first", "library-second"],
            queue_index: Some(0),
            playback_order: vec![0, 1],
            ..PlaybackSession::<&str>::default()
        };

        let selection =
            session.select_library_track(&["library-first", "library-second"], 1, false, |a, b| {
                a == b
            });

        assert_eq!(
            selection,
            PlaybackSelection {
                selected_index: 1,
                visible_index: Some(1),
            }
        );
        assert_eq!(session.queue_tracks, vec!["queued-first", "library-second"]);
        assert_eq!(session.queue_index, Some(1));
        assert_eq!(session.playback_order, vec![0, 1]);
    }

    #[test]
    fn playback_session_clamps_empty_library_selection() {
        let mut session = PlaybackSession::<&str>::default();

        let selection = session.select_library_track(&[], 10, true, |a, b| a == b);

        assert_eq!(
            selection,
            PlaybackSelection {
                selected_index: 0,
                visible_index: None,
            }
        );
        assert!(session.queue_tracks.is_empty());
        assert_eq!(session.queue_index, Some(0));
        assert!(session.playback_order.is_empty());
    }

    #[test]
    fn playback_session_queues_library_track_next_from_empty_queue() {
        let mut session = PlaybackSession {
            queue_index: Some(0),
            ..PlaybackSession::<&str>::default()
        };

        assert!(session.queue_library_track_next(
            &["first", "second", "third"],
            0,
            "third",
            |a, b| a == b,
        ));

        assert_eq!(session.queue_tracks, vec!["first", "second", "third"]);
        assert_eq!(session.playback_order, vec![0, 2, 1]);
    }

    #[test]
    fn playback_session_queues_missing_track_next_by_appending_it() {
        let mut session = PlaybackSession {
            queue_tracks: vec!["current", "other"],
            queue_index: Some(0),
            playback_order: vec![0, 1],
            ..PlaybackSession::<&str>::default()
        };

        assert!(session.queue_library_track_next(&[], 0, "missing", |a, b| a == b));

        assert_eq!(session.queue_tracks, vec!["current", "other", "missing"]);
        assert_eq!(session.playback_order, vec![0, 2, 1]);
    }

    #[test]
    fn playback_session_does_not_queue_next_in_radio_mode() {
        let mut session = PlaybackSession {
            mode: PlaybackMode::Radio {
                station_id: "station-1".to_string(),
            },
            queue_tracks: vec!["current", "target"],
            queue_index: Some(0),
            playback_order: vec![0, 1],
            ..PlaybackSession::<&str>::default()
        };

        assert!(
            !session
                .queue_library_track_next(&["current", "target"], 0, "target", |a, b| { a == b })
        );

        assert_eq!(session.playback_order, vec![0, 1]);
    }

    #[test]
    fn playback_session_rebuilds_stale_order_before_queueing_next() {
        let mut session = PlaybackSession {
            queue_tracks: vec!["current", "other", "target"],
            queue_index: Some(1),
            playback_order: vec![0],
            ..PlaybackSession::<&str>::default()
        };

        assert!(session.queue_library_track_next(&[], 1, "current", |a, b| a == b));

        assert_eq!(session.playback_order, vec![1, 0, 2]);
    }

    #[test]
    fn playback_session_restores_library_playback_state() {
        let mut session = PlaybackSession {
            mode: PlaybackMode::Radio {
                station_id: "station-1".to_string(),
            },
            now_playing_key: Some("old".to_string()),
            queue_tracks: vec!["old"],
            queue_index: Some(0),
            playback_order: vec![0],
            shuffle_enabled: false,
        };

        let selection = session
            .restore_library_playback(
                RestoredPlayback {
                    tracks: vec!["first", "second", "third"],
                    current_index: 1,
                    playback_order: vec![1, 2, 0],
                    shuffle_enabled: true,
                },
                &["visible-first", "second"],
                0,
                |a, b| a == b,
                |track| (*track).to_string(),
            )
            .expect("restore applies");

        assert_eq!(
            selection,
            PlaybackSelection {
                selected_index: 1,
                visible_index: Some(1),
            }
        );
        assert!(!session.mode.is_radio());
        assert_eq!(session.now_playing_key.as_deref(), Some("second"));
        assert_eq!(session.queue_tracks, vec!["first", "second", "third"]);
        assert_eq!(session.queue_index, Some(1));
        assert_eq!(session.playback_order, vec![1, 2, 0]);
        assert!(session.shuffle_enabled);
    }

    #[test]
    fn playback_session_restore_uses_fallback_selection_when_current_track_is_hidden() {
        let mut session = PlaybackSession::<&str>::default();

        let selection = session
            .restore_library_playback(
                RestoredPlayback {
                    tracks: vec!["first", "second"],
                    current_index: 1,
                    playback_order: vec![0, 1],
                    shuffle_enabled: false,
                },
                &["visible-first"],
                5,
                |a, b| a == b,
                |track| (*track).to_string(),
            )
            .expect("restore applies");

        assert_eq!(
            selection,
            PlaybackSelection {
                selected_index: 0,
                visible_index: None,
            }
        );
        assert_eq!(session.now_playing_key.as_deref(), Some("second"));
    }

    #[test]
    fn playback_session_restore_rejects_missing_current_index() {
        let mut session = PlaybackSession {
            now_playing_key: Some("old".to_string()),
            queue_tracks: vec!["old"],
            queue_index: Some(0),
            playback_order: vec![0],
            ..PlaybackSession::<&str>::default()
        };

        let selection = session.restore_library_playback(
            RestoredPlayback {
                tracks: vec!["first"],
                current_index: 99,
                playback_order: vec![0],
                shuffle_enabled: true,
            },
            &["first"],
            0,
            |a, b| a == b,
            |track| (*track).to_string(),
        );

        assert_eq!(selection, None);
        assert_eq!(session.now_playing_key.as_deref(), Some("old"));
        assert_eq!(session.queue_tracks, vec!["old"]);
        assert_eq!(session.queue_index, Some(0));
        assert_eq!(session.playback_order, vec![0]);
        assert!(!session.shuffle_enabled);
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct TestTrack {
        key: &'static str,
        title: &'static str,
    }

    fn test_track_key(track: &TestTrack) -> String {
        track.key.to_string()
    }

    #[test]
    fn playback_session_refresh_replaces_queued_tracks_from_library() {
        let mut session = PlaybackSession {
            now_playing_key: Some("current".to_string()),
            queue_tracks: vec![
                TestTrack {
                    key: "current",
                    title: "Old current",
                },
                TestTrack {
                    key: "missing",
                    title: "Keep queued copy",
                },
            ],
            queue_index: Some(0),
            playback_order: vec![0, 1],
            ..PlaybackSession::default()
        };

        session.reconcile_library_refresh(
            &[
                TestTrack {
                    key: "current",
                    title: "Fresh current",
                },
                TestTrack {
                    key: "other",
                    title: "Other",
                },
            ],
            test_track_key,
        );

        assert_eq!(session.now_playing_key.as_deref(), Some("current"));
        assert_eq!(session.queue_tracks[0].title, "Fresh current");
        assert_eq!(session.queue_tracks[1].title, "Keep queued copy");
        assert_eq!(session.queue_index, Some(0));
        assert_eq!(session.playback_order, vec![0, 1]);
    }

    #[test]
    fn playback_session_refresh_clears_queue_when_now_playing_disappears() {
        let mut session = PlaybackSession {
            now_playing_key: Some("missing".to_string()),
            queue_tracks: vec![TestTrack {
                key: "missing",
                title: "Missing",
            }],
            queue_index: Some(0),
            playback_order: vec![0],
            ..PlaybackSession::default()
        };

        session.reconcile_library_refresh(
            &[TestTrack {
                key: "other",
                title: "Other",
            }],
            test_track_key,
        );

        assert_eq!(session.now_playing_key, None);
        assert!(session.queue_tracks.is_empty());
        assert_eq!(session.queue_index, None);
        assert!(session.playback_order.is_empty());
    }

    #[test]
    fn playback_session_refresh_clears_queue_without_active_track() {
        let mut session = PlaybackSession {
            now_playing_key: None,
            queue_tracks: vec![TestTrack {
                key: "queued",
                title: "Queued",
            }],
            queue_index: Some(0),
            playback_order: vec![0],
            ..PlaybackSession::default()
        };

        session.reconcile_library_refresh(&[], test_track_key);

        assert_eq!(session.now_playing_key, None);
        assert!(session.queue_tracks.is_empty());
        assert_eq!(session.queue_index, None);
        assert!(session.playback_order.is_empty());
    }

    #[test]
    fn playback_order_is_sequential_when_shuffle_is_disabled() {
        let order = build_playback_order(4, 2, false);

        assert_eq!(order, vec![0, 1, 2, 3]);
    }

    #[test]
    fn playback_order_clamps_start_index_for_shuffle() {
        let order = build_playback_order(3, 99, true);

        assert_eq!(order.first(), Some(&2));
        assert_eq!(order.len(), 3);
        for index in 0..3 {
            assert!(order.contains(&index));
        }
    }

    #[test]
    fn next_and_previous_indices_follow_playback_order() {
        let order = vec![2, 0, 3, 1];

        assert_eq!(next_playback_index(&order, 0), Some(3));
        assert_eq!(previous_playback_index(&order, 0), Some(2));
        assert_eq!(next_playback_index(&order, 1), None);
        assert_eq!(previous_playback_index(&order, 2), None);
    }

    #[test]
    fn queued_indices_follow_playback_order_after_current_track() {
        let queued = queued_indices_with_limit(&[0, 1, 2], 1, 50);

        assert_eq!(queued, vec![2]);
    }

    #[test]
    fn queued_indices_respect_limit() {
        let queued = queued_indices_with_limit(&[0, 1, 2, 3], 0, 2);

        assert_eq!(queued, vec![1, 2]);
    }

    #[test]
    fn upcoming_track_count_counts_after_current_index() {
        assert_eq!(upcoming_track_count(&[0, 1, 2, 3], 1), 2);
        assert_eq!(upcoming_track_count(&[0, 1, 2, 3], 3), 0);
        assert_eq!(upcoming_track_count(&[0, 1, 2, 3], 99), 0);
    }

    #[test]
    fn fallback_retry_only_applies_to_direct_streams() {
        assert!(can_retry_with_transcode(Some(PlaybackStreamKind::Direct)));
        assert!(!can_retry_with_transcode(Some(
            PlaybackStreamKind::Transcode
        )));
        assert!(!can_retry_with_transcode(None));
    }

    #[test]
    fn fallback_status_reports_restored_position() {
        let status = fallback_playback_status(
            "MP3 320 kbps",
            FallbackSeekRestore::Restored(Duration::from_secs(83)),
            test_format_duration,
        );

        assert_eq!(
            status,
            "Playing transcoded stream | MP3 320 kbps | resumed at 1:23"
        );
    }

    #[test]
    fn fallback_status_reports_seek_restore_failure() {
        let status = fallback_playback_status(
            "MP3 320 kbps",
            FallbackSeekRestore::Failed {
                position: Duration::from_secs(83),
                error: "seek failed".to_string(),
            },
            test_format_duration,
        );

        assert_eq!(
            status,
            "Playing transcoded stream | seek restore to 1:23 failed: seek failed"
        );
    }

    #[test]
    fn fallback_status_handles_missing_position() {
        let status = fallback_playback_status(
            "MP3 320 kbps",
            FallbackSeekRestore::NotNeeded,
            test_format_duration,
        );

        assert_eq!(status, "Playing transcoded stream | MP3 320 kbps");
    }

    fn test_format_duration(duration: Duration) -> String {
        let seconds = duration.as_secs();
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }

    #[test]
    fn gapless_transition_prefers_matching_item_id() {
        let item_ids = vec![
            Some("first".to_string()),
            Some("second".to_string()),
            Some("third".to_string()),
        ];

        let index = gapless_transition_index(&item_ids, &[0, 1, 2], 0, "third");

        assert_eq!(index, Some(2));
    }

    #[test]
    fn gapless_transition_falls_back_to_next_ordered_index() {
        let item_ids = vec![Some("first".to_string()), Some("second".to_string())];

        let index = gapless_transition_index(&item_ids, &[0, 1], 0, "missing");

        assert_eq!(index, Some(1));
    }

    #[test]
    fn gapless_transition_returns_none_when_match_and_next_are_missing() {
        let item_ids = vec![Some("first".to_string())];

        let index = gapless_transition_index(&item_ids, &[0], 0, "missing");

        assert_eq!(index, None);
    }

    #[test]
    fn playback_snapshot_dedupes_ordered_item_ids() {
        let item_ids = vec![
            Some("first".to_string()),
            Some("second".to_string()),
            Some("first".to_string()),
        ];

        let snapshot = playback_snapshot("second".to_string(), &item_ids, &[0, 2, 1], 42, true)
            .expect("snapshot builds");

        assert_eq!(snapshot.version, PLAYBACK_STATE_VERSION);
        assert_eq!(snapshot.current_item_id, "second");
        assert_eq!(snapshot.ordered_item_ids, vec!["first", "second"]);
        assert_eq!(snapshot.position_secs, 42);
        assert!(snapshot.shuffle_enabled);
    }

    #[test]
    fn playback_snapshot_requires_current_item_and_ordered_items() {
        let item_ids = vec![Some("first".to_string())];

        assert!(playback_snapshot(String::new(), &item_ids, &[0], 0, false).is_none());
        assert!(playback_snapshot("first".to_string(), &item_ids, &[99], 0, false).is_none());
    }

    #[test]
    fn restore_ordered_item_ids_uses_available_library_items() {
        let library_item_ids = vec![
            "first".to_string(),
            "second".to_string(),
            "third".to_string(),
        ];
        let ordered_item_ids = vec![
            "first".to_string(),
            "missing".to_string(),
            "second".to_string(),
            "first".to_string(),
            "third".to_string(),
        ];

        let restored = restore_ordered_item_ids(&library_item_ids, &ordered_item_ids, "second")
            .expect("queue restores");

        assert_eq!(restored.item_ids, vec!["first", "second", "third"]);
        assert_eq!(restored.current_index, 1);
        assert_eq!(restored.playback_order, vec![0, 1, 2]);
    }

    #[test]
    fn restore_ordered_item_ids_requires_current_item() {
        let library_item_ids = vec!["first".to_string()];
        let ordered_item_ids = vec!["first".to_string()];

        assert!(
            restore_ordered_item_ids(&library_item_ids, &ordered_item_ids, "missing").is_none()
        );
    }

    #[test]
    fn move_upcoming_track_reorders_visible_queue_without_touching_current_track() {
        let mut playback_order = vec![4, 0, 1, 2, 3];

        let changed = move_upcoming_track_in_playback_order(&mut playback_order, 4, 0, 3, 50);

        assert!(changed);
        assert_eq!(playback_order, vec![4, 1, 2, 0, 3]);
    }

    #[test]
    fn move_upcoming_track_ignores_no_op_moves() {
        let mut playback_order = vec![0, 1, 2, 3];

        let changed = move_upcoming_track_in_playback_order(&mut playback_order, 0, 1, 2, 50);

        assert!(!changed);
        assert_eq!(playback_order, vec![0, 1, 2, 3]);
    }

    #[test]
    fn queue_track_next_moves_existing_track_directly_after_current() {
        let mut playback_order = vec![0, 1, 2, 3];

        let changed = queue_track_next_in_playback_order(&mut playback_order, 1, 3);

        assert!(changed);
        assert_eq!(playback_order, vec![0, 1, 3, 2]);
    }

    #[test]
    fn queue_track_next_inserts_missing_track_directly_after_current() {
        let mut playback_order = vec![0, 1, 2];

        let changed = queue_track_next_in_playback_order(&mut playback_order, 0, 4);

        assert!(changed);
        assert_eq!(playback_order, vec![0, 4, 1, 2]);
    }

    #[test]
    fn queue_track_next_ignores_current_or_already_next_track() {
        let mut current_track_order = vec![0, 1, 2];
        let current_changed = queue_track_next_in_playback_order(&mut current_track_order, 1, 1);
        assert!(!current_changed);
        assert_eq!(current_track_order, vec![0, 1, 2]);

        let mut already_next_order = vec![0, 1, 2];
        let next_changed = queue_track_next_in_playback_order(&mut already_next_order, 0, 1);
        assert!(!next_changed);
        assert_eq!(already_next_order, vec![0, 1, 2]);
    }
}
