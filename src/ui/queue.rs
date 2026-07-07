use super::prelude::*;

pub(crate) const NEXT_UP_PAGE_LIMIT: usize = 50;

pub(crate) struct QueueView {
    pub(crate) empty: gtk::Label,
    pub(crate) rows: Vec<QueueRow>,
}

pub(crate) struct QueueRow {
    pub(crate) button: gtk::Button,
    pub(crate) art: gtk::Image,
    pub(crate) title: gtk::Label,
    pub(crate) artist: gtk::Label,
    pub(crate) track_index: Rc<RefCell<Option<usize>>>,
    pub(crate) artwork_url: Rc<RefCell<Option<String>>>,
}

pub(crate) struct NextUpPageView {
    pub(crate) empty: gtk::Box,
    pub(crate) list: gtk::Box,
    pub(crate) rows: Rc<RefCell<Vec<gtk::Button>>>,
}

pub(crate) fn next_up_page(state: Rc<RefCell<UiState>>) -> gtk::ScrolledWindow {
    let scroll = gtk::ScrolledWindow::new();
    scroll.add_css_class("collection-scroll");
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_overlay_scrolling(true);
    scroll.set_hexpand(true);
    scroll.set_vexpand(true);

    let page = gtk::Box::new(Orientation::Vertical, 0);
    page.add_css_class("next-up-page");
    page.set_margin_top(18);
    page.set_margin_bottom(18);
    page.set_margin_start(18);
    page.set_margin_end(18);
    scroll.set_child(Some(&page));

    let empty = collection_empty_state("Nothing queued yet");
    empty.add_css_class("next-up-empty");
    page.append(&empty);

    let list = gtk::Box::new(Orientation::Vertical, 8);
    list.add_css_class("next-up-list");
    list.set_hexpand(true);

    let list_drop = gtk::DropTarget::new(gtk::glib::Type::U32, gtk::gdk::DragAction::MOVE);
    {
        let state = state.clone();
        list_drop.connect_drop(move |_, value, _, _| {
            let Ok(from) = value.get::<u32>() else {
                return false;
            };
            move_next_up_track(&state, from as usize, NEXT_UP_PAGE_LIMIT)
        });
    }
    list.add_controller(list_drop);

    page.append(&list);

    state.borrow_mut().next_up_view = Some(Rc::new(NextUpPageView {
        empty,
        list,
        rows: Rc::new(RefCell::new(Vec::new())),
    }));
    rebuild_queue_list(&state);
    scroll
}

pub(crate) fn queued_tracks(ui: &UiState) -> Vec<(usize, UiTrack)> {
    queued_tracks_with_limit(ui, QUEUE_PREVIEW_LIMIT)
}

pub(crate) fn next_up_tracks(ui: &UiState) -> Vec<(usize, UiTrack)> {
    queued_tracks_with_limit(ui, NEXT_UP_PAGE_LIMIT)
}

pub(crate) fn queued_tracks_with_limit(ui: &UiState, limit: usize) -> Vec<(usize, UiTrack)> {
    let tracks = if ui.playback_session.queue_tracks.is_empty() {
        ui.tracks.as_slice()
    } else {
        ui.playback_session.queue_tracks.as_slice()
    };
    let current_index = ui.playback_session.current_index_or(ui.selected_index);
    queued_tracks_from_order_with_limit(
        tracks,
        &ui.playback_session.playback_order,
        current_index,
        limit,
    )
}

#[cfg(test)]
pub(crate) fn queued_tracks_from_order(
    tracks: &[UiTrack],
    playback_order: &[usize],
    current_index: usize,
) -> Vec<(usize, UiTrack)> {
    queued_tracks_from_order_with_limit(tracks, playback_order, current_index, QUEUE_PREVIEW_LIMIT)
}

