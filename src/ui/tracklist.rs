use super::prelude::*;

pub(crate) fn scroll_track_list_to_index(state: &Rc<RefCell<UiState>>, index: usize) {
    let (stack, track_count) = {
        let ui = state.borrow();
        (ui.track_stack.clone(), ui.tracks.len())
    };
    if track_count == 0 {
        return;
    }

    let scroll = stack
        .as_ref()
        .and_then(|s| s.child_by_name("list"))
        .and_then(|c| c.downcast::<gtk::ScrolledWindow>().ok());

    if let Some(scroll) = scroll {
        let adj = scroll.vadjustment();
        let row_height = adj.upper() / track_count as f64;
        let target = index as f64 * row_height;
        adj.set_value(target.clamp(0.0, adj.upper() - adj.page_size()));
    }
}

pub(crate) fn focus_track_list(state: &Rc<RefCell<UiState>>) {
    let Some(scroll) = track_list_scroll(state) else {
        return;
    };

    if let Some(list) = scroll
        .child()
        .and_then(|widget| widget.downcast::<gtk::ColumnView>().ok())
    {
        list.grab_focus();
    } else {
        scroll.grab_focus();
    }
}

pub(crate) fn scroll_track_list_to_top(state: &Rc<RefCell<UiState>>) {
    if let Some(scroll) = track_list_scroll(state) {
        scroll.vadjustment().set_value(0.0);
    }
}

pub(crate) fn track_list_scroll(state: &Rc<RefCell<UiState>>) -> Option<gtk::ScrolledWindow> {
    state
        .borrow()
        .track_stack
        .as_ref()
        .and_then(|stack| stack.child_by_name("list"))
        .and_then(|child| child.downcast::<gtk::ScrolledWindow>().ok())
}

pub(crate) const TITLE_WIDTH: i32 = 260;

pub(crate) const ARTIST_WIDTH: i32 = 160;

pub(crate) const ALBUM_WIDTH: i32 = 220;

pub(crate) const DURATION_WIDTH: i32 = 66;

#[derive(Clone, Copy)]
pub(crate) struct TrackColumn {
    pub(crate) header: &'static str,
    pub(crate) width: i32,
    pub(crate) expand: bool,
    pub(crate) xalign: f32,
    pub(crate) sort_column: SortColumn,
    pub(crate) class_name: Option<&'static str>,
}

pub(crate) const TRACK_COLUMNS: [TrackColumn; 4] = [
    TrackColumn {
        header: "Title",
        width: TITLE_WIDTH,
        expand: true,
        xalign: 0.0,
        sort_column: SortColumn::Title,
        class_name: Some("track-title"),
    },
    TrackColumn {
        header: "Artist",
        width: ARTIST_WIDTH,
        expand: true,
        xalign: 0.0,
        sort_column: SortColumn::Artist,
        class_name: None,
    },
    TrackColumn {
        header: "Album",
        width: ALBUM_WIDTH,
        expand: true,
        xalign: 0.0,
        sort_column: SortColumn::Album,
        class_name: None,
    },
    TrackColumn {
        header: "Time",
        width: DURATION_WIDTH,
        expand: false,
        xalign: 1.0,
        sort_column: SortColumn::Duration,
        class_name: Some("mono"),
    },
];

