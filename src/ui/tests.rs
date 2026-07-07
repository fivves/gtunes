use super::prelude::*;

fn test_session() -> JellyfinSession {
    JellyfinSession {
        server_url: "https://jellyfin.example/".to_string(),
        server_id: Some("server-id".to_string()),
        user_id: "user-id".to_string(),
        username: "eddie".to_string(),
        access_token: "token".to_string(),
    }
}

fn test_track() -> UiTrack {
    UiTrack {
        item_id: Some("track-id".to_string()),
        date_last_saved: Some("2026-01-01T00:00:00.0000000Z".to_string()),
        album_id: Some("album-id".to_string()),
        media_source_id: Some("media-source-id".to_string()),
        stream_url: Some("https://jellyfin.example/Audio/track-id/stream".to_string()),
        fallback_stream_url: Some("https://jellyfin.example/Audio/track-id/universal".to_string()),
        stream_http_headers: Vec::new(),
        artwork_url: None,
        thumbnail_artwork_url: None,
        title: "Song".to_string(),
        artist: "Artist".to_string(),
        album_artist: Some("Artist".to_string()),
        artist_images: Vec::new(),
        album: "Album".to_string(),
        disc_number: Some(1),
        track_number: Some(2),
        album_position: None,
        duration: "3:04".to_string(),
        quality: "FLAC".to_string(),
    }
}

fn test_track_with(
    item_id: &str,
    album_id: &str,
    title: &str,
    album: &str,
    artist: &str,
    album_artist: &str,
) -> UiTrack {
    UiTrack {
        item_id: Some(item_id.to_string()),
        album_id: Some(album_id.to_string()),
        date_last_saved: Some("2026-01-01T00:00:00.0000000Z".to_string()),
        title: title.to_string(),
        album: album.to_string(),
        artist: artist.to_string(),
        album_artist: Some(album_artist.to_string()),
        ..test_track()
    }
}

fn numbered_album_track(item_id: &str, title: &str, artist: &str, track_number: i32) -> UiTrack {
    UiTrack {
        item_id: Some(item_id.to_string()),
        title: title.to_string(),
        artist: artist.to_string(),
        album: "Mixed Artist Album".to_string(),
        album_artist: Some("Main Artist".to_string()),
        disc_number: Some(1),
        track_number: Some(track_number),
        ..test_track()
    }
}

fn positioned_album_track(
    item_id: &str,
    title: &str,
    artist: &str,
    album_position: usize,
) -> UiTrack {
    UiTrack {
        item_id: Some(item_id.to_string()),
        title: title.to_string(),
        artist: artist.to_string(),
        album: "Mixed Artist Album".to_string(),
        album_artist: Some("Main Artist".to_string()),
        disc_number: None,
        track_number: None,
        album_position: Some(album_position),
        ..test_track()
    }
}

#[test]
fn invisible_search_prioritizes_leading_word_matches() {
    let soundtrack_track = UiTrack {
        title: "I Don't Give".to_string(),
        album: "American Wedding Original Soundtrack".to_string(),
        artist: "Avril Lavigne".to_string(),
        ..test_track()
    };
    let title_track = UiTrack {
        title: "American Idiot".to_string(),
        album: "American Idiot".to_string(),
        artist: "Green Day".to_string(),
        ..test_track()
    };

    assert!(
        invisible_track_search_rank(&title_track, "american")
            < invisible_track_search_rank(&soundtrack_track, "american")
    );
}

#[test]
fn invisible_search_prioritizes_word_boundaries_over_substrings() {
    assert!(
        invisible_text_search_rank("Made in America", "america")
            < invisible_text_search_rank("Panamericana", "america")
    );
}

#[test]
fn collection_summaries_group_tracks_and_count_artist_albums() {
    let tracks = vec![
        test_track_with("track-1", "album-1", "First", "Beta", "Guest", "Artist B"),
        test_track_with("track-2", "album-1", "Second", "Beta", "Guest", "Artist B"),
        test_track_with(
            "track-3", "album-2", "Third", "Alpha", "Artist A", "Artist A",
        ),
    ];

    let albums = album_summaries(&tracks, "");
    assert_eq!(albums.len(), 2);
    assert_eq!(albums[0].name, "Alpha");
    assert_eq!(albums[0].song_count, 1);
    assert_eq!(albums[1].name, "Beta");
    assert_eq!(albums[1].artist, "Artist B");
    assert_eq!(albums[1].song_count, 2);

    let artists = artist_summaries(&tracks, "");
    assert_eq!(artists.len(), 2);
    assert_eq!(artists[0].name, "Artist A");
    assert_eq!(artists[0].album_count, 1);
    assert_eq!(artists[0].song_count, 1);
    assert_eq!(artists[1].name, "Artist B");
    assert_eq!(artists[1].album_count, 1);
    assert_eq!(artists[1].song_count, 2);

    assert_eq!(
        artist_album_song_counts_from(&albums, &artist_key("Artist B"), ""),
        (1, 2)
    );
}