pub(crate) fn queued_tracks_from_order_with_limit(
    tracks: &[UiTrack],
    playback_order: &[usize],
    current_index: usize,
    limit: usize,
) -> Vec<(usize, UiTrack)> {
    session::queued_indices_with_limit(playback_order, current_index, limit)
        .into_iter()
        .filter_map(|index| tracks.get(index).cloned().map(|track| (index, track)))
        .collect()
}

pub(crate) fn upcoming_track_count(ui: &UiState) -> usize {
    let current_index = ui.playback_session.current_index_or(ui.selected_index);
    ui.playback_session.upcoming_count(current_index)
}

pub(crate) fn move_next_up_track(
    state: &Rc<RefCell<UiState>>,
    from: usize,
    to_slot: usize,
) -> bool {
    let changed = {
        let mut ui = state.borrow_mut();
        let current_index = ui.playback_session.current_index_or(ui.selected_index);
        let changed = ui.playback_session.move_upcoming_track(
            current_index,
            from,
            to_slot,
            NEXT_UP_PAGE_LIMIT,
        );
        if changed {
            arm_gapless_next(&mut ui);
            save_playback_snapshot_now(&mut ui);
        }
        changed
    };
    if changed {
        rebuild_queue_list(state);
    }
    changed
}

pub(crate) fn queue_track_next(ui: &mut UiState, target_track: UiTrack) -> bool {
    ui.playback_session.queue_library_track_next(
        &ui.tracks,
        ui.selected_index,
        target_track,
        same_track,
    )
}

pub(crate) fn finalize_queue_change(state: &Rc<RefCell<UiState>>) {
    {
        let mut ui = state.borrow_mut();
        arm_gapless_next(&mut ui);
        save_playback_snapshot_now(&mut ui);
        update_page_summary(&ui);
    }
    rebuild_queue_list(state);
}

pub(crate) fn queue_visible_track_next(state: &Rc<RefCell<UiState>>, visible_index: usize) -> bool {
    let changed = {
        let mut ui = state.borrow_mut();
        let Some(track) = ui.tracks.get(visible_index).cloned() else {
            return false;
        };
        queue_track_next(&mut ui, track)
    };
    if changed {
        finalize_queue_change(state);
    }
    changed
}

pub(crate) fn queue_existing_track_next(
    state: &Rc<RefCell<UiState>>,
    playback_index: usize,
) -> bool {
    let changed = {
        let mut ui = state.borrow_mut();
        let track = if ui.playback_session.queue_tracks.is_empty() {
            ui.tracks.get(playback_index).cloned()
        } else {
            ui.playback_session
                .queue_tracks
                .get(playback_index)
                .cloned()
        };
        let Some(track) = track else {
            return false;
        };
        queue_track_next(&mut ui, track)
    };
    if changed {
        finalize_queue_change(state);
    }
    changed
}

pub(crate) fn connect_play_next_gesture<F>(widget: &impl IsA<gtk::Widget>, handler: F)
where
    F: Fn() -> bool + 'static,
{
    let gesture = gtk::GestureClick::new();
    gesture.set_button(0);
    gesture.connect_pressed(move |gesture, _, _, _| {
        let button = gesture.current_button();
        let modifiers = gesture.current_event_state();
        let should_queue_next = button == 3
            || (button == 1 && modifiers.contains(gtk::gdk::ModifierType::CONTROL_MASK));
        if should_queue_next && handler() {
            gesture.set_state(gtk::EventSequenceState::Claimed);
        }
    });
    widget.add_controller(gesture);
}

pub(crate) const QUEUE_PREVIEW_LIMIT: usize = 15;

