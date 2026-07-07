use super::prelude::*;

#[derive(Debug)]
pub(crate) struct InvisibleSearchState {
    pub(crate) query: String,
    pub(crate) last_input_at: Instant,
}

pub(crate) const INVISIBLE_SEARCH_TIMEOUT: Duration = Duration::from_millis(1_200);

pub(crate) fn connect_app_shortcuts(root: &gtk::Box, state: Rc<RefCell<UiState>>) {
    let controller = gtk::EventControllerKey::new();
    controller.set_propagation_phase(gtk::PropagationPhase::Capture);
    let invisible_search = Rc::new(RefCell::new(InvisibleSearchState {
        query: String::new(),
        last_input_at: Instant::now(),
    }));
    controller.connect_key_pressed(move |_, key, _, modifiers| {
        if modifiers.contains(gtk::gdk::ModifierType::CONTROL_MASK) {
            match key {
                gtk::gdk::Key::_1 => set_library_page(&state, LibraryPage::Tracks),
                gtk::gdk::Key::_2 => set_library_page(&state, LibraryPage::Albums),
                gtk::gdk::Key::_3 => set_library_page(&state, LibraryPage::Artists),
                gtk::gdk::Key::_4 => set_library_page(&state, LibraryPage::Playlists),
                gtk::gdk::Key::_5 => set_library_page(&state, LibraryPage::Radio),
                gtk::gdk::Key::s | gtk::gdk::Key::S => toggle_shuffle(&state),
                gtk::gdk::Key::f | gtk::gdk::Key::F => {
                    if let Some(search) = state.borrow().search_entry.as_ref() {
                        search.grab_focus();
                    }
                }
                _ => return gtk::glib::Propagation::Proceed,
            }

            return gtk::glib::Propagation::Stop;
        }

        if modifiers.intersects(
            gtk::gdk::ModifierType::ALT_MASK
                | gtk::gdk::ModifierType::SUPER_MASK
                | gtk::gdk::ModifierType::META_MASK,
        ) {
            return gtk::glib::Propagation::Proceed;
        }

        match key {
            gtk::gdk::Key::Escape => {
                invisible_search.borrow_mut().query.clear();
                if search_entry_has_focus(&state) {
                    clear_search_entry(&state);
                    focus_active_content(&state);
                    gtk::glib::Propagation::Stop
                } else if text_input_has_focus(&state) {
                    gtk::glib::Propagation::Proceed
                } else {
                    escape_back_or_top(&state);
                    gtk::glib::Propagation::Stop
                }
            }
            gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter => {
                if text_input_has_focus(&state) {
                    invisible_search.borrow_mut().query.clear();
                    return gtk::glib::Propagation::Proceed;
                }
                if let Some(query) = active_invisible_search_query(&invisible_search) {
                    if state.borrow().is_track_list_visible() {
                        play_track_at_selected_index(&state);
                    } else {
                        activate_invisible_collection_match(&state, &query);
                    }
                    gtk::glib::Propagation::Stop
                } else if activate_focused_collection_item(&state) {
                    gtk::glib::Propagation::Stop
                } else {
                    gtk::glib::Propagation::Proceed
                }
            }
            gtk::gdk::Key::space => {
                if text_input_has_focus(&state) {
                    invisible_search.borrow_mut().query.clear();
                    return gtk::glib::Propagation::Proceed;
                }
                let query = {
                    let mut search = invisible_search.borrow_mut();
                    let now = Instant::now();
                    if now.duration_since(search.last_input_at) > INVISIBLE_SEARCH_TIMEOUT {
                        search.query.clear();
                    }
                    if search.query.is_empty() {
                        return gtk::glib::Propagation::Proceed;
                    }
                    search.query.push(' ');
                    search.last_input_at = now;
                    search.query.clone()
                };
                navigate_invisible_search(&state, &query);
                gtk::glib::Propagation::Stop
            }
            gtk::gdk::Key::BackSpace => {
                if text_input_has_focus(&state) {
                    invisible_search.borrow_mut().query.clear();
                    return gtk::glib::Propagation::Proceed;
                }
                let query = {
                    let mut search = invisible_search.borrow_mut();
                    search.query.pop();
                    search.last_input_at = Instant::now();
                    search.query.clone()
                };
                if query.is_empty() || !navigate_invisible_search(&state, &query) {
                    gtk::glib::Propagation::Proceed
                } else {
                    gtk::glib::Propagation::Stop
                }
            }
            _ => {
                if text_input_has_focus(&state) {
                    invisible_search.borrow_mut().query.clear();
                    return gtk::glib::Propagation::Proceed;
                }
                let Some(character) = key.to_unicode().filter(|character| !character.is_control())
                else {
                    return gtk::glib::Propagation::Proceed;
                };

                let query = {
                    let mut search = invisible_search.borrow_mut();
                    let now = Instant::now();
                    if now.duration_since(search.last_input_at) > INVISIBLE_SEARCH_TIMEOUT {
                        search.query.clear();
                    }
                    search.query.push(character);
                    search.last_input_at = now;
                    search.query.clone()
                };

                if navigate_invisible_search(&state, &query) {
                    gtk::glib::Propagation::Stop
                } else {
                    gtk::glib::Propagation::Proceed
                }
            }
        }
    });
    root.add_controller(controller);
}

