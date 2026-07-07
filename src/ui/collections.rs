use super::prelude::*;

pub(crate) const ALBUM_ART_SIZE: i32 = 168;

pub(crate) const COLLECTION_TILE_WIDTH: i32 = 184;

pub(crate) const ARTIST_ART_SIZE: i32 = 148;

pub(crate) const COLLECTION_TILE_INITIAL_BATCH: usize = 24;

pub(crate) const COLLECTION_TILE_IDLE_BATCH: usize = 24;

pub(crate) const COLLECTION_RETURN_HIGHLIGHT_MS: u64 = 850;

pub(crate) fn focus_collection_item(state: &Rc<RefCell<UiState>>, index: usize) {
    let (grid, visible_content) = {
        let ui = state.borrow();
        let grid = match visible_library_content(&ui) {
            VisibleLibraryContent::Albums => ui.album_grid.clone(),
            VisibleLibraryContent::Artists => ui.artist_grid.clone(),
            VisibleLibraryContent::Playlists => ui.playlist_grid.clone(),
            VisibleLibraryContent::Radio => None,
            VisibleLibraryContent::Tracks | VisibleLibraryContent::NextUp => None,
        };
        (grid, visible_library_content(&ui))
    };
    if matches!(
        visible_content,
        VisibleLibraryContent::Tracks | VisibleLibraryContent::NextUp
    ) {
        select_track_for_navigation(state, index);
        return;
    }

    let Some(grid) = grid else {
        return;
    };
    let Some(child) = grid.child_at_index(index as i32) else {
        return;
    };

    if let Some(button) = collection_child_button(&child) {
        button.grab_focus();
    } else {
        child.grab_focus();
    }
}

pub(crate) fn has_collection_detail_open(state: &Rc<RefCell<UiState>>) -> bool {
    let ui = state.borrow();
    ui.album_filter.is_some() || ui.artist_filter.is_some() || ui.playlist_filter.is_some()
}

pub(crate) fn scroll_active_collection_grid_to_top(state: &Rc<RefCell<UiState>>) {
    let Some(grid) = active_collection_grid(state) else {
        return;
    };

    if let Some(scroll) = collection_grid_scroll(&grid) {
        scroll.vadjustment().set_value(0.0);
    }
}

pub(crate) fn collection_grid_scroll(grid: &gtk::FlowBox) -> Option<gtk::ScrolledWindow> {
    let mut parent = grid.parent();
    while let Some(widget) = parent {
        if let Ok(scroll) = widget.clone().downcast::<gtk::ScrolledWindow>() {
            return Some(scroll);
        }
        parent = widget.parent();
    }
    None
}

pub(crate) fn activate_focused_collection_item(state: &Rc<RefCell<UiState>>) -> bool {
    let Some(grid) = active_collection_grid(state) else {
        return false;
    };

    let mut child = grid.first_child();
    while let Some(widget) = child {
        if widget_has_focus_within(&widget)
            && let Some(button) = widget
                .clone()
                .downcast::<gtk::FlowBoxChild>()
                .ok()
                .and_then(|child| collection_child_button(&child))
                .or_else(|| widget.clone().downcast::<gtk::Button>().ok())
        {
            button.emit_clicked();
            return true;
        }
        child = widget.next_sibling();
    }

    false
}

pub(crate) fn collection_child_button(child: &gtk::FlowBoxChild) -> Option<gtk::Button> {
    child
        .first_child()
        .and_then(|widget| widget.downcast::<gtk::Button>().ok())
}

pub(crate) fn active_collection_grid(state: &Rc<RefCell<UiState>>) -> Option<gtk::FlowBox> {
    let ui = state.borrow();
    match visible_library_content(&ui) {
        VisibleLibraryContent::Albums => ui.album_grid.clone(),
        VisibleLibraryContent::Artists => ui.artist_grid.clone(),
        VisibleLibraryContent::Playlists => ui.playlist_grid.clone(),
        VisibleLibraryContent::Radio => None,
        VisibleLibraryContent::Tracks | VisibleLibraryContent::NextUp => None,
    }
}

pub(crate) fn active_collection_grid_and_content(
    state: &Rc<RefCell<UiState>>,
) -> Option<(VisibleLibraryContent, gtk::FlowBox)> {
    let ui = state.borrow();
    let content = visible_library_content(&ui);
    let grid = match content {
        VisibleLibraryContent::Albums => ui.album_grid.clone(),
        VisibleLibraryContent::Artists => ui.artist_grid.clone(),
        VisibleLibraryContent::Playlists => ui.playlist_grid.clone(),
        VisibleLibraryContent::Radio => None,
        VisibleLibraryContent::Tracks | VisibleLibraryContent::NextUp => None,
    }?;
    Some((content, grid))
}

pub(crate) fn collection_return_target_for_key(
    state: &Rc<RefCell<UiState>>,
    key: String,
) -> Option<CollectionReturnTarget> {
    let (content, _) = active_collection_grid_and_content(state)?;
    Some(CollectionReturnTarget { content, key })
}

pub(crate) fn save_active_collection_scroll_position(state: &Rc<RefCell<UiState>>) {
    let Some((content, grid)) = active_collection_grid_and_content(state) else {
        return;
    };
    let Some(scroll) = collection_grid_scroll(&grid) else {
        return;
    };
    let value = scroll.vadjustment().value();
    let mut ui = state.borrow_mut();
    set_collection_scroll_value(&mut ui, content, value);
}