pub(crate) fn track_table(state: Rc<RefCell<UiState>>) -> gtk::Box {
    let wrapper = gtk::Box::new(Orientation::Vertical, 0);
    wrapper.set_hexpand(true);
    wrapper.set_vexpand(true);

    let stack = gtk::Stack::new();
    stack.set_hexpand(true);
    stack.set_vexpand(true);

    let scroll = gtk::ScrolledWindow::new();
    scroll.add_css_class("track-scroll");
    scroll.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
    scroll.set_overlay_scrolling(true);
    scroll.set_hexpand(true);
    scroll.set_vexpand(true);

    let model = state.borrow().track_model.clone();
    let selection = gtk::SingleSelection::new(Some(model));
    selection.set_autoselect(false);
    selection.set_can_unselect(false);

    let list = gtk::ColumnView::new(Some(selection.clone()));
    list.add_css_class("track-list");
    list.set_single_click_activate(true);
    list.set_hexpand(true);
    list.set_vexpand(true);
    list.set_show_column_separators(false);
    list.set_show_row_separators(true);
    let current_sort = state.borrow().sort_column;
    let current_direction = if state.borrow().sort_ascending {
        gtk::SortType::Ascending
    } else {
        gtk::SortType::Descending
    };
    let mut active_sort_column = None;
    for column in TRACK_COLUMNS {
        let view_column = track_column_view(column, state.clone());
        if column.sort_column == current_sort {
            active_sort_column = Some(view_column.clone());
        }
        list.append_column(&view_column);
    }
    if let Some(sorter) = list.sorter() {
        let state = state.clone();
        sorter.connect_changed(move |sorter, _| {
            let Ok(sorter) = sorter.clone().downcast::<gtk::ColumnViewSorter>() else {
                return;
            };
            let Some(column) = sorter.primary_sort_column() else {
                return;
            };
            let Some(title) = column.title() else {
                return;
            };
            let Some(sort_column) = sort_column_for_header(title.as_str()) else {
                return;
            };
            let sort_ascending = sorter.primary_sort_order() == gtk::SortType::Ascending;
            set_sort_order(&state, sort_column, sort_ascending);
        });
    }
    if let Some(column) = active_sort_column.as_ref() {
        list.sort_by_column(Some(column), current_direction);
    }
    {
        let state = state.clone();
        list.connect_activate(move |_, position| {
            play_track_at(&state, position as usize);
        });
    }

    scroll.set_child(Some(&list));
    stack.add_named(&scroll, Some("list"));

    let empty_state = gtk::Box::new(Orientation::Vertical, 8);
    empty_state.add_css_class("track-empty-state");
    empty_state.set_valign(Align::Start);
    let empty_icon = gtk::Image::from_icon_name("folder-music-symbolic");
    empty_icon.add_css_class("placeholder-icon");
    empty_icon.set_pixel_size(28);
    empty_icon.set_halign(Align::Start);
    let empty = label("Connect to Jellyfin to load your music", "rail-title");
    let empty_detail = label("Your Jellyfin tracks will appear here after sync.", "meta");
    empty_detail.set_wrap(true);
    empty_detail.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    empty_state.append(&empty_icon);
    empty_state.append(&empty);
    empty_state.append(&empty_detail);
    stack.add_named(&empty_state, Some("empty"));
    stack.set_visible_child_name("empty");

    {
        let mut ui = state.borrow_mut();
        ui.track_selection = Some(selection);
        ui.track_stack = Some(stack.clone());
        ui.track_empty = Some(empty);
        ui.track_empty_detail = Some(empty_detail);
    }
    refresh_track_model(&state);

    wrapper.append(&stack);
    wrapper
}

pub(crate) fn track_column_view(
    column: TrackColumn,
    state: Rc<RefCell<UiState>>,
) -> gtk::ColumnViewColumn {
    let factory = gtk::SignalListItemFactory::new();

    let state_setup = state.clone();
    factory.connect_setup(move |_, list_item| {
        let Some(list_item) = list_item.downcast_ref::<gtk::ListItem>() else {
            return;
        };

        if column.sort_column == SortColumn::Title {
            let (cell, _) = track_title_cell(false);
            list_item.set_child(Some(&cell));
        } else {
            let cell = track_cell_label(column);
            list_item.set_child(Some(&cell));
        }

        if let Some(child) = list_item.child() {
            let state = state_setup.clone();
            let list_item = list_item.clone();
            connect_play_next_gesture(&child, move || {
                queue_visible_track_next(&state, list_item.position() as usize)
            });
        }
    });

    let state_bind = state.clone();
    factory.connect_bind(move |_, list_item| {
        let Some(list_item) = list_item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let position = list_item.position() as usize;
        let (track, is_now_playing) = {
            let ui = state_bind.borrow();
            let Some(track) = ui.tracks.get(position).cloned() else {
                return;
            };
            let key = track_key(&track);
            let is_now_playing =
                ui.playback_session.now_playing_key.as_deref() == Some(key.as_str());
            (track, is_now_playing)
        };

        if column.sort_column == SortColumn::Title {
            bind_title_cell(list_item, &track.title, is_now_playing);
            if let Some(indicator) = get_indicator_image(list_item) {
                let key = track_key(&track);
                let mut ui = state_bind.borrow_mut();
                ui.track_indicators
                    .retain(|(_, existing)| existing != &indicator);
                ui.track_indicators.push((key, indicator));
            }
        } else if let Some(label) = list_item
            .child()
            .and_then(|child| child.downcast::<gtk::Label>().ok())
        {
            label.set_text(track_value(&track, column.sort_column));
        }
    });

    if column.sort_column == SortColumn::Title {
        let state = state.clone();
        factory.connect_unbind(move |_, list_item| {
            let Some(list_item) = list_item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            if let Some(indicator) = get_indicator_image(list_item) {
                state
                    .borrow_mut()
                    .track_indicators
                    .retain(|(_, existing)| existing != &indicator);
            }
        });
    }

    let view_column = gtk::ColumnViewColumn::new(Some(column.header), Some(factory));
    view_column.set_resizable(true);
    view_column.set_expand(column.expand);
    let sorter = gtk::CustomSorter::new(|_, _| gtk::Ordering::Equal);
    view_column.set_sorter(Some(&sorter));
    if column.width > 0 {
        view_column.set_fixed_width(column.width);
    }
    view_column
}

