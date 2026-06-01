# gTunes Technical Design

## Product Direction

gTunes is a Jellyfin-hosted music streaming app for Linux. It is not local-first and
is not an iTunes clone. It keeps the strongest old desktop music-library ideas:
fast source navigation, dense track selection, persistent playback context, and a
scrubbable waveform, then applies them to a modern GTK4/Libadwaita app.

Codename: `gTunes`

Developer metadata: `fivves`

Both values should stay centralized and easy to rename before release.

## V1 Boundary

V1 must:

- Sign in to a Jellyfin server.
- Browse tracks, albums, artists, and playlists.
- Display Jellyfin album artwork and artist images.
- Play Jellyfin-hosted songs with GStreamer.
- Prefer direct play and fall back to Jellyfin transcoding when needed.
- Search and select tracks quickly.
- Maintain a queue and now-playing state.
- Show a real waveform and support waveform scrubbing.
- Reserve a visible "lyrics coming soon" space.
- Respect system light mode, dark mode, and the current GTK theme as much as
  GTK4/Libadwaita allows.

Lyrics support is planned but not required for V1 playback.

Packaging is intentionally out of scope for this pass.

## Stack

- Rust for the production application.
- GTK4 and Libadwaita for the desktop shell.
- GStreamer for playback.
- SQLite for metadata, artwork, queue, settings, and waveform cache indexes.
- Reqwest for Jellyfin HTTP calls.
- Serde for Jellyfin response models.

## Architecture

The app should keep UI, network, playback, and cache responsibilities separate.

`ui`
: Builds the responsive Libadwaita shell. It should bind to application state
  instead of talking directly to Jellyfin or GStreamer.

`jellyfin`
: Owns server configuration, auth, typed API models, image URLs, stream URLs,
  pagination, and transcoding/direct-play selection.

`playback`
: Owns GStreamer state, queue control, position updates, errors, and seeking.

`cache`
: Owns SQLite migrations and persistence for Jellyfin metadata, artwork paths,
  waveform cache paths, app settings, queue state, and sync cursors.

`waveform`
: Owns waveform cache keys, background generation, and sample decoding. Jellyfin
  generally does not provide waveform data, so gTunes should generate and cache it.

`lyrics`
: Starts as a placeholder. Later it should support Jellyfin lyrics where present,
  embedded lyrics when available, and `.lrc` synced lyrics.

## Responsive Layout Guidance

The target window manager is Hyprland, so the app must handle frequent tiling and
manual resizing.

- Use adjustable panes for navigation, track selection, and context.
- Keep the transport and current track visible at practical widths.
- Allow horizontal scrolling for dense table content rather than crushing titles.
- Prefer theme colors and Libadwaita style classes over fixed palettes.
- Use compact context rails and collapsible details before hiding primary controls.

## Reliability Plan

V1 should handle:

- Jellyfin server offline or slow.
- Expired or revoked auth tokens.
- Missing artwork and missing artist images.
- Direct-play failure with transcoding fallback.
- Corrupt or unsupported streams.
- Interrupted waveform generation.
- SQLite migration failure with a clear recovery path.
- UI remaining responsive during sync, image fetches, and waveform work.

The UI thread must not block on network, disk indexing, image decoding, or waveform
generation.