#[test]
fn artist_summaries_merge_separator_variants() {
    let tracks = vec![
        test_track_with(
            "track-1",
            "album-1",
            "First",
            "Alpha",
            "Blink 182",
            "Blink 182",
        ),
        test_track_with(
            "track-2",
            "album-2",
            "Second",
            "Beta",
            "blink-182",
            "blink-182",
        ),
        test_track_with(
            "track-3",
            "album-2",
            "Third",
            "Beta",
            "blink-182",
            "blink-182",
        ),
        test_track_with(
            "track-4",
            "album-3",
            "Fourth",
            "Gamma",
            "blink\u{2010}182",
            "blink\u{2010}182",
        ),
    ];

    assert_eq!(artist_key("Blink 182"), artist_key("blink-182"));
    assert_eq!(artist_key("blink-182"), artist_key("blink\u{2010}182"));

    let artists = artist_summaries(&tracks, "");

    assert_eq!(artists.len(), 1);
    assert_eq!(artists[0].name, "blink-182");
    assert_eq!(artists[0].album_count, 3);
    assert_eq!(artists[0].song_count, 4);
}

#[test]
fn album_track_sort_preserves_track_order_before_artist_order() {
    let mut tracks = vec![
        numbered_album_track("track-3", "Third", "Main Artist", 3),
        numbered_album_track("track-1", "First", "Main Artist", 1),
        numbered_album_track("track-2", "Second", "ZZ Guest", 2),
    ];
    let mut selected_index = 0;

    sort_track_slice(
        &mut tracks,
        SortColumn::Artist,
        true,
        "",
        true,
        None,
        &mut selected_index,
    );

    let track_ids = tracks
        .iter()
        .map(|track| track.item_id.as_deref())
        .collect::<Vec<_>>();
    assert_eq!(
        track_ids,
        vec![Some("track-1"), Some("track-2"), Some("track-3")]
    );
}

#[test]
fn album_track_sort_preserves_source_album_position_without_track_numbers() {
    let mut tracks = vec![
        positioned_album_track("track-3", "Third", "Main Artist", 2),
        positioned_album_track("track-1", "First", "Main Artist", 0),
        positioned_album_track("track-2", "Second", "ZZ Guest", 1),
    ];
    let mut selected_index = 0;

    sort_track_slice(
        &mut tracks,
        SortColumn::Artist,
        true,
        "",
        true,
        None,
        &mut selected_index,
    );

    let track_ids = tracks
        .iter()
        .map(|track| track.item_id.as_deref())
        .collect::<Vec<_>>();
    assert_eq!(
        track_ids,
        vec![Some("track-1"), Some("track-2"), Some("track-3")]
    );
}

#[test]
fn cached_library_without_album_order_metadata_requires_refresh() {
    let library = CachedLibrary {
        tracks: vec![
            UiTrack {
                album: "Album".to_string(),
                disc_number: None,
                track_number: None,
                album_position: None,
                ..test_track()
            },
            UiTrack {
                item_id: Some("track-2".to_string()),
                album: "Album".to_string(),
                disc_number: None,
                track_number: None,
                album_position: None,
                ..test_track()
            },
        ],
        playlists: Vec::new(),
    };

    assert!(library_needs_album_order_refresh(&library));
}

#[test]
fn refresh_prefers_currently_playing_track_when_it_still_exists() {
    let tracks = vec![
        test_track_with(
            "track-1", "album-1", "First", "Alpha", "Artist A", "Artist A",
        ),
        test_track_with(
            "track-2", "album-2", "Second", "Beta", "Artist B", "Artist B",
        ),
    ];

    let selected_key = preferred_refresh_track_key(&tracks, Some("track-2"), Some("track-1"));

    assert_eq!(selected_key.as_deref(), Some("track-2"));
}