pub(crate) fn active_invisible_search_query(
    search: &Rc<RefCell<InvisibleSearchState>>,
) -> Option<String> {
    let search = search.borrow();
    if search.query.is_empty()
        || Instant::now().duration_since(search.last_input_at) > INVISIBLE_SEARCH_TIMEOUT
    {
        return None;
    }

    Some(search.query.clone())
}

pub(crate) fn search_entry_has_focus(state: &Rc<RefCell<UiState>>) -> bool {
    state
        .borrow()
        .search_entry
        .as_ref()
        .is_some_and(widget_has_focus_within)
}

pub(crate) fn clear_search_entry(state: &Rc<RefCell<UiState>>) {
    let entry = state.borrow().search_entry.clone();
    if let Some(entry) = entry {
        entry.set_text("");
    }
}

pub(crate) fn text_input_has_focus(state: &Rc<RefCell<UiState>>) -> bool {
    let ui = state.borrow();
    ui.search_entry
        .as_ref()
        .is_some_and(widget_has_focus_within)
        || ui
            .connection_server_entry
            .as_ref()
            .is_some_and(widget_has_focus_within)
        || ui
            .connection_username_entry
            .as_ref()
            .is_some_and(widget_has_focus_within)
        || ui
            .connection_password_entry
            .as_ref()
            .is_some_and(widget_has_focus_within)
        || ui
            .radio_name_entry
            .as_ref()
            .is_some_and(widget_has_focus_within)
        || ui
            .radio_url_entry
            .as_ref()
            .is_some_and(widget_has_focus_within)
}

pub(crate) fn widget_has_focus_within(widget: &impl IsA<gtk::Widget>) -> bool {
    let widget = widget.as_ref();
    widget.has_focus()
        || widget.is_focus()
        || widget
            .focus_child()
            .is_some_and(|child| widget_has_focus_within(&child))
}

pub(crate) fn navigate_invisible_search(state: &Rc<RefCell<UiState>>, query: &str) -> bool {
    let normalized_query = query.trim().to_lowercase();
    if normalized_query.is_empty() {
        return false;
    }

    let (visible_content, target) = {
        let ui = state.borrow();
        let visible_content = visible_library_content(&ui);
        let target = match visible_content {
            VisibleLibraryContent::Tracks => ui
                .tracks
                .iter()
                .enumerate()
                .filter_map(|(index, track)| {
                    invisible_track_search_rank(track, &normalized_query).map(|rank| (index, rank))
                })
                .min_by(|(_, left), (_, right)| left.cmp(right))
                .map(|(index, _)| index),
            VisibleLibraryContent::Albums => visible_album_summaries(&ui)
                .iter()
                .enumerate()
                .filter_map(|(index, album)| {
                    invisible_search_rank(
                        [album.name.as_str(), album.artist.as_str()],
                        &normalized_query,
                    )
                    .map(|rank| (index, rank))
                })
                .min_by(|(_, left), (_, right)| left.cmp(right))
                .map(|(index, _)| index),
            VisibleLibraryContent::Artists => {
                filter_artist_summaries(&ui.library_artists, &ui.search_query)
                    .iter()
                    .enumerate()
                    .filter_map(|(index, artist)| {
                        invisible_search_rank([artist.name.as_str()], &normalized_query)
                            .map(|rank| (index, rank))
                    })
                    .min_by(|(_, left), (_, right)| left.cmp(right))
                    .map(|(index, _)| index)
            }
            VisibleLibraryContent::Playlists => filter_playlists(&ui.playlists, &ui.search_query)
                .iter()
                .enumerate()
                .filter_map(|(index, playlist)| {
                    invisible_search_rank([playlist.name.as_str()], &normalized_query)
                        .map(|rank| (index, rank))
                })
                .min_by(|(_, left), (_, right)| left.cmp(right))
                .map(|(index, _)| index),
            VisibleLibraryContent::Radio | VisibleLibraryContent::NextUp => None,
        };
        (visible_content, target)
    };

    let Some(index) = target else {
        return false;
    };

    match visible_content {
        VisibleLibraryContent::Tracks => select_track_for_navigation(state, index),
        VisibleLibraryContent::Albums
        | VisibleLibraryContent::Artists
        | VisibleLibraryContent::Playlists
        | VisibleLibraryContent::Radio
        | VisibleLibraryContent::NextUp => focus_collection_item(state, index),
    }

    true
}