pub(crate) fn restore_active_collection_scroll_position(state: &Rc<RefCell<UiState>>) {
    let Some((content, grid)) = active_collection_grid_and_content(state) else {
        return;
    };
    let Some(scroll) = collection_grid_scroll(&grid) else {
        return;
    };
    let value = {
        let ui = state.borrow();
        collection_scroll_value(&ui, content)
    };
    restore_collection_scroll(scroll, value);
}

pub(crate) fn set_collection_scroll_value(
    ui: &mut UiState,
    content: VisibleLibraryContent,
    value: f64,
) {
    match content {
        VisibleLibraryContent::Albums => ui.album_grid_scroll_value = value,
        VisibleLibraryContent::Artists => ui.artist_grid_scroll_value = value,
        VisibleLibraryContent::Playlists => ui.playlist_grid_scroll_value = value,
        VisibleLibraryContent::Radio
        | VisibleLibraryContent::Tracks
        | VisibleLibraryContent::NextUp => {}
    }
}

pub(crate) fn collection_scroll_value(ui: &UiState, content: VisibleLibraryContent) -> f64 {
    match content {
        VisibleLibraryContent::Albums => ui.album_grid_scroll_value,
        VisibleLibraryContent::Artists => ui.artist_grid_scroll_value,
        VisibleLibraryContent::Playlists => ui.playlist_grid_scroll_value,
        VisibleLibraryContent::Radio
        | VisibleLibraryContent::Tracks
        | VisibleLibraryContent::NextUp => 0.0,
    }
}

pub(crate) fn restore_collection_scroll(scroll: gtk::ScrolledWindow, value: f64) {
    let attempts = Rc::new(Cell::new(0usize));
    gtk::glib::idle_add_local(move || {
        let adj = scroll.vadjustment();
        let max = (adj.upper() - adj.page_size()).max(0.0);
        adj.set_value(value.clamp(0.0, max));

        let attempt = attempts.get() + 1;
        attempts.set(attempt);
        if value <= max || attempt >= 60 {
            gtk::glib::ControlFlow::Break
        } else {
            gtk::glib::ControlFlow::Continue
        }
    });
}

pub(crate) fn pulse_collection_return_target(
    state: &Rc<RefCell<UiState>>,
    target: Option<CollectionReturnTarget>,
) {
    let Some(target) = target else {
        return;
    };

    let state = state.clone();
    let attempts = Rc::new(Cell::new(0usize));
    gtk::glib::idle_add_local(move || {
        if let Some(button) = collection_return_target_button(&state, &target) {
            button.add_css_class("return-highlight");
            gtk::glib::timeout_add_local_once(
                Duration::from_millis(COLLECTION_RETURN_HIGHLIGHT_MS),
                move || {
                    button.remove_css_class("return-highlight");
                },
            );
            return gtk::glib::ControlFlow::Break;
        }

        let attempt = attempts.get() + 1;
        attempts.set(attempt);
        if attempt >= 60 {
            gtk::glib::ControlFlow::Break
        } else {
            gtk::glib::ControlFlow::Continue
        }
    });
}

pub(crate) fn collection_return_target_button(
    state: &Rc<RefCell<UiState>>,
    target: &CollectionReturnTarget,
) -> Option<gtk::Button> {
    let (grid, index) = {
        let ui = state.borrow();
        if visible_library_content(&ui) != target.content {
            return None;
        }
        let grid = collection_grid_for_content(&ui, target.content)?;
        let index = collection_return_target_index(&ui, target)?;
        (grid, index)
    };

    grid.child_at_index(index as i32)
        .and_then(|child| collection_child_button(&child))
}

pub(crate) fn collection_grid_for_content(
    ui: &UiState,
    content: VisibleLibraryContent,
) -> Option<gtk::FlowBox> {
    match content {
        VisibleLibraryContent::Albums => ui.album_grid.clone(),
        VisibleLibraryContent::Artists => ui.artist_grid.clone(),
        VisibleLibraryContent::Playlists => ui.playlist_grid.clone(),
        VisibleLibraryContent::Radio
        | VisibleLibraryContent::Tracks
        | VisibleLibraryContent::NextUp => None,
    }
}

pub(crate) fn collection_return_target_index(
    ui: &UiState,
    target: &CollectionReturnTarget,
) -> Option<usize> {
    match target.content {
        VisibleLibraryContent::Albums => visible_album_summaries(ui)
            .iter()
            .position(|album| album.key == target.key),
        VisibleLibraryContent::Artists => {
            filter_artist_summaries(&ui.library_artists, &ui.search_query)
                .iter()
                .position(|artist| artist.key == target.key)
        }
        VisibleLibraryContent::Playlists => filter_playlists(&ui.playlists, &ui.search_query)
            .iter()
            .position(|playlist| playlist.id == target.key),
        VisibleLibraryContent::Radio
        | VisibleLibraryContent::Tracks
        | VisibleLibraryContent::NextUp => None,
    }
}

pub(crate) fn album_grid_page(state: Rc<RefCell<UiState>>) -> gtk::ScrolledWindow {
    let scroll = gtk::ScrolledWindow::new();
    scroll.add_css_class("collection-scroll");
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_overlay_scrolling(true);
    scroll.set_hexpand(true);
    scroll.set_vexpand(true);

    let flow = gtk::FlowBox::new();
    flow.add_css_class("collection-grid");
    flow.add_css_class("album-grid");
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow.set_min_children_per_line(1);
    flow.set_max_children_per_line(8);
    flow.set_row_spacing(18);
    flow.set_column_spacing(18);
    flow.set_homogeneous(false);
    flow.set_valign(Align::Start);
    scroll.set_child(Some(&flow));

    state.borrow_mut().album_grid = Some(flow);
    scroll
}