#[test]
fn refresh_falls_back_to_previous_selection_when_playing_track_is_missing() {
    let tracks = vec![
        test_track_with(
            "track-1", "album-1", "First", "Alpha", "Artist A", "Artist A",
        ),
        test_track_with(
            "track-2", "album-2", "Second", "Beta", "Artist B", "Artist B",
        ),
    ];

    let selected_key = preferred_refresh_track_key(&tracks, Some("missing"), Some("track-1"));

    assert_eq!(selected_key.as_deref(), Some("track-1"));
}

#[test]
fn album_navigation_preserves_current_track_when_it_belongs_to_that_album() {
    let track = test_track_with(
        "track-2", "album-1", "Second", "Alpha", "Artist A", "Artist A",
    );

    let selected_key = track_key_if_same_album(&track, "album-1");

    assert_eq!(selected_key.as_deref(), Some("track-2"));
}

#[test]
fn album_navigation_does_not_force_selection_for_other_albums() {
    let track = test_track_with(
        "track-2", "album-1", "Second", "Alpha", "Artist A", "Artist A",
    );

    let selected_key = track_key_if_same_album(&track, "album-2");

    assert!(selected_key.is_none());
}

#[test]
fn queued_tracks_follow_playback_order_after_current_track() {
    let tracks = vec![
        test_track_with(
            "track-1", "album-1", "First", "Alpha", "Artist A", "Artist A",
        ),
        test_track_with(
            "track-2", "album-1", "Second", "Alpha", "Artist A", "Artist A",
        ),
        test_track_with(
            "track-3", "album-1", "Third", "Alpha", "Artist A", "Artist A",
        ),
    ];

    let queued = queued_tracks_from_order(&tracks, &[0, 1, 2], 1);

    let queued_ids = queued
        .iter()
        .map(|(_, track)| track.item_id.as_deref())
        .collect::<Vec<_>>();
    assert_eq!(queued_ids, vec![Some("track-3")]);
}

#[test]
fn persisted_playback_restore_uses_current_library_tracks() {
    let first = UiTrack {
        item_id: Some("first".to_string()),
        title: "First".to_string(),
        ..test_track()
    };
    let second = UiTrack {
        item_id: Some("second".to_string()),
        title: "Second".to_string(),
        ..test_track()
    };
    let third = UiTrack {
        item_id: Some("third".to_string()),
        title: "Third".to_string(),
        ..test_track()
    };
    let snapshot = session::PersistedPlaybackState {
        version: session::PLAYBACK_STATE_VERSION,
        current_item_id: "second".to_string(),
        ordered_item_ids: vec![
            "first".to_string(),
            "missing".to_string(),
            "second".to_string(),
            "third".to_string(),
        ],
        position_secs: 42,
        shuffle_enabled: true,
    };

    let (tracks, index, order) =
        restore_playback_snapshot_tracks(&[first, second, third], &snapshot)
            .expect("snapshot restores");

    let restored_ids = tracks
        .iter()
        .filter_map(|track| track.item_id.as_deref())
        .collect::<Vec<_>>();
    assert_eq!(restored_ids, vec!["first", "second", "third"]);
    assert_eq!(index, 1);
    assert_eq!(order, vec![0, 1, 2]);
}

#[test]
fn persisted_playback_restore_requires_current_track() {
    let snapshot = session::PersistedPlaybackState {
        version: session::PLAYBACK_STATE_VERSION,
        current_item_id: "missing".to_string(),
        ordered_item_ids: vec!["track-id".to_string()],
        position_secs: 0,
        shuffle_enabled: false,
    };

    assert!(restore_playback_snapshot_tracks(&[test_track()], &snapshot).is_none());
}

#[test]
fn legacy_library_cache_migrates_and_hydrates_stream_headers() {
    let cache = CacheDatabase::open_memory().expect("in-memory cache opens");
    let session = test_session();
    let tracks = vec![test_track()];
    let legacy_key = legacy_library_cache_key_v2(&session);
    cache
        .set_setting(
            &legacy_key,
            &serde_json::to_string(&tracks).expect("serialize"),
        )
        .expect("write legacy cache");

    let mut loaded = load_library_cache(&cache, &session)
        .expect("load library cache")
        .expect("tracks exist");
    hydrate_cached_library(&mut loaded, &session);

    assert_eq!(loaded.tracks.len(), 1);
    assert_eq!(loaded.tracks[0].title, "Song");
    assert!(loaded.playlists.is_empty());
    assert_eq!(
        loaded.tracks[0].fallback_stream_url,
        tracks[0].fallback_stream_url
    );
    assert_eq!(
        loaded.tracks[0].stream_http_headers,
        vec![("X-Emby-Token".to_string(), "token".to_string())]
    );
}