pub(crate) fn track_cell_label(column: TrackColumn) -> gtk::Label {
    let cell = label("", column.class_name.unwrap_or_default());
    cell.add_css_class("track-cell");
    if column.sort_column == SortColumn::Duration {
        cell.add_css_class("track-time-cell");
    }
    cell.set_single_line_mode(true);
    cell.set_lines(1);
    cell.set_xalign(column.xalign);
    cell.set_halign(Align::Fill);
    cell.set_hexpand(true);
    cell
}

pub(crate) fn track_title_cell(is_now_playing: bool) -> (gtk::Box, gtk::Image) {
    let cell = gtk::Box::new(Orientation::Horizontal, 7);
    cell.add_css_class("track-cell");
    cell.add_css_class("track-title-cell");
    cell.set_halign(Align::Fill);
    cell.set_hexpand(true);
    cell.set_valign(Align::Center);

    let indicator = gtk::Image::from_icon_name("media-playback-start-symbolic");
    indicator.add_css_class("now-playing-indicator");
    indicator.set_pixel_size(13);
    indicator.set_size_request(14, -1);
    indicator.set_opacity(if is_now_playing { 1.0 } else { 0.0 });
    cell.append(&indicator);

    let title = label("", "track-title");
    title.set_single_line_mode(true);
    title.set_lines(1);
    title.set_halign(Align::Fill);
    title.set_hexpand(true);
    cell.append(&title);

    (cell, indicator)
}

pub(crate) fn bind_title_cell(list_item: &gtk::ListItem, title: &str, is_now_playing: bool) {
    let Some(cell) = list_item
        .child()
        .and_then(|child| child.downcast::<gtk::Box>().ok())
    else {
        return;
    };

    if let Some(indicator) = cell
        .first_child()
        .and_then(|child| child.downcast::<gtk::Image>().ok())
    {
        indicator.set_opacity(if is_now_playing { 1.0 } else { 0.0 });
    }

    let label = cell
        .first_child()
        .and_then(|child| child.next_sibling())
        .and_then(|child| child.downcast::<gtk::Label>().ok());
    if let Some(label) = label {
        label.set_text(title);
    }
}

pub(crate) fn track_value(track: &UiTrack, column: SortColumn) -> &str {
    match column {
        SortColumn::Title => &track.title,
        SortColumn::Artist => &track.artist,
        SortColumn::Album => &track.album,
        SortColumn::Duration => &track.duration,
    }
}

pub(crate) fn sort_column_for_header(header: &str) -> Option<SortColumn> {
    TRACK_COLUMNS
        .iter()
        .find(|column| column.header == header)
        .map(|column| column.sort_column)
}