pub(crate) fn artist_grid_page(state: Rc<RefCell<UiState>>) -> gtk::ScrolledWindow {
    let scroll = gtk::ScrolledWindow::new();
    scroll.add_css_class("collection-scroll");
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_overlay_scrolling(true);
    scroll.set_hexpand(true);
    scroll.set_vexpand(true);

    let flow = gtk::FlowBox::new();
    flow.add_css_class("collection-grid");
    flow.add_css_class("artist-grid");
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow.set_min_children_per_line(1);
    flow.set_max_children_per_line(8);
    flow.set_row_spacing(18);
    flow.set_column_spacing(18);
    flow.set_homogeneous(true);
    flow.set_valign(Align::Start);
    scroll.set_child(Some(&flow));

    state.borrow_mut().artist_grid = Some(flow);
    scroll
}

pub(crate) fn playlist_grid_page(state: Rc<RefCell<UiState>>) -> gtk::ScrolledWindow {
    let scroll = gtk::ScrolledWindow::new();
    scroll.add_css_class("collection-scroll");
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_overlay_scrolling(true);
    scroll.set_hexpand(true);
    scroll.set_vexpand(true);

    let flow = gtk::FlowBox::new();
    flow.add_css_class("collection-grid");
    flow.add_css_class("playlist-grid");
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow.set_min_children_per_line(1);
    flow.set_max_children_per_line(8);
    flow.set_row_spacing(18);
    flow.set_column_spacing(18);
    flow.set_homogeneous(false);
    flow.set_valign(Align::Start);
    scroll.set_child(Some(&flow));

    state.borrow_mut().playlist_grid = Some(flow);
    scroll
}

pub(crate) fn refresh_collection_grids(state: &Rc<RefCell<UiState>>) {
    refresh_album_grid(state);
    refresh_artist_grid(state);
    refresh_playlist_grid(state);
    update_nav_counts(state);
}

pub(crate) fn refresh_visible_collection_grid(state: &Rc<RefCell<UiState>>) {
    let active_view = {
        let ui = state.borrow();
        (
            ui.active_page,
            ui.album_filter.clone(),
            ui.artist_filter.clone(),
            ui.playlist_filter.clone(),
        )
    };

    match active_view {
        (LibraryPage::Albums, None, _, _) => refresh_album_grid(state),
        (LibraryPage::Artists, _, None, _) => refresh_artist_grid(state),
        (LibraryPage::Artists, None, Some(_), _) => refresh_album_grid(state),
        (LibraryPage::Playlists, _, _, None) => refresh_playlist_grid(state),
        (LibraryPage::NextUp, _, _, _) => {}
        _ => {}
    }
}

pub(crate) fn refresh_album_grid(state: &Rc<RefCell<UiState>>) {
    let (grid, albums) = {
        let ui = state.borrow();
        let albums = if ui.active_page == LibraryPage::Artists {
            ui.artist_filter
                .as_deref()
                .map(|selected_artist_key| {
                    album_summaries_for_artist_from(
                        &ui.library_albums,
                        selected_artist_key,
                        &ui.search_query,
                    )
                })
                .unwrap_or_else(|| filter_album_summaries(&ui.library_albums, &ui.search_query))
        } else {
            filter_album_summaries(&ui.library_albums, &ui.search_query)
        };
        (ui.album_grid.clone(), albums)
    };
    let Some(grid) = grid else {
        return;
    };
    next_collection_render_generation(state);
    clear_flow_box(&grid);

    if albums.is_empty() {
        grid.insert(&collection_empty_state("No albums found"), -1);
        return;
    }

    render_album_tiles_batched(state, grid, albums);
}

pub(crate) fn refresh_artist_grid(state: &Rc<RefCell<UiState>>) {
    let (grid, artists) = {
        let ui = state.borrow();
        (
            ui.artist_grid.clone(),
            filter_artist_summaries(&ui.library_artists, &ui.search_query),
        )
    };
    let Some(grid) = grid else {
        return;
    };
    next_collection_render_generation(state);
    clear_flow_box(&grid);

    if artists.is_empty() {
        grid.insert(&collection_empty_state("No artists found"), -1);
        return;
    }

    render_artist_tiles_batched(state, grid, artists);
}

pub(crate) fn refresh_playlist_grid(state: &Rc<RefCell<UiState>>) {
    let (grid, playlists) = {
        let ui = state.borrow();
        (
            ui.playlist_grid.clone(),
            filter_playlists(&ui.playlists, &ui.search_query),
        )
    };
    let Some(grid) = grid else {
        return;
    };
    next_collection_render_generation(state);
    clear_flow_box(&grid);

    if playlists.is_empty() {
        grid.insert(&collection_empty_state("No playlists found"), -1);
        return;
    }

    render_playlist_tiles_batched(state, grid, playlists);
}

pub(crate) fn clear_flow_box(flow: &gtk::FlowBox) {
    while let Some(child) = flow.first_child() {
        flow.remove(&child);
    }
}

pub(crate) fn next_collection_render_generation(state: &Rc<RefCell<UiState>>) -> u64 {
    let mut ui = state.borrow_mut();
    ui.collection_render_generation = ui.collection_render_generation.wrapping_add(1);
    ui.collection_render_generation
}