#[test]
fn cached_library_hydrates_sidebar_cover_thumbnail_size() {
    let session = test_session();
    let mut library = CachedLibrary {
        tracks: vec![UiTrack {
            thumbnail_artwork_url: Some(
                "https://jellyfin.example/Items/album-id/Images/Primary?maxWidth=160&maxHeight=160&quality=80&api_key=token"
                    .to_string(),
            ),
            ..test_track()
        }],
        playlists: Vec::new(),
    };

    hydrate_cached_library(&mut library, &session);

    let thumbnail_url = library.tracks[0]
        .thumbnail_artwork_url
        .as_deref()
        .expect("thumbnail url");
    let parsed = url::Url::parse(thumbnail_url).expect("valid thumbnail url");
    let query = parsed.query_pairs().collect::<HashMap<_, _>>();
    assert_eq!(
        query.get("maxWidth").map(std::borrow::Cow::as_ref),
        Some("220")
    );
    assert_eq!(
        query.get("maxHeight").map(std::borrow::Cow::as_ref),
        Some("220")
    );
    assert_eq!(
        query.get("quality").map(std::borrow::Cow::as_ref),
        Some("80")
    );
    assert_eq!(
        query.get("api_key").map(std::borrow::Cow::as_ref),
        Some("token")
    );
}

#[test]
fn changed_summary_ids_selects_new_and_modified_items() {
    let unchanged = UiTrack {
        item_id: Some("unchanged".to_string()),
        date_last_saved: Some("stamp-a".to_string()),
        ..test_track()
    };
    let changed = UiTrack {
        item_id: Some("changed".to_string()),
        date_last_saved: Some("stamp-b".to_string()),
        ..test_track()
    };
    let deleted = UiTrack {
        item_id: Some("deleted".to_string()),
        date_last_saved: Some("stamp-c".to_string()),
        ..test_track()
    };
    let cached = [unchanged, changed, deleted]
        .into_iter()
        .filter_map(|track| track.item_id.clone().map(|id| (id, track)))
        .collect::<HashMap<_, _>>();
    let summaries = vec![
        JellyfinItemSummary {
            id: "unchanged".to_string(),
            date_last_saved: Some("stamp-a".to_string()),
        },
        JellyfinItemSummary {
            id: "changed".to_string(),
            date_last_saved: Some("stamp-new".to_string()),
        },
        JellyfinItemSummary {
            id: "new".to_string(),
            date_last_saved: Some("stamp-new-item".to_string()),
        },
    ];

    let changed_ids = changed_summary_ids(&summaries, &cached, |track| {
        track.date_last_saved.as_deref()
    });

    assert_eq!(changed_ids, vec!["changed".to_string(), "new".to_string()]);
}

#[test]
fn summaries_missing_change_stamps_detects_unsafe_incremental_refresh() {
    assert!(summaries_missing_change_stamps(&[JellyfinItemSummary {
        id: "track-id".to_string(),
        date_last_saved: None,
    }]));
    assert!(!summaries_missing_change_stamps(&[JellyfinItemSummary {
        id: "track-id".to_string(),
        date_last_saved: Some("stamp".to_string()),
    }]));
}

#[test]
fn empty_library_cache_is_ignored() {
    let cache = CacheDatabase::open_memory().expect("in-memory cache opens");
    let session = test_session();
    cache
        .set_setting(
            &library_cache_key(&session),
            &serde_json::to_string(&CachedLibrary {
                tracks: Vec::new(),
                playlists: Vec::new(),
            })
            .expect("serialize"),
        )
        .expect("write empty cache");

    assert!(
        load_library_cache(&cache, &session)
            .expect("load library cache")
            .is_none()
    );
}