pub(crate) fn refresh_track_model(state: &Rc<RefCell<UiState>>) {
    {
        let mut ui = state.borrow_mut();
        ui.track_indicators.clear();
    }
    let (
        model,
        selection,
        stack,
        empty,
        empty_detail,
        track_count,
        selected_index,
        empty_text,
        empty_detail_text,
    ) = {
        let ui = state.borrow();
        let (empty_text, empty_detail_text) = if ui.search_query.is_empty() {
            (
                "Connect to Jellyfin to load your music".to_string(),
                "Your Jellyfin tracks will appear here after sync.".to_string(),
            )
        } else {
            (
                format!("No tracks match \"{}\"", ui.search_query),
                "Try a different search or clear the search field.".to_string(),
            )
        };
        (
            ui.track_model.clone(),
            ui.track_selection.clone(),
            ui.track_stack.clone(),
            ui.track_empty.clone(),
            ui.track_empty_detail.clone(),
            ui.tracks.len(),
            ui.selected_index,
            empty_text,
            empty_detail_text,
        )
    };

    let v_adj = stack
        .as_ref()
        .and_then(|s| s.child_by_name("list"))
        .and_then(|c| c.downcast::<gtk::ScrolledWindow>().ok())
        .map(|s| s.vadjustment());

    let old_val = v_adj.as_ref().map(|a| a.value());

    let additions = vec![""; track_count];
    model.splice(0, model.n_items(), &additions);

    if let Some(empty) = empty.as_ref() {
        empty.set_text(&empty_text);
    }
    if let Some(empty_detail) = empty_detail.as_ref() {
        empty_detail.set_text(&empty_detail_text);
    }
    if let Some(stack) = stack.as_ref() {
        stack.set_visible_child_name(if track_count == 0 { "empty" } else { "list" });
    }

    gtk::glib::idle_add_local(move || {
        if let Some(selection) = selection.as_ref()
            && track_count > 0
        {
            selection.set_selected(selected_index.min(track_count.saturating_sub(1)) as u32);
        }
        if let (Some(adj), Some(val)) = (v_adj.as_ref(), old_val) {
            adj.set_value(val);
        }
        gtk::glib::ControlFlow::Break
    });

    rebuild_queue_list(state);
}

pub(crate) fn update_list_indicators(state: &Rc<RefCell<UiState>>) {
    let ui = state.borrow();
    let now_playing_key = ui.playback_session.now_playing_key.as_deref();

    for (track_key_value, indicator) in &ui.track_indicators {
        let is_playing = now_playing_key == Some(track_key_value.as_str());
        indicator.set_opacity(if is_playing { 1.0 } else { 0.0 });
    }
}

pub(crate) fn get_indicator_image(list_item: &gtk::ListItem) -> Option<gtk::Image> {
    list_item
        .child()
        .and_then(|child| child.downcast::<gtk::Box>().ok())
        .and_then(|cell| cell.first_child())
        .and_then(|child| child.downcast::<gtk::Image>().ok())
}

pub(crate) fn set_sort_order(state: &Rc<RefCell<UiState>>, column: SortColumn, ascending: bool) {
    let view_settings = {
        let mut ui = state.borrow_mut();
        if ui.sort_column == column && ui.sort_ascending == ascending {
            return;
        }
        ui.sort_column = column;
        ui.sort_ascending = ascending;
        let view_settings = LibraryViewSettings {
            sort_column: ui.sort_column,
            sort_ascending: ui.sort_ascending,
        };
        apply_track_filter(&mut ui, None);
        update_now_playing_labels(&ui);
        update_play_button(&ui);
        update_page_summary(&ui);
        view_settings
    };
    save_library_view_settings(view_settings);
    refresh_track_model(state);
    load_selected_cover_art(state);
    load_selected_waveform(state);
}

pub(crate) fn select_track_model_row(state: &Rc<RefCell<UiState>>, index: usize) {
    let (selection, track_count) = {
        let ui = state.borrow();
        (ui.track_selection.clone(), ui.tracks.len())
    };
    if let Some(selection) = selection
        && track_count > 0
        && selection.selected() != index as u32
    {
        selection.set_selected(index.min(track_count - 1) as u32);
    }
}

pub(crate) fn scroll_to_now_playing(state: &Rc<RefCell<UiState>>) {
    let needs_tracks_page = {
        let ui = state.borrow();
        ui.active_page != LibraryPage::Tracks
    };
    if needs_tracks_page {
        set_library_page(state, LibraryPage::Tracks);
        let state = state.clone();
        gtk::glib::idle_add_local(move || {
            scroll_to_now_playing(&state);
            gtk::glib::ControlFlow::Break
        });
        return;
    }

    let idx = {
        let ui = state.borrow();
        ui.tracks.iter().position(|t| {
            ui.playback_session
                .now_playing_key
                .as_deref()
                .is_some_and(|key| track_has_key(t, key))
        })
    };

    let Some(idx) = idx else {
        return;
    };

    {
        let mut ui = state.borrow_mut();
        ui.selected_index = idx;
        update_now_playing_labels(&ui);
        update_play_button(&ui);
    }
    select_track_model_row(state, idx);
    rebuild_queue_list(state);
    load_selected_cover_art(state);
    load_selected_waveform(state);
    update_list_indicators(state);

    scroll_track_list_to_index(state, idx);
}