pub(crate) fn queue_card(state: Rc<RefCell<UiState>>) -> gtk::Box {
    let card = gtk::Box::new(Orientation::Vertical, 4);
    card.add_css_class("queue-card");
    card.set_halign(Align::Fill);
    card.set_hexpand(true);
    card.append(&next_up_link_button(state.clone()));

    let empty = label("Nothing up next", "meta");
    card.append(&empty);

    let scroll = gtk::ScrolledWindow::new();
    scroll.add_css_class("queue-scroll");
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_overlay_scrolling(true);
    scroll.set_propagate_natural_height(true);
    scroll.set_max_content_height(128);
    scroll.set_hexpand(true);

    let list = gtk::Box::new(Orientation::Vertical, 4);
    list.set_hexpand(true);
    scroll.set_child(Some(&list));

    let mut rows = Vec::with_capacity(QUEUE_PREVIEW_LIMIT);
    for _ in 0..QUEUE_PREVIEW_LIMIT {
        let button = gtk::Button::new();
        button.add_css_class("queue-row");
        button.add_css_class("flat");
        button.set_visible(false);
        button.set_halign(Align::Fill);
        button.set_hexpand(true);
        button.set_cursor_from_name(Some("pointer"));

        let row = gtk::Box::new(Orientation::Horizontal, 7);
        row.set_halign(Align::Fill);
        row.set_valign(Align::Center);
        row.set_hexpand(true);
        row.set_size_request(0, -1);

        let art = cover_art(28);
        art.set_icon_name(Some("audio-x-generic-symbolic"));
        art.set_valign(Align::Center);
        row.append(&art);

        let title = label("", "queue-title");
        title.set_single_line_mode(true);
        title.set_lines(1);
        title.set_width_chars(1);
        title.set_max_width_chars(22);
        title.set_halign(Align::Fill);
        title.set_hexpand(true);
        let artist = label("", "queue-artist");
        artist.set_single_line_mode(true);
        artist.set_lines(1);
        artist.set_width_chars(1);
        artist.set_max_width_chars(22);
        artist.set_halign(Align::Fill);
        artist.set_hexpand(true);
        let text = gtk::Box::new(Orientation::Vertical, 1);
        text.set_halign(Align::Fill);
        text.set_valign(Align::Center);
        text.set_hexpand(true);
        text.set_size_request(0, -1);
        text.append(&title);
        text.append(&artist);
        row.append(&text);
        button.set_child(Some(&row));

        let track_index = Rc::new(RefCell::new(None));
        let click_track_index = track_index.clone();
        let click_state = state.clone();
        button.connect_clicked(move |_| {
            let index = *click_track_index.borrow();
            if let Some(index) = index {
                play_track_at_existing_order(&click_state, index);
            }
        });
        {
            let gesture_track_index = track_index.clone();
            let gesture_state = state.clone();
            connect_play_next_gesture(&button, move || {
                let Some(index) = *gesture_track_index.borrow() else {
                    return false;
                };
                queue_existing_track_next(&gesture_state, index)
            });
        }

        list.append(&button);
        rows.push(QueueRow {
            button,
            art,
            title,
            artist,
            track_index,
            artwork_url: Rc::new(RefCell::new(None)),
        });
    }

    card.append(&scroll);
    state.borrow_mut().queue_view = Some(Rc::new(QueueView { empty, rows }));
    rebuild_queue_list(&state);

    card
}

pub(crate) fn rebuild_queue_list(state: &Rc<RefCell<UiState>>) {
    let (queue_view, upcoming) = {
        let ui = state.borrow();
        (ui.queue_view.clone(), queued_tracks(&ui))
    };
    if let Some(queue_view) = queue_view {
        queue_view.empty.set_visible(upcoming.is_empty());

        for (row, item) in queue_view.rows.iter().zip(
            upcoming
                .into_iter()
                .map(Some)
                .chain(std::iter::repeat(None)),
        ) {
            let Some((index, track)) = item else {
                row.button.set_visible(false);
                *row.track_index.borrow_mut() = None;
                *row.artwork_url.borrow_mut() = None;
                continue;
            };

            row.button.set_visible(true);
            row.button
                .set_tooltip_text(Some(&format!("Play {}", track.title)));
            *row.track_index.borrow_mut() = Some(index);
            row.title.set_text(&track.title);
            row.artist.set_text(&track.artist);

            if *row.artwork_url.borrow() != track.thumbnail_artwork_url {
                *row.artwork_url.borrow_mut() = track.thumbnail_artwork_url.clone();
                row.art.set_paintable(Option::<&gtk::gdk::Paintable>::None);
                row.art.set_icon_name(Some("audio-x-generic-symbolic"));
                load_queue_art(
                    track.thumbnail_artwork_url,
                    row.art.clone(),
                    row.artwork_url.clone(),
                );
            }
        }
    }

    rebuild_next_up_page(state);
}

