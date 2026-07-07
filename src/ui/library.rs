use super::prelude::*;

pub(crate) fn visible_album_summaries(ui: &UiState) -> Vec<AlbumSummary> {
    if ui.active_page == LibraryPage::Artists {
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
    }
}

#[derive(Clone, Debug)]
pub(crate) struct AlbumSummary {
    pub(crate) key: String,
    pub(crate) name: String,
    pub(crate) artist: String,
    pub(crate) artist_image_url: Option<String>,
    pub(crate) artwork_url: Option<String>,
    pub(crate) song_count: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct ArtistSummary {
    pub(crate) key: String,
    pub(crate) name: String,
    pub(crate) image_url: Option<String>,
    pub(crate) album_count: usize,
    pub(crate) song_count: usize,
}

pub(crate) struct AlbumAccumulator {
    pub(crate) key: String,
    pub(crate) name: String,
    pub(crate) artwork_url: Option<String>,
    pub(crate) song_count: usize,
    pub(crate) explicit_artist_votes: Vec<ArtistVote>,
    pub(crate) fallback_artist_votes: Vec<ArtistVote>,
}

pub(crate) struct ArtistVote {
    pub(crate) key: String,
    pub(crate) name: String,
    pub(crate) image_url: Option<String>,
    pub(crate) count: usize,
    pub(crate) first_seen: usize,
}

pub(crate) struct ArtistNameVote {
    pub(crate) key: String,
    pub(crate) name: String,
    pub(crate) count: usize,
    pub(crate) first_seen: usize,
}

pub(crate) fn album_summaries(tracks: &[UiTrack], query: &str) -> Vec<AlbumSummary> {
    let mut albums = Vec::<AlbumAccumulator>::new();
    let mut album_indexes = HashMap::<String, usize>::new();
    for (track_index, track) in tracks.iter().enumerate() {
        let key = album_key(track);
        if let Some(album_index) = album_indexes.get(&key).copied() {
            let album = &mut albums[album_index];
            album.song_count += 1;
            if album.artwork_url.is_none() {
                album.artwork_url = track.thumbnail_artwork_url.clone();
            }
            add_artist_vote(
                &mut album.explicit_artist_votes,
                track.album_artist.as_deref(),
                track
                    .album_artist
                    .as_deref()
                    .and_then(|artist| track.artist_thumbnail_url_for(artist)),
                track_index,
            );
            add_artist_vote(
                &mut album.fallback_artist_votes,
                Some(&track.artist),
                track.artist_thumbnail_url_for(&track.artist),
                track_index,
            );
            continue;
        }

        album_indexes.insert(key.clone(), albums.len());
        let mut album = AlbumAccumulator {
            key,
            name: track.album.clone(),
            artwork_url: track.thumbnail_artwork_url.clone(),
            song_count: 1,
            explicit_artist_votes: Vec::new(),
            fallback_artist_votes: Vec::new(),
        };
        add_artist_vote(
            &mut album.explicit_artist_votes,
            track.album_artist.as_deref(),
            track
                .album_artist
                .as_deref()
                .and_then(|artist| track.artist_thumbnail_url_for(artist)),
            track_index,
        );
        add_artist_vote(
            &mut album.fallback_artist_votes,
            Some(&track.artist),
            track.artist_thumbnail_url_for(&track.artist),
            track_index,
        );
        albums.push(album);
    }

    let mut albums = albums
        .into_iter()
        .map(|album| {
            let preferred_artist =
                preferred_album_artist(&album.explicit_artist_votes, &album.fallback_artist_votes);
            AlbumSummary {
                key: album.key,
                name: album.name,
                artist: preferred_artist.name,
                artist_image_url: preferred_artist.image_url,
                artwork_url: album.artwork_url,
                song_count: album.song_count,
            }
        })
        .collect::<Vec<_>>();

    let query = query.trim().to_lowercase();
    if !query.is_empty() {
        albums.retain(|album| {
            album.name.to_lowercase().contains(&query)
                || album.artist.to_lowercase().contains(&query)
        });
    }

    albums.sort_by(|left, right| {
        compare_text(&left.name, &right.name)
            .then_with(|| compare_text(&left.artist, &right.artist))
    });
    albums
}

pub(crate) fn album_summaries_for_artist_from(
    albums: &[AlbumSummary],
    selected_artist_key: &str,
    query: &str,
) -> Vec<AlbumSummary> {
    filter_album_summaries(albums, query)
        .into_iter()
        .filter(|album| artist_key(&album.artist) == selected_artist_key)
        .collect()
}

pub(crate) fn filter_album_summaries(albums: &[AlbumSummary], query: &str) -> Vec<AlbumSummary> {
    let query = query.trim().to_lowercase();
    let mut albums = albums
        .iter()
        .filter(|album| {
            query.is_empty()
                || album.name.to_lowercase().contains(&query)
                || album.artist.to_lowercase().contains(&query)
        })
        .cloned()
        .collect::<Vec<_>>();
    albums.sort_by(|left, right| {
        compare_text(&left.name, &right.name)
            .then_with(|| compare_text(&left.artist, &right.artist))
    });
    albums
}

pub(crate) fn artist_album_song_counts_from(
    albums: &[AlbumSummary],
    selected_artist_key: &str,
    query: &str,
) -> (usize, usize) {
    let albums = album_summaries_for_artist_from(albums, selected_artist_key, query);
    album_song_counts(&albums)
}

pub(crate) fn album_song_counts(albums: &[AlbumSummary]) -> (usize, usize) {
    let song_count = albums.iter().map(|album| album.song_count).sum();
    (albums.len(), song_count)
}

pub(crate) fn artist_summaries(tracks: &[UiTrack], query: &str) -> Vec<ArtistSummary> {
    struct ArtistAccumulator {
        key: String,
        image_url: Option<String>,
        album_count: usize,
        song_count: usize,
        name_votes: Vec<ArtistNameVote>,
    }

    let mut artists = Vec::<ArtistAccumulator>::new();
    let mut artist_indexes = HashMap::<String, usize>::new();
    for (album_index, album) in album_summaries(tracks, "").into_iter().enumerate() {
        let key = artist_key(&album.artist);
        let image_url = artist_summary_image_url(&album);
        if let Some(artist_index) = artist_indexes.get(&key).copied() {
            let artist = &mut artists[artist_index];
            artist.album_count += 1;
            artist.song_count += album.song_count;
            if artist.image_url.is_none() {
                artist.image_url = image_url;
            }
            add_artist_name_vote(
                &mut artist.name_votes,
                &album.artist,
                album.song_count,
                album_index,
            );
            continue;
        }

        artist_indexes.insert(key.clone(), artists.len());
        let mut name_votes = Vec::new();
        add_artist_name_vote(
            &mut name_votes,
            &album.artist,
            album.song_count,
            album_index,
        );
        artists.push(ArtistAccumulator {
            key,
            image_url,
            album_count: 1,
            song_count: album.song_count,
            name_votes,
        });
    }

    let mut artists = artists
        .into_iter()
        .map(|artist| ArtistSummary {
            key: artist.key,
            name: preferred_artist_name(&artist.name_votes)
                .unwrap_or("Unknown Artist")
                .to_string(),
            image_url: artist.image_url,
            album_count: artist.album_count,
            song_count: artist.song_count,
        })
        .collect::<Vec<_>>();

    let query = query.trim().to_lowercase();
    if !query.is_empty() {
        artists.retain(|artist| artist.name.to_lowercase().contains(&query));
    }

    artists.sort_by(|left, right| compare_text(&left.name, &right.name));
    artists
}

pub(crate) fn filter_artist_summaries(
    artists: &[ArtistSummary],
    query: &str,
) -> Vec<ArtistSummary> {
    let query = query.trim().to_lowercase();
    let mut artists = artists
        .iter()
        .filter(|artist| query.is_empty() || artist.name.to_lowercase().contains(&query))
        .cloned()
        .collect::<Vec<_>>();
    artists.sort_by(|left, right| compare_text(&left.name, &right.name));
    artists
}

pub(crate) fn rebuild_library_summaries(ui: &mut UiState) {
    ui.library_albums = album_summaries(&ui.all_tracks, "");
    ui.library_artists = artist_summaries(&ui.all_tracks, "");
}

pub(crate) fn assign_album_positions(tracks: &mut [UiTrack]) {
    let mut album_counts = HashMap::<String, usize>::new();
    for track in tracks {
        let position = album_counts.entry(album_key(track)).or_default();
        track.album_position = Some(*position);
        *position += 1;
    }
}

pub(crate) fn filter_playlists(playlists: &[UiPlaylist], query: &str) -> Vec<UiPlaylist> {
    let query = query.trim().to_lowercase();
    let mut playlists = playlists
        .iter()
        .filter(|playlist| query.is_empty() || playlist.name.to_lowercase().contains(&query))
        .cloned()
        .collect::<Vec<_>>();
    playlists.sort_by(|left, right| compare_text(&left.name, &right.name));
    playlists
}

pub(crate) fn artist_summary_image_url(album: &AlbumSummary) -> Option<String> {
    album.artist_image_url.clone()
}

pub(crate) fn artist_count_text(album_count: usize, song_count: usize) -> String {
    format!(
        "{} | {}",
        count_text(album_count, "album", "albums"),
        count_text(song_count, "song", "songs")
    )
}

pub(crate) fn count_text(count: usize, singular: &str, plural: &str) -> String {
    let noun = if count == 1 { singular } else { plural };
    format!("{count} {noun}")
}

pub(crate) fn add_artist_vote(
    votes: &mut Vec<ArtistVote>,
    artist: Option<&str>,
    image_url: Option<String>,
    first_seen: usize,
) {
    let Some(artist) = artist.map(str::trim).filter(|artist| !artist.is_empty()) else {
        return;
    };
    let key = artist_key(artist);
    if let Some(vote) = votes.iter_mut().find(|vote| vote.key == key) {
        vote.count += 1;
        if vote.image_url.is_none() {
            vote.image_url = image_url;
        }
        return;
    }

    votes.push(ArtistVote {
        key,
        name: artist.to_string(),
        image_url,
        count: 1,
        first_seen,
    });
}

pub(crate) fn add_artist_name_vote(
    votes: &mut Vec<ArtistNameVote>,
    artist: &str,
    count: usize,
    first_seen: usize,
) {
    let artist = artist.trim();
    if artist.is_empty() {
        return;
    }

    let key = normalized_artist_display_key(artist);
    if let Some(vote) = votes.iter_mut().find(|vote| vote.key == key) {
        vote.count += count;
        return;
    }

    votes.push(ArtistNameVote {
        key,
        name: artist.to_string(),
        count,
        first_seen,
    });
}

pub(crate) fn preferred_artist_name(votes: &[ArtistNameVote]) -> Option<&str> {
    votes
        .iter()
        .max_by(|left, right| {
            left.count
                .cmp(&right.count)
                .then_with(|| right.first_seen.cmp(&left.first_seen))
        })
        .map(|vote| vote.name.as_str())
}

pub(crate) struct PreferredArtist {
    pub(crate) name: String,
    pub(crate) image_url: Option<String>,
}

pub(crate) fn preferred_album_artist(
    explicit_votes: &[ArtistVote],
    fallback_votes: &[ArtistVote],
) -> PreferredArtist {
    preferred_artist_vote(explicit_votes)
        .or_else(|| preferred_artist_vote(fallback_votes))
        .map(|vote| PreferredArtist {
            name: vote.name.clone(),
            image_url: vote.image_url.clone(),
        })
        .unwrap_or_else(|| PreferredArtist {
            name: "Unknown Artist".to_string(),
            image_url: None,
        })
}

pub(crate) fn preferred_artist_vote(votes: &[ArtistVote]) -> Option<&ArtistVote> {
    votes.iter().max_by(|left, right| {
        left.count
            .cmp(&right.count)
            .then_with(|| right.first_seen.cmp(&left.first_seen))
    })
}

pub(crate) fn sort_track_slice(
    tracks: &mut [UiTrack],
    column: SortColumn,
    ascending: bool,
    search_query: &str,
    album_order_first: bool,
    selected_key: Option<&str>,
    selected_index: &mut usize,
) {
    let normalized_query = search_query.trim().to_lowercase();
    tracks.sort_by(|left, right| {
        let exact_match_ordering = if normalized_query.is_empty() {
            Ordering::Equal
        } else {
            exact_title_match_rank(left, &normalized_query)
                .cmp(&exact_title_match_rank(right, &normalized_query))
        };

        let column_ordering = match column {
            SortColumn::Title => compare_text(&left.title, &right.title),
            SortColumn::Artist => compare_artist_album_track(left, right),
            SortColumn::Album => compare_text(&left.album, &right.album),
            SortColumn::Duration => {
                duration_seconds(&left.duration).cmp(&duration_seconds(&right.duration))
            }
        };

        let ordering = if album_order_first {
            compare_album_track_order(left, right).then(exact_match_ordering)
        } else {
            exact_match_ordering.then(column_ordering)
        }
        .then_with(|| compare_text(&left.title, &right.title))
        .then_with(|| compare_text(&left.artist, &right.artist))
        .then_with(|| compare_text(&left.album, &right.album));

        if ascending || album_order_first {
            ordering
        } else {
            ordering.reverse()
        }
    });

    if let Some(selected_key) = selected_key {
        *selected_index = tracks
            .iter()
            .position(|track| track_has_key(track, selected_key))
            .unwrap_or(0);
    } else {
        *selected_index = 0;
    }
}

pub(crate) fn apply_track_filter(ui: &mut UiState, selected_key: Option<&str>) {
    let query = ui.search_query.to_lowercase();
    let artist_album_keys = ui.artist_filter.as_deref().map(|selected_artist_key| {
        ui.library_albums
            .iter()
            .filter(|album| artist_key(&album.artist) == selected_artist_key)
            .map(|album| album.key.clone())
            .collect::<HashSet<_>>()
    });
    let source_tracks = ui
        .playlist_filter
        .as_deref()
        .and_then(|playlist_id| {
            ui.playlists
                .iter()
                .find(|playlist| playlist.id == playlist_id)
                .map(|playlist| playlist.tracks.as_slice())
        })
        .unwrap_or(ui.all_tracks.as_slice());
    ui.tracks = source_tracks
        .iter()
        .filter(|track| {
            let album_matches = ui
                .album_filter
                .as_deref()
                .map(|key| album_key(track) == key)
                .unwrap_or(true);
            let artist_matches = ui
                .artist_filter
                .as_deref()
                .map(|_| {
                    artist_album_keys
                        .as_ref()
                        .is_some_and(|album_keys| album_keys.contains(&album_key(track)))
                })
                .unwrap_or(true);
            let search_matches = query.is_empty() || track_matches_query(track, &query);
            album_matches && artist_matches && search_matches
        })
        .cloned()
        .collect();
    sort_track_slice(
        &mut ui.tracks,
        ui.sort_column,
        ui.sort_ascending,
        &ui.search_query,
        ui.album_filter.is_some(),
        selected_key,
        &mut ui.selected_index,
    );
    ui.track_filter_signature = ui.current_track_filter_signature();
    if ui.playback_session.queue_tracks.is_empty() {
        let selected_index = ui.selected_index;
        rebuild_playback_order(ui, selected_index);
    }
}

pub(crate) fn exact_title_match_rank(track: &UiTrack, query: &str) -> u8 {
    if track.title.trim().eq_ignore_ascii_case(query) {
        0
    } else {
        1
    }
}

pub(crate) fn preferred_refresh_track_key(
    tracks: &[UiTrack],
    now_playing_key: Option<&str>,
    selected_key: Option<&str>,
) -> Option<String> {
    now_playing_key
        .filter(|key| tracks.iter().any(|track| track_has_key(track, key)))
        .map(|key| key.to_string())
        .or_else(|| {
            selected_key
                .filter(|key| tracks.iter().any(|track| track_has_key(track, key)))
                .map(|key| key.to_string())
        })
}

pub(crate) fn current_display_track(state: &UiState) -> Option<&UiTrack> {
    if state.playback_session.mode.is_radio() {
        return None;
    }

    state
        .playback_session
        .now_playing_key
        .as_deref()
        .and_then(|key| find_track_by_key(state, key))
        .or_else(|| state.tracks.get(state.selected_index))
}

pub(crate) fn find_track_by_key<'a>(state: &'a UiState, key: &str) -> Option<&'a UiTrack> {
    state
        .all_tracks
        .iter()
        .find(|track| track_has_key(track, key))
        .or_else(|| state.tracks.iter().find(|track| track_has_key(track, key)))
}

