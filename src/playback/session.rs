use std::collections::HashSet;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::PlaybackStreamKind;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RestoredPlaybackItems {
    pub item_ids: Vec<String>,
    pub current_index: usize,
    pub playback_order: Vec<usize>,
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