pub(crate) fn collection_render_generation(state: &Rc<RefCell<UiState>>) -> u64 {
    state.borrow().collection_render_generation
}

pub(crate) fn render_album_tiles_batched(
    state: &Rc<RefCell<UiState>>,
    grid: gtk::FlowBox,
    mut albums: Vec<AlbumSummary>,
) {
    let generation = next_collection_render_generation(state);
    let initial_count = albums.len().min(COLLECTION_TILE_INITIAL_BATCH);
    for (index, album) in albums.drain(..initial_count).enumerate() {
        grid.insert(&album_tile(album, state.clone(), index), -1);
    }

    if albums.is_empty() {
        return;
    }

    let state = state.clone();
    let mut next_index = initial_count;
    gtk::glib::idle_add_local(move || {
        if collection_render_generation(&state) != generation {
            return gtk::glib::ControlFlow::Break;
        }

        let batch_count = albums.len().min(COLLECTION_TILE_IDLE_BATCH);
        for album in albums.drain(..batch_count) {
            grid.insert(&album_tile(album, state.clone(), next_index), -1);
            next_index += 1;
        }

        if albums.is_empty() {
            gtk::glib::ControlFlow::Break
        } else {
            gtk::glib::ControlFlow::Continue
        }
    });
}

pub(crate) fn render_artist_tiles_batched(
    state: &Rc<RefCell<UiState>>,
    grid: gtk::FlowBox,
    mut artists: Vec<ArtistSummary>,
) {
    let generation = next_collection_render_generation(state);
    let initial_count = artists.len().min(COLLECTION_TILE_INITIAL_BATCH);
    for (index, artist) in artists.drain(..initial_count).enumerate() {
        grid.insert(&artist_tile(artist, state.clone(), index), -1);
    }

    if artists.is_empty() {
        return;
    }

    let state = state.clone();
    let mut next_index = initial_count;
    gtk::glib::idle_add_local(move || {
        if collection_render_generation(&state) != generation {
            return gtk::glib::ControlFlow::Break;
        }

        let batch_count = artists.len().min(COLLECTION_TILE_IDLE_BATCH);
        for artist in artists.drain(..batch_count) {
            grid.insert(&artist_tile(artist, state.clone(), next_index), -1);
            next_index += 1;
        }

        if artists.is_empty() {
            gtk::glib::ControlFlow::Break
        } else {
            gtk::glib::ControlFlow::Continue
        }
    });
}

pub(crate) fn render_playlist_tiles_batched(
    state: &Rc<RefCell<UiState>>,
    grid: gtk::FlowBox,
    mut playlists: Vec<UiPlaylist>,
) {
    let generation = next_collection_render_generation(state);
    let initial_count = playlists.len().min(COLLECTION_TILE_INITIAL_BATCH);
    for (index, playlist) in playlists.drain(..initial_count).enumerate() {
        grid.insert(&playlist_tile(playlist, state.clone(), index), -1);
    }

    if playlists.is_empty() {
        return;
    }

    let state = state.clone();
    let mut next_index = initial_count;
    gtk::glib::idle_add_local(move || {
        if collection_render_generation(&state) != generation {
            return gtk::glib::ControlFlow::Break;
        }

        let batch_count = playlists.len().min(COLLECTION_TILE_IDLE_BATCH);
        for playlist in playlists.drain(..batch_count) {
            grid.insert(&playlist_tile(playlist, state.clone(), next_index), -1);
            next_index += 1;
        }

        if playlists.is_empty() {
            gtk::glib::ControlFlow::Break
        } else {
            gtk::glib::ControlFlow::Continue
        }
    });
}

pub(crate) fn collection_empty_state(text: &str) -> gtk::Box {
    let empty = gtk::Box::new(Orientation::Vertical, 8);
    empty.add_css_class("collection-empty");
    empty.append(&label(text, "rail-title"));
    empty.append(&label(
        "Try a different search or connect to Jellyfin.",
        "meta",
    ));
    empty
}

pub(crate) fn album_tile(
    album: AlbumSummary,
    state: Rc<RefCell<UiState>>,
    tile_index: usize,
) -> gtk::Button {
    let button = collection_tile_button(&album.name);
    button.add_css_class("album-tile");
    button.set_halign(Align::Fill);
    button.set_hexpand(false);
    button.set_size_request(COLLECTION_TILE_WIDTH, -1);

    let layout = gtk::Box::new(Orientation::Vertical, 8);
    layout.set_halign(Align::Fill);
    layout.set_hexpand(true);

    let frame = gtk::Box::new(Orientation::Vertical, 0);
    frame.add_css_class("album-art-frame");
    frame.set_size_request(ALBUM_ART_SIZE, ALBUM_ART_SIZE);
    frame.set_halign(Align::Center);
    frame.set_valign(Align::Fill);
    frame.set_overflow(gtk::Overflow::Hidden);

    let art = cover_art(ALBUM_ART_SIZE);
    art.add_css_class("collection-art");
    art.add_css_class("album-art");
    art.set_halign(Align::Fill);
    art.set_valign(Align::Fill);
    art.set_icon_name(Some("audio-x-generic-symbolic"));
    if let Some(url) = album.artwork_url.clone() {
        art.add_css_class("artwork-loading");
        let current_url = Rc::new(RefCell::new(Some(url.clone())));
        load_collection_queue_art(Some(url), art.clone(), current_url, tile_index);
    }
    frame.append(&art);
    layout.append(&frame);
    layout.append(&collection_tile_label(&album.name, "collection-title"));
    layout.append(&collection_tile_label(
        &format!(
            "{} | {}",
            album.artist,
            count_text(album.song_count, "song", "songs")
        ),
        "collection-subtitle",
    ));
    button.set_child(Some(&layout));

    button.connect_clicked(move |_| {
        show_album_tracks(&state, &album);
    });
    button
}