pub(crate) fn compare_text(left: &str, right: &str) -> Ordering {
    left.chars()
        .flat_map(char::to_lowercase)
        .cmp(right.chars().flat_map(char::to_lowercase))
}

pub(crate) fn compare_artist_album_track(left: &UiTrack, right: &UiTrack) -> Ordering {
    compare_text(&left.artist, &right.artist)
        .then_with(|| compare_text(&left.album, &right.album))
        .then_with(|| compare_optional_i32(left.disc_number, right.disc_number))
        .then_with(|| compare_optional_i32(left.track_number, right.track_number))
        .then_with(|| compare_text(&left.title, &right.title))
}

pub(crate) fn compare_album_track_order(left: &UiTrack, right: &UiTrack) -> Ordering {
    compare_optional_usize(left.album_position, right.album_position)
        .then_with(|| compare_optional_i32(left.disc_number, right.disc_number))
        .then_with(|| compare_optional_i32(left.track_number, right.track_number))
        .then_with(|| compare_text(&left.title, &right.title))
        .then_with(|| compare_text(&left.artist, &right.artist))
}

pub(crate) fn compare_optional_usize(left: Option<usize>, right: Option<usize>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.cmp(&right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

pub(crate) fn compare_optional_i32(left: Option<i32>, right: Option<i32>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.cmp(&right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}
