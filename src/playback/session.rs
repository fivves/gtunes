use std::time::{SystemTime, UNIX_EPOCH};

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