pub(crate) fn rebuild_next_up_page(state: &Rc<RefCell<UiState>>) {
    let (next_up_view, upcoming) = {
        let ui = state.borrow();
        (ui.next_up_view.clone(), next_up_tracks(&ui))
    };
    let Some(next_up_view) = next_up_view else {
        return;
    };

    next_up_view.empty.set_visible(upcoming.is_empty());
    while let Some(child) = next_up_view.list.first_child() {
        next_up_view.list.remove(&child);
    }
    next_up_view.rows.borrow_mut().clear();

    for (position, (index, track)) in upcoming.into_iter().enumerate() {
        let row = next_up_row(
            state.clone(),
            position,
            index,
            track,
            next_up_view.rows.clone(),
        );
        next_up_view.list.append(&row);
        next_up_view.rows.borrow_mut().push(row);
    }
}

pub(crate) fn next_up_row(
    state: Rc<RefCell<UiState>>,
    position: usize,
    track_index: usize,
    track: UiTrack,
    rows: Rc<RefCell<Vec<gtk::Button>>>,
) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("flat");
    button.add_css_class("next-up-row");
    button.set_halign(Align::Fill);
    button.set_hexpand(true);
    button.set_cursor_from_name(Some("pointer"));
    button.set_tooltip_text(Some(&format!("Play {}", track.title)));

    let row = gtk::Box::new(Orientation::Horizontal, 14);
    row.set_halign(Align::Fill);
    row.set_hexpand(true);
    row.set_valign(Align::Center);

    let index_label = label(&(position + 1).to_string(), "next-up-index");
    index_label.set_xalign(0.5);
    index_label.set_valign(Align::Center);
    row.append(&index_label);

    let art = cover_art(48);
    art.add_css_class("next-up-art");
    art.set_icon_name(Some("audio-x-generic-symbolic"));
    art.set_valign(Align::Center);
    row.append(&art);

    let text = gtk::Box::new(Orientation::Vertical, 2);
    text.add_css_class("next-up-text");
    text.set_halign(Align::Fill);
    text.set_valign(Align::Center);
    text.set_hexpand(true);
    let title = label(&track.title, "next-up-title");
    title.set_single_line_mode(true);
    title.set_lines(1);
    title.set_valign(Align::End);
    let artist = label(&track.artist, "next-up-artist");
    artist.set_single_line_mode(true);
    artist.set_lines(1);
    artist.set_valign(Align::Start);
    text.append(&title);
    text.append(&artist);
    row.append(&text);

    let trailing = gtk::Box::new(Orientation::Horizontal, 12);
    trailing.add_css_class("next-up-trailing");
    trailing.set_valign(Align::Center);

    let duration = label(&track.duration, "meta");
    duration.add_css_class("next-up-time");
    duration.add_css_class("mono");
    duration.set_xalign(1.0);
    duration.set_valign(Align::Center);
    trailing.append(&duration);

    let handle = gtk::Box::new(Orientation::Horizontal, 0);
    handle.add_css_class("next-up-handle");
    handle.set_valign(Align::Center);
    handle.append(&gtk::Image::from_icon_name("list-drag-handle-symbolic"));
    trailing.append(&handle);

    row.append(&trailing);

    button.set_child(Some(&row));
    {
        let state = state.clone();
        button.connect_clicked(move |_| {
            play_track_at_existing_order(&state, track_index);
        });
    }
    {
        let state = state.clone();
        connect_play_next_gesture(&button, move || {
            queue_existing_track_next(&state, track_index)
        });
    }

    let artwork_url = Rc::new(RefCell::new(track.thumbnail_artwork_url.clone()));
    load_queue_art(
        track.thumbnail_artwork_url.clone(),
        art.clone(),
        artwork_url.clone(),
    );

    let drag_source = gtk::DragSource::new();
    drag_source.set_actions(gtk::gdk::DragAction::MOVE);
    drag_source.connect_prepare(move |_, _, _| {
        Some(gtk::gdk::ContentProvider::for_value(
            &(position as u32).to_value(),
        ))
    });
    {
        let button = button.clone();
        drag_source.connect_drag_begin(move |_, _| {
            button.add_css_class("dragging");
        });
    }
    {
        let button = button.clone();
        let rows_end = rows.clone();
        drag_source.connect_drag_end(move |_, _, _| {
            button.remove_css_class("dragging");
            for row in rows_end.borrow().iter() {
                row.remove_css_class("dodge-up");
                row.remove_css_class("dodge-down");
            }
        });
    }
    button.add_controller(drag_source);

    let drop_target = gtk::DropTarget::new(gtk::glib::Type::U32, gtk::gdk::DragAction::MOVE);
    {
        let button = button.clone();
        let rows_motion = rows.clone();
        drop_target.connect_motion(move |_, _, y| {
            let is_drop_after = y >= f64::from(button.height()) / 2.0;
            button.remove_css_class("drop-before");
            button.remove_css_class("drop-after");
            if is_drop_after {
                button.add_css_class("drop-after");
            } else {
                button.add_css_class("drop-before");
            }
            let insert_at = position + usize::from(is_drop_after);
            let rows_ref = rows_motion.borrow();
            for (i, row) in rows_ref.iter().enumerate() {
                row.remove_css_class("dodge-up");
                row.remove_css_class("dodge-down");
                if i + 1 == insert_at {
                    row.add_css_class("dodge-up");
                } else if i == insert_at {
                    row.add_css_class("dodge-down");
                }
            }
            gtk::gdk::DragAction::MOVE
        });
    }
    {
        let button = button.clone();
        let rows_leave = rows.clone();
        drop_target.connect_leave(move |_| {
            button.remove_css_class("drop-before");
            button.remove_css_class("drop-after");
            for row in rows_leave.borrow().iter() {
                row.remove_css_class("dodge-up");
                row.remove_css_class("dodge-down");
            }
        });
    }
    {
        let state = state.clone();
        let button = button.clone();
        let rows_drop = rows.clone();
        drop_target.connect_drop(move |_, value, _, y| {
            button.remove_css_class("drop-before");
            button.remove_css_class("drop-after");
            for row in rows_drop.borrow().iter() {
                row.remove_css_class("dodge-up");
                row.remove_css_class("dodge-down");
            }
            let Ok(from) = value.get::<u32>() else {
                return false;
            };
            let to_slot = position + usize::from(y >= f64::from(button.height()) / 2.0);
            move_next_up_track(&state, from as usize, to_slot)
        });
    }
    button.add_controller(drop_target);

    button
}

pub(crate) fn next_up_link_button(state: Rc<RefCell<UiState>>) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("flat");
    button.add_css_class("queue-link");
    button.set_halign(Align::Fill);
    button.set_hexpand(true);
    button.set_cursor_from_name(Some("pointer"));
    button.set_tooltip_text(Some("Open the full Next Up queue"));

    let line = gtk::Box::new(Orientation::Horizontal, 8);
    line.set_hexpand(true);
    line.append(&label("Next Up", "rail-title"));
    let spacer = gtk::Box::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    line.append(&spacer);
    line.append(&gtk::Image::from_icon_name("go-next-symbolic"));
    button.set_child(Some(&line));

    button.connect_clicked(move |_| {
        set_library_page(&state, LibraryPage::NextUp);
    });
    button
}