pub(crate) fn activate_invisible_collection_match(
    state: &Rc<RefCell<UiState>>,
    query: &str,
) -> bool {
    let normalized_query = query.trim().to_lowercase();
    if normalized_query.is_empty() {
        return false;
    }

    enum CollectionMatch {
        Album(AlbumSummary),
        Artist(ArtistSummary),
    }

    let target = {
        let ui = state.borrow();
        match visible_library_content(&ui) {
            VisibleLibraryContent::Albums => visible_album_summaries(&ui)
                .into_iter()
                .filter_map(|album| {
                    invisible_search_rank(
                        [album.name.as_str(), album.artist.as_str()],
                        &normalized_query,
                    )
                    .map(|rank| (album, rank))
                })
                .min_by(|(_, left), (_, right)| left.cmp(right))
                .map(|(album, _)| CollectionMatch::Album(album)),
            VisibleLibraryContent::Artists => {
                filter_artist_summaries(&ui.library_artists, &ui.search_query)
                    .into_iter()
                    .filter_map(|artist| {
                        invisible_search_rank([artist.name.as_str()], &normalized_query)
                            .map(|rank| (artist, rank))
                    })
                    .min_by(|(_, left), (_, right)| left.cmp(right))
                    .map(|(artist, _)| CollectionMatch::Artist(artist))
            }
            VisibleLibraryContent::Tracks
            | VisibleLibraryContent::Playlists
            | VisibleLibraryContent::NextUp => None,
            VisibleLibraryContent::Radio => None,
        }
    };

    match target {
        Some(CollectionMatch::Album(album)) => {
            show_album_tracks(state, &album);
            true
        }
        Some(CollectionMatch::Artist(artist)) => {
            show_artist_albums(state, &artist);
            true
        }
        None => false,
    }
}

pub(crate) fn invisible_track_search_rank(track: &UiTrack, query: &str) -> Option<(u8, u8, usize)> {
    [
        (track.title.as_str(), 0),
        (track.artist.as_str(), 1),
        (track.album.as_str(), 2),
    ]
    .into_iter()
    .filter_map(|(text, field_rank)| {
        invisible_text_search_rank(text, query)
            .map(|(match_rank, match_index)| (match_rank, field_rank, match_index))
    })
    .min()
}

pub(crate) fn invisible_search_rank<'a>(
    texts: impl IntoIterator<Item = &'a str>,
    query: &str,
) -> Option<(u8, u8, usize)> {
    texts
        .into_iter()
        .enumerate()
        .filter_map(|(field_rank, text)| {
            invisible_text_search_rank(text, query)
                .map(|(match_rank, match_index)| (match_rank, field_rank as u8, match_index))
        })
        .min()
}

pub(crate) fn invisible_text_search_rank(text: &str, query: &str) -> Option<(u8, usize)> {
    let normalized_text = text.to_lowercase();
    let full_match_index = normalized_text.find(query)?;
    let word_match_index = normalized_text
        .match_indices(query)
        .find_map(|(index, _)| is_word_boundary(&normalized_text, index).then_some(index));

    let rank = if normalized_text.starts_with(query) {
        0
    } else if let Some(index) = word_match_index {
        return Some((1, index));
    } else {
        2
    };

    Some((rank, full_match_index))
}

pub(crate) fn is_word_boundary(text: &str, index: usize) -> bool {
    if index == 0 {
        return true;
    }

    text[..index]
        .chars()
        .next_back()
        .is_none_or(|character| !character.is_alphanumeric())
}

pub(crate) fn select_track_for_navigation(state: &Rc<RefCell<UiState>>, index: usize) {
    let selected_index = {
        let mut ui = state.borrow_mut();
        if ui.tracks.is_empty() {
            return;
        }
        ui.selected_index = index.min(ui.tracks.len() - 1);
        let selected_index = ui.selected_index;
        if ui.playback_session.queue_tracks.is_empty() {
            rebuild_playback_order(&mut ui, selected_index);
        }
        update_now_playing_labels(&ui);
        update_play_button(&ui);
        selected_index
    };

    select_track_model_row(state, selected_index);
    scroll_track_list_to_index(state, selected_index);
    rebuild_queue_list(state);
    load_selected_cover_art(state);
    load_selected_waveform(state);
}

pub(crate) fn escape_back_or_top(state: &Rc<RefCell<UiState>>) {
    if has_collection_detail_open(state) {
        return_to_collection_grid(state);
    } else {
        focus_active_content_top(state);
    }
}

pub(crate) fn focus_active_content(state: &Rc<RefCell<UiState>>) {
    if state.borrow().is_track_list_visible() {
        focus_track_list(state);
    } else {
        focus_active_collection_grid(state);
    }
}

pub(crate) fn focus_active_content_top(state: &Rc<RefCell<UiState>>) {
    if state.borrow().is_track_list_visible() {
        let has_tracks = !state.borrow().tracks.is_empty();
        if has_tracks {
            select_track_for_navigation(state, 0);
        } else {
            scroll_track_list_to_top(state);
            focus_track_list(state);
        }
    } else {
        scroll_active_collection_grid_to_top(state);
        focus_active_collection_grid(state);
    }
}
