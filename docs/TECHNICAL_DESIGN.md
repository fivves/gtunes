# gTunes Technical Design

This document describes the gTunes 1.0 architecture and release behavior.

## Product Scope

gTunes is a Jellyfin music streaming client for Linux desktops. It is designed
for users who already manage their music in Jellyfin and want a native desktop
library browser with strong playback controls.

gTunes is not a local-first music manager, a web client, a mobile client, or an
iTunes clone. The 1.0 release concentrates on Jellyfin login, library browsing,
playback, queue context, artwork, waveform scrubbing, local caching, and desktop
media integration.

gTunes 1.0 does not support lyrics.

## Stack

- Rust for application logic.
- GTK4 and Libadwaita for the desktop shell.
- GStreamer `playbin` for audio playback and seeking.
- SQLite through `rusqlite` for settings, sessions, library cache records, and
  waveform indexes.
- Reqwest for Jellyfin HTTP calls and image fetches.
- Serde and Serde JSON for Jellyfin responses and cached payloads.
- Souvlaki for MPRIS desktop media controls.
- `directories` for platform data and cache locations.
- `tracing` for structured diagnostics.

## Module Boundaries

`app`
: Owns application startup, GTK activation, CSS provider registration, and the
  main window lifecycle.

`config`
: Centralizes app identifiers and package metadata used by the UI, Jellyfin user
  agent, MPRIS, and desktop integration.

`ui`
: Builds the Libadwaita interface, owns UI state, connects widgets to
  application behavior, handles background worker polling, and coordinates
  library, playback, artwork, waveform, and MPRIS updates.

`jellyfin`
: Owns authentication, typed Jellyfin models, item and image URLs, direct and
  transcoded stream URLs, playlist APIs, pagination, and Jellyfin HTTP headers.

`playback`
: Wraps GStreamer `playbin`, applies Jellyfin stream headers, tracks playback
  state, handles seeking, polls bus events, and arms the next stream for
  gapless-style handoff.

`cache`
: Owns SQLite opening, schema migration, app settings, saved Jellyfin sessions,
  waveform cache indexes, and cache reset behavior.

`waveform`
: Generates waveform peaks from Jellyfin stream URIs with GStreamer, writes
  versioned JSON summaries, and reloads cached summaries.

## Startup and Session Flow

On launch, gTunes opens the default SQLite database and attempts to load a saved
Jellyfin session. If a session and cached library are available, the connection
panel is hidden and cached tracks/playlists are shown immediately.

If cached data is missing or stale, gTunes creates an authenticated Jellyfin
client with the saved token and refreshes the library in a background thread. UI
polling uses GTK timeouts so network and disk work do not block the main thread.

When the user signs in manually, the app authenticates with
`Users/AuthenticateByName`, saves the resulting session, loads tracks and
playlists, then writes the library cache.

If Jellyfin returns unauthorized or forbidden responses for a saved session,
gTunes surfaces a reconnect path that asks only for the account password and
then refreshes the library.

## Library Model

The primary source of truth is the Jellyfin audio item list. Each track is
mapped into UI data that includes:

- Jellyfin item ID and media source ID.
- Track title, artist, album artist, album, disc number, track number, and
  duration.
- Container-derived quality label.
- Album and thumbnail artwork URLs.
- Artist image URLs when Jellyfin exposes them.
- Direct stream URL, transcoded fallback URL, and stream HTTP headers.
- `DateLastSaved` metadata used for incremental refreshes.

Albums and artists are derived locally from the synced track set. Playlists are
loaded from Jellyfin playlist APIs and cached with their track membership.

Refresh behavior prefers incremental updates. gTunes asks Jellyfin for item
summaries and compares `DateLastSaved` values against cached tracks and
playlists. If timestamps are unavailable, or if legacy cached data lacks fields
needed by 1.0, the app falls back to a full refresh.

## UI Behavior

The main shell has three persistent areas:

- Player bar: transport controls, now-playing labels, waveform, elapsed and
  remaining time, search, shuffle, and settings.
- Sidebar: library navigation, queue preview, and current cover art.
- Content view: tracks, album grid, artist grid, playlist grid, or detail views.

Tracks are displayed in a GTK column view with sortable title, artist, album,
and time columns. Album, artist, and playlist views use artwork grids with
batched rendering so large libraries remain responsive.

The search field filters the current library surface. Type-to-jump navigation
also works when normal text inputs are not focused, allowing quick keyboard
selection without moving to the search field.

The now-playing artist and album labels are links into the matching collection
views. Clicking the current cover art opens a larger undecorated artwork window.

## Playback Behavior

Playback starts with the Jellyfin direct stream URL. The playback engine applies
the `X-Emby-Token` header to HTTP sources and tracks whether the current stream
is direct or transcoded.

When a direct stream fails, UI playback handling can retry with the Jellyfin
universal/transcoded stream URL for the same item. Playback status text reports
when a transcoded stream is being used.

The selected track and playback order drive previous/next controls. Shuffle
rebuilds the playback order from the selected track and updates the queue
preview. The queue preview displays up to 15 upcoming tracks and allows jumping
to any visible upcoming track.

GStreamer position and duration are polled on a 250 ms interval. The same loop
updates labels, MPRIS state, waveform progress, end-of-stream behavior, and
playback errors.

If "Keep playing while closed" is enabled, closing the window hides it and
allows playback to continue. Otherwise, closing the window stops playback.

## Waveforms

Waveforms are generated from Jellyfin stream URIs with a separate GStreamer
decode pipeline. gTunes converts audio to mono floating-point samples, summarizes
the stream into fixed-size peak data, and stores a versioned JSON file in the
platform cache directory.

Waveform cache locations are indexed in SQLite by item ID and media source ID.
If a cache file is missing, stale, or invalid, gTunes regenerates it. The
waveform drawing area displays loading, failure, and ready states. Once duration
is known, clicking or dragging across the waveform seeks playback.

## Persistence

SQLite schema version 1 creates:

- `app_settings` for saved session JSON, library cache JSON, sort settings, and
  close behavior.
- `jellyfin_servers` for future server metadata.
- `media_items` for future structured media cache records.
- `artwork_cache` for future persistent artwork indexes.
- `waveform_cache` for generated waveform summary locations.
- `queue_state` for future persisted queue state.

Current artwork files are cached in the system temporary directory with
`gtunes-artwork-*` names. Waveform files are cached under the platform cache
directory.

The settings menu can reset the database, data directory, cache directory, and
temporary artwork files. Reset returns the app to first-time setup.

## Reliability Notes

The 1.0 app handles these cases explicitly:

- Invalid Jellyfin server URLs.
- Offline, unreachable, or slow Jellyfin servers.
- Expired or revoked Jellyfin sessions.
- Missing artwork and missing artist images.
- Empty track, album, artist, and playlist search results.
- Legacy library cache formats.
- Database versions newer than the app supports.
- Missing GStreamer `playbin`.
- Playback errors, direct stream failures, and transcoded fallback.
- Interrupted or stale waveform cache files.

Network requests, library refreshes, cache reset, artwork fetches, and waveform
generation run away from the GTK UI thread. UI state is updated from GTK timeout
polling on the main thread.