pub(crate) fn artist_tile(
    artist: ArtistSummary,
    state: Rc<RefCell<UiState>>,
    tile_index: usize,
) -> gtk::Button {
    let button = collection_tile_button(&artist.name);
    button.add_css_class("artist-tile");

    let layout = gtk::Box::new(Orientation::Vertical, 8);
    layout.set_halign(Align::Fill);
    layout.set_hexpand(true);

    let avatar = gtk::Image::from_icon_name("avatar-default-symbolic");
    avatar.add_css_class("artist-art");
    avatar.add_css_class("artist-placeholder");
    avatar.set_size_request(ARTIST_ART_SIZE, ARTIST_ART_SIZE);
    avatar.set_halign(Align::Center);
    avatar.set_overflow(gtk::Overflow::Hidden);
    avatar.set_pixel_size(56);
    if let Some(url) = artist.image_url.clone() {
        avatar.add_css_class("artwork-loading");
        load_collection_picture_art(url, avatar.clone(), tile_index);
    }
    layout.append(&avatar);
    layout.append(&collection_tile_label(&artist.name, "collection-title"));
    layout.append(&collection_tile_label(
        &artist_count_text(artist.album_count, artist.song_count),
        "meta",
    ));
    button.set_child(Some(&layout));

    button.connect_clicked(move |_| {
        show_artist_albums(&state, &artist);
    });
    button
}

pub(crate) fn playlist_tile(
    playlist: UiPlaylist,
    state: Rc<RefCell<UiState>>,
    tile_index: usize,
) -> gtk::Button {
    let button = collection_tile_button(&playlist.name);
    button.add_css_class("playlist-tile");
    button.set_halign(Align::Fill);
    button.set_hexpand(false);
    button.set_size_request(COLLECTION_TILE_WIDTH, -1);

    let layout = gtk::Box::new(Orientation::Vertical, 8);
    layout.set_halign(Align::Fill);
    layout.set_hexpand(true);

    let frame = gtk::Box::new(Orientation::Vertical, 0);
    frame.add_css_class("album-art-frame");
    frame.set_size_request(ALBUM_ART_SIZE, ALBUM_ART_SIZE);
    frame.set_halign(Align::Center);
    frame.set_valign(Align::Fill);
    frame.set_overflow(gtk::Overflow::Hidden);

    let art = cover_art(ALBUM_ART_SIZE);
    art.add_css_class("collection-art");
    art.add_css_class("album-art");
    art.set_halign(Align::Fill);
    art.set_valign(Align::Fill);
    art.set_icon_name(Some("media-playlist-consecutive-symbolic"));
    if let Some(url) = playlist.thumbnail_artwork_url.clone() {
        art.add_css_class("artwork-loading");
        let current_url = Rc::new(RefCell::new(Some(url.clone())));
        load_collection_queue_art(Some(url), art.clone(), current_url, tile_index);
    }
    frame.append(&art);
    layout.append(&frame);
    layout.append(&collection_tile_label(&playlist.name, "collection-title"));
    layout.append(&collection_tile_label(
        &count_text(playlist.tracks.len(), "song", "songs"),
        "collection-subtitle",
    ));
    button.set_child(Some(&layout));

    button.connect_clicked(move |_| {
        show_playlist_tracks(&state, &playlist);
    });
    button
}

pub(crate) fn collection_tile_button(title: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("collection-tile");
    button.set_halign(Align::Fill);
    button.set_valign(Align::Start);
    button.set_size_request(COLLECTION_TILE_WIDTH, -1);
    button.set_tooltip_text(Some(title));
    button.set_cursor_from_name(Some("pointer"));
    button
}

pub(crate) fn collection_tile_label(text: &str, class_name: &str) -> gtk::Label {
    let title = label(text, class_name);
    title.set_xalign(0.5);
    title.set_justify(gtk::Justification::Center);
    title.set_single_line_mode(true);
    title.set_lines(1);
    title.set_halign(Align::Fill);
    title.set_hexpand(true);
    title.set_width_chars(1);
    title.set_max_width_chars(24);
    title
}

pub(crate) fn set_search_query(state: &Rc<RefCell<UiState>>, query: &str) {
    let show_tracks = state.borrow().is_track_list_visible();
    {
        let mut ui = state.borrow_mut();
        if ui.search_query == query {
            return;
        }
        ui.search_query = query.to_string();
        if show_tracks {
            let selected_key = ui.tracks.get(ui.selected_index).map(track_key);
            apply_track_filter(&mut ui, selected_key.as_deref());
            update_now_playing_labels(&ui);
            update_play_button(&ui);
        }
        update_page_summary(&ui);
    }
    if show_tracks {
        refresh_track_model(state);
    }
    refresh_visible_collection_grid(state);
    update_content_view(state, NavDirection::DrillForward);
    if show_tracks {
        load_selected_cover_art(state);
        load_selected_waveform(state);
    }
}