#[test]
fn library_cache_round_trips_playlists_and_hydrates_headers() {
    let cache = CacheDatabase::open_memory().expect("in-memory cache opens");
    let session = test_session();
    let track = test_track();
    let playlist = UiPlaylist {
        id: "playlist-id".to_string(),
        name: "Favorites".to_string(),
        date_last_saved: Some("2026-01-01T00:00:00.0000000Z".to_string()),
        artwork_url: None,
        thumbnail_artwork_url: None,
        tracks: vec![track.clone()],
    };
    let payload = ConnectionPayload {
        session: session.clone(),
        tracks: vec![track],
        playlists: vec![playlist],
    };

    save_library_cache(&cache, &session, &payload).expect("save library cache");
    let mut loaded = load_library_cache(&cache, &session)
        .expect("load library cache")
        .expect("library exists");
    hydrate_cached_library(&mut loaded, &session);

    assert_eq!(loaded.tracks.len(), 1);
    assert_eq!(loaded.playlists.len(), 1);
    assert_eq!(loaded.playlists[0].name, "Favorites");
    assert_eq!(loaded.playlists[0].tracks.len(), 1);
    assert_eq!(
        loaded.playlists[0].tracks[0].stream_http_headers,
        vec![("X-Emby-Token".to_string(), "token".to_string())]
    );
}

#[test]
fn radio_source_detection_recognizes_youtube_and_twitch_urls() {
    let youtube = url::Url::parse("https://www.youtube.com/watch?v=abc123").expect("url");
    let youtube_short = url::Url::parse("https://youtu.be/abc123").expect("url");
    let twitch = url::Url::parse("https://www.twitch.tv/channel").expect("url");
    let stream = url::Url::parse("https://radio.example/live.mp3").expect("url");

    assert_eq!(
        radio_source_kind_for_url(&youtube),
        RadioSourceKind::YouTube
    );
    assert_eq!(
        radio_source_kind_for_url(&youtube_short),
        RadioSourceKind::YouTube
    );
    assert_eq!(radio_source_kind_for_url(&twitch), RadioSourceKind::Twitch);
    assert_eq!(radio_source_kind_for_url(&stream), RadioSourceKind::Stream);
}

#[test]
fn legacy_radio_station_source_falls_back_to_url_detection() {
    assert_eq!(
        radio_source_kind_from_station("local", "https://www.youtube.com/live/abc123"),
        RadioSourceKind::YouTube
    );
    assert_eq!(
        radio_source_kind_from_station("local", "https://twitch.tv/channel"),
        RadioSourceKind::Twitch
    );
    assert_eq!(
        radio_source_kind_from_station("local", "https://radio.example/live.mp3"),
        RadioSourceKind::Stream
    );
}

#[test]
fn radio_station_conflicts_ignores_the_edited_station() {
    let stations = vec![RadioStation {
        id: "custom:1".to_string(),
        name: "Loft".to_string(),
        url: "https://radio.example/live.mp3".to_string(),
        source: "stream".to_string(),
        icon: None,
        built_in: false,
    }];

    assert!(!radio_station_conflicts(
        &stations,
        Some("custom:1"),
        "Loft",
        "https://radio.example/live.mp3"
    ));
    assert!(radio_station_conflicts(
        &stations,
        None,
        "Loft",
        "https://radio.example/live.mp3"
    ));
}

#[test]
fn radio_station_deserializes_without_icon_field() {
    let station: RadioStation = serde_json::from_str(
        r#"{"id":"custom:1","name":"Test","url":"https://radio.example/live.mp3","source":"stream","built_in":false}"#,
    )
    .expect("station");

    assert_eq!(station.icon, None);
    assert_eq!(
        station.icon_glyph(),
        default_radio_icon_for_kind(RadioSourceKind::Stream)
    );
}

#[test]
fn playback_requests_can_target_direct_or_transcoded_urls() {
    let track = test_track();

    let direct =
        playback_request_for_track_kind(&track, PlaybackStreamKind::Direct).expect("direct");
    let fallback =
        playback_request_for_track_kind(&track, PlaybackStreamKind::Transcode).expect("fallback");

    assert_eq!(direct.stream_kind, PlaybackStreamKind::Direct);
    assert_eq!(direct.stream_url.path(), "/Audio/track-id/stream");
    assert_eq!(fallback.stream_kind, PlaybackStreamKind::Transcode);
    assert_eq!(fallback.stream_url.path(), "/Audio/track-id/universal");
}