pub(crate) fn set_library_page(state: &Rc<RefCell<UiState>>, page: LibraryPage) {
    let show_tracks = page == LibraryPage::Tracks;
    let mut refresh_tracks = false;
    let old_page;
    {
        let mut ui = state.borrow_mut();
        if ui.active_page == page
            && ui.album_filter.is_none()
            && ui.artist_filter.is_none()
            && ui.playlist_filter.is_none()
            && ui.collection_detail_title.is_none()
            && ui.collection_detail_subtitle.is_none()
            && ui.collection_detail_parent_search_query.is_none()
            && ui.collection_return_target.is_none()
            && ui.collection_parent_return_target.is_none()
        {
            return;
        }
        old_page = ui.active_page;
        ui.active_page = page;
        ui.album_filter = None;
        ui.artist_filter = None;
        ui.playlist_filter = None;
        ui.collection_detail_title = None;
        ui.collection_detail_subtitle = None;
        ui.collection_detail_parent_search_query = None;
        ui.collection_return_target = None;
        ui.collection_parent_return_target = None;
        if show_tracks {
            if !ui.track_filter_is_current() {
                let selected_key = ui.tracks.get(ui.selected_index).map(track_key);
                apply_track_filter(&mut ui, selected_key.as_deref());
                refresh_tracks = true;
            }
            update_now_playing_labels(&ui);
            update_play_button(&ui);
        }
        update_page_summary(&ui);
    }
    let direction = if library_page_order(page) >= library_page_order(old_page) {
        NavDirection::PageForward
    } else {
        NavDirection::PageBackward
    };
    if refresh_tracks {
        refresh_track_model(state);
    }
    refresh_visible_collection_grid(state);
    update_nav_selection(state);
    update_content_view(state, direction);
    focus_active_collection_grid(state);
    if refresh_tracks {
        load_selected_cover_art(state);
        load_selected_waveform(state);
    }
}

pub(crate) fn show_album_tracks(state: &Rc<RefCell<UiState>>, album: &AlbumSummary) {
    let selected_key = {
        let ui = state.borrow();
        current_display_track(&ui).and_then(|track| track_key_if_same_album(track, &album.key))
    };
    let return_target = collection_return_target_for_key(state, album.key.clone());
    save_active_collection_scroll_position(state);

    {
        let mut ui = state.borrow_mut();
        let selected_artist = if ui.active_page == LibraryPage::Artists {
            ui.artist_filter.clone()
        } else {
            None
        };
        let parent_return_target = selected_artist
            .is_some()
            .then(|| ui.collection_return_target.clone())
            .flatten();
        ui.active_page = if selected_artist.is_some() {
            LibraryPage::Artists
        } else {
            LibraryPage::Albums
        };
        ui.playlist_filter = None;
        ui.album_filter = Some(album.key.clone());
        ui.artist_filter = selected_artist;
        ui.collection_detail_title = Some(album.name.clone());
        ui.collection_detail_subtitle = Some(format!(
            "{} | {}",
            album.artist,
            count_text(album.song_count, "song", "songs")
        ));
        ui.collection_detail_parent_search_query = Some(ui.search_query.clone());
        ui.collection_return_target = return_target;
        ui.collection_parent_return_target = parent_return_target;
        apply_track_filter(&mut ui, selected_key.as_deref());
        update_now_playing_labels(&ui);
        update_play_button(&ui);
        update_page_summary(&ui);
    }
    refresh_track_model(state);
    update_content_view(state, NavDirection::DrillForward);
    load_selected_cover_art(state);
    load_selected_waveform(state);
}

pub(crate) fn show_playlist_tracks(state: &Rc<RefCell<UiState>>, playlist: &UiPlaylist) {
    let return_target = collection_return_target_for_key(state, playlist.id.clone());
    save_active_collection_scroll_position(state);

    {
        let mut ui = state.borrow_mut();
        ui.active_page = LibraryPage::Playlists;
        ui.album_filter = None;
        ui.artist_filter = None;
        ui.playlist_filter = Some(playlist.id.clone());
        ui.collection_detail_title = Some(playlist.name.clone());
        ui.collection_detail_subtitle = Some(count_text(playlist.tracks.len(), "song", "songs"));
        ui.collection_detail_parent_search_query = Some(ui.search_query.clone());
        ui.collection_return_target = return_target;
        ui.collection_parent_return_target = None;
        ui.selected_index = 0;
        apply_track_filter(&mut ui, None);
        update_now_playing_labels(&ui);
        update_play_button(&ui);
        update_page_summary(&ui);
    }
    refresh_track_model(state);
    update_content_view(state, NavDirection::DrillForward);
    load_selected_cover_art(state);
    load_selected_waveform(state);
}

pub(crate) fn show_artist_albums(state: &Rc<RefCell<UiState>>, artist: &ArtistSummary) {
    let return_target = collection_return_target_for_key(state, artist.key.clone());
    save_active_collection_scroll_position(state);

    {
        let mut ui = state.borrow_mut();
        ui.active_page = LibraryPage::Artists;
        ui.album_filter = None;
        ui.playlist_filter = None;
        ui.artist_filter = Some(artist.key.clone());
        ui.collection_detail_title = Some(artist.name.clone());
        ui.collection_detail_subtitle =
            Some(artist_count_text(artist.album_count, artist.song_count));
        ui.collection_detail_parent_search_query = Some(ui.search_query.clone());
        ui.collection_return_target = return_target;
        ui.collection_parent_return_target = None;
        ui.selected_index = 0;
        ui.page_summary.set_text(&format!(
            "{} | {}",
            artist.name,
            artist_count_text(artist.album_count, artist.song_count)
        ));
    }
    refresh_visible_collection_grid(state);
    update_content_view(state, NavDirection::DrillForward);
    focus_active_collection_grid(state);
}

pub(crate) fn focus_active_collection_grid(state: &Rc<RefCell<UiState>>) {
    let (
        active_page,
        album_filter,
        artist_filter,
        playlist_filter,
        album_grid,
        artist_grid,
        playlist_grid,
    ) = {
        let ui = state.borrow();
        (
            ui.active_page,
            ui.album_filter.clone(),
            ui.artist_filter.clone(),
            ui.playlist_filter.clone(),
            ui.album_grid.clone(),
            ui.artist_grid.clone(),
            ui.playlist_grid.clone(),
        )
    };

    let grid = match active_page {
        LibraryPage::Albums if album_filter.is_none() => album_grid,
        LibraryPage::Artists if artist_filter.is_none() => artist_grid,
        LibraryPage::Artists if album_filter.is_none() => album_grid,
        LibraryPage::Playlists if playlist_filter.is_none() => playlist_grid,
        _ => None,
    };

    let Some(grid) = grid else {
        return;
    };
    let Some(child) = grid.first_child() else {
        return;
    };
    if let Some(button) = child
        .first_child()
        .and_then(|widget| widget.downcast::<gtk::Button>().ok())
    {
        button.grab_focus();
    } else {
        child.grab_focus();
    }
}

pub(crate) fn return_to_collection_grid(state: &Rc<RefCell<UiState>>) {
    let refresh_grid = {
        let ui = state.borrow();
        ui.collection_detail_parent_search_query
            .as_deref()
            .is_none_or(|query| query != ui.search_query)
    };
    let return_target = state.borrow().collection_return_target.clone();

    {
        let mut ui = state.borrow_mut();
        if ui.active_page == LibraryPage::Artists && ui.album_filter.is_some() {
            ui.album_filter = None;
            ui.collection_return_target = ui.collection_parent_return_target.take();
            if let Some(artist_key_value) = ui.artist_filter.clone() {
                let artist_detail = ui
                    .library_artists
                    .iter()
                    .find(|artist| artist.key == artist_key_value)
                    .map(|artist| {
                        (
                            artist.name.clone(),
                            artist_count_text(artist.album_count, artist.song_count),
                        )
                    });
                if let Some((name, subtitle)) = artist_detail {
                    ui.collection_detail_title = Some(name);
                    ui.collection_detail_subtitle = Some(subtitle);
                }
            }
        } else {
            ui.album_filter = None;
            ui.artist_filter = None;
            ui.playlist_filter = None;
            ui.collection_detail_title = None;
            ui.collection_detail_subtitle = None;
            ui.collection_detail_parent_search_query = None;
            ui.collection_return_target = None;
            ui.collection_parent_return_target = None;
        }
        ui.selected_index = 0;
        update_page_summary(&ui);
    }
    if refresh_grid {
        refresh_visible_collection_grid(state);
    }
    update_content_view(state, NavDirection::DrillBackward);
    focus_active_collection_grid(state);
    restore_active_collection_scroll_position(state);
    pulse_collection_return_target(state, return_target);
}

pub(crate) fn navigate_to_now_playing_artist(state: &Rc<RefCell<UiState>>) {
    let artist = {
        let ui = state.borrow();
        if ui.playback_session.mode.is_radio() {
            return;
        }
        let Some(track) = current_display_track(&ui) else {
            return;
        };
        let key = artist_key(&track.artist);
        ui.library_artists
            .iter()
            .find(|artist| artist.key == key)
            .cloned()
    };

    if let Some(artist) = artist {
        set_library_page(state, LibraryPage::Artists);
        show_artist_albums(state, &artist);
    }
}

pub(crate) fn navigate_to_now_playing_album(state: &Rc<RefCell<UiState>>) {
    let album = {
        let ui = state.borrow();
        if ui.playback_session.mode.is_radio() {
            return;
        }
        let Some(track) = current_display_track(&ui) else {
            return;
        };
        let key = album_key(track);
        ui.library_albums
            .iter()
            .find(|album| album.key == key)
            .cloned()
    };

    if let Some(album) = album {
        set_library_page(state, LibraryPage::Albums);
        show_album_tracks(state, &album);
    }
}

pub(crate) fn update_content_view(state: &Rc<RefCell<UiState>>, direction: NavDirection) {
    let (
        stack,
        detail_header,
        title_label,
        subtitle_label,
        visible_child,
        show_detail,
        title,
        subtitle,
    ) = {
        let ui = state.borrow();
        let show_detail =
            ui.album_filter.is_some() || ui.artist_filter.is_some() || ui.playlist_filter.is_some();
        let visible_child = match (ui.active_page, show_detail) {
            (LibraryPage::Tracks, _) => "tracks",
            (LibraryPage::Albums, false) => "albums",
            (LibraryPage::Albums, true) => "tracks",
            (LibraryPage::Artists, false) => "artists",
            (LibraryPage::Artists, true) if ui.album_filter.is_none() => "albums",
            (LibraryPage::Artists, true) => "tracks",
            (LibraryPage::Playlists, false) => "playlists",
            (LibraryPage::Playlists, true) => "tracks",
            (LibraryPage::Radio, _) => "radio",
            (LibraryPage::NextUp, _) => "next-up",
        };
        (
            ui.library_stack.clone(),
            ui.detail_header.clone(),
            ui.detail_title_label.clone(),
            ui.detail_subtitle_label.clone(),
            visible_child.to_string(),
            show_detail,
            ui.collection_detail_title.clone().unwrap_or_default(),
            ui.collection_detail_subtitle.clone().unwrap_or_default(),
        )
    };

    if let Some(stack) = stack.as_ref() {
        stack.set_transition_type(match direction {
            NavDirection::DrillForward => gtk::StackTransitionType::SlideLeft,
            NavDirection::DrillBackward => gtk::StackTransitionType::SlideRight,
            NavDirection::PageForward => gtk::StackTransitionType::SlideUp,
            NavDirection::PageBackward => gtk::StackTransitionType::SlideDown,
        });
        stack.set_visible_child_name(&visible_child);
    }
    if let Some(header) = detail_header.as_ref() {
        header.set_visible(show_detail);
    }
    {
        let (sidebar_queue_card, show_sidebar_queue) = {
            let ui = state.borrow();
            (
                ui.sidebar_queue_card.clone(),
                !matches!(ui.active_page, LibraryPage::NextUp | LibraryPage::Radio),
            )
        };
        if let Some(queue_card) = sidebar_queue_card.as_ref() {
            queue_card.set_visible(show_sidebar_queue);
        }
    }
    if let Some(label) = title_label.as_ref() {
        label.set_text(&title);
    }
    if let Some(label) = subtitle_label.as_ref() {
        label.set_text(&subtitle);
    }
}

pub(crate) fn update_nav_counts(state: &Rc<RefCell<UiState>>) {
    let (
        track_label,
        album_label,
        artist_label,
        playlist_label,
        radio_label,
        tracks,
        albums,
        artists,
        playlists,
        radio_stations,
    ) = {
        let ui = state.borrow();
        (
            ui.nav_track_count.clone(),
            ui.nav_album_count.clone(),
            ui.nav_artist_count.clone(),
            ui.nav_playlist_count.clone(),
            ui.nav_radio_count.clone(),
            ui.all_tracks.len(),
            ui.library_albums.len(),
            ui.library_artists.len(),
            ui.playlists.len(),
            radio_stations_for_display_from(&ui.radio_stations).len(),
        )
    };

    if let Some(label) = track_label.as_ref() {
        label.set_text(&tracks.to_string());
    }
    if let Some(label) = album_label.as_ref() {
        label.set_text(&albums.to_string());
    }
    if let Some(label) = artist_label.as_ref() {
        label.set_text(&artists.to_string());
    }
    if let Some(label) = playlist_label.as_ref() {
        label.set_text(&playlists.to_string());
    }
    if let Some(label) = radio_label.as_ref() {
        label.set_text(&radio_stations.to_string());
    }
}

pub(crate) fn update_nav_selection(state: &Rc<RefCell<UiState>>) {
    let (list, active_page) = {
        let ui = state.borrow();
        (ui.nav_list.clone(), ui.active_page)
    };
    let Some(list) = list else {
        return;
    };

    let row_index = match active_page {
        LibraryPage::Tracks => Some(0),
        LibraryPage::Albums => Some(1),
        LibraryPage::Artists => Some(2),
        LibraryPage::Playlists => Some(3),
        LibraryPage::Radio => Some(4),
        LibraryPage::NextUp => None,
    };
    if row_index.is_none() {
        list.unselect_all();
        return;
    }
    let Some(row_index) = row_index else {
        return;
    };
    if list.selected_row().as_ref().map(gtk::ListBoxRow::index) == Some(row_index) {
        return;
    }
    if let Some(row) = list.row_at_index(row_index) {
        list.select_row(Some(&row));
    }
}

pub(crate) fn update_page_summary(ui: &UiState) {
    if let Some(title) = ui.collection_detail_title.as_deref() {
        if ui.active_page == LibraryPage::Artists && ui.album_filter.is_none() {
            let (album_count, song_count) = ui
                .artist_filter
                .as_deref()
                .map(|artist_key| {
                    artist_album_song_counts_from(&ui.library_albums, artist_key, &ui.search_query)
                })
                .unwrap_or((0, 0));
            ui.page_summary.set_text(&format!(
                "{title} | {}",
                artist_count_text(album_count, song_count)
            ));
            return;
        }
        ui.page_summary
            .set_text(&format!("{title} | {} matching tracks", ui.tracks.len()));
        return;
    }

    match ui.active_page {
        LibraryPage::Tracks => {
            ui.page_summary
                .set_text(&format!("Tracks | {} tracks", ui.all_tracks.len()));
        }
        LibraryPage::Albums => {
            ui.page_summary
                .set_text(&format!("Albums | {} albums", ui.library_albums.len()));
        }
        LibraryPage::Artists => {
            ui.page_summary
                .set_text(&format!("Artists | {} artists", ui.library_artists.len()));
        }
        LibraryPage::Playlists => {
            ui.page_summary
                .set_text(&format!("Playlists | {} playlists", ui.playlists.len()));
        }
        LibraryPage::Radio => {
            ui.page_summary.set_text(&format!(
                "Radio | {} Stations",
                radio_stations_for_display_from(&ui.radio_stations).len()
            ));
        }
        LibraryPage::NextUp => {
            ui.page_summary.set_text(&format!(
                "Next Up | {} queued tracks",
                upcoming_track_count(ui)
            ));
        }
    }
}
