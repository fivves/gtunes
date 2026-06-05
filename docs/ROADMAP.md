# Roadmap

gTunes 1.0 has shipped as a Jellyfin music client for Linux desktops. This file
tracks likely post-1.0 work and documents boundaries that should stay clear for
contributors.

## 1.0 Release Baseline

- Native GTK4/Libadwaita application shell.
- Jellyfin username/password authentication.
- Saved Jellyfin sessions.
- Cached library startup.
- Incremental Jellyfin refreshes when change timestamps are available.
- Tracks, albums, artists, and playlists.
- Search, sort, type-to-jump navigation, and collection detail views.
- Album artwork, artist images, playlist artwork, and full-size cover art.
- GStreamer playback with direct streams and transcoded fallback.
- Previous, play/pause, next, shuffle, and queue preview controls.
- Generated waveform summaries with local caching and waveform scrubbing.
- Elapsed and remaining playback time.
- MPRIS desktop media controls and metadata.
- Reconnect flow for expired or revoked Jellyfin sessions.
- Settings for library refresh, close behavior, shortcuts, cache reset, and
  quitting the app.
- AppImage build script.

Lyrics are not part of the 1.0 release.

## Near-Term Priorities

1. Improve release packaging.
   - Decide the first fully supported distribution target.
   - Add maintained desktop metadata and icon assets outside the AppImage script.
   - Document runtime GStreamer plugin expectations per distribution.
   - Define the tagged release process.

2. Broaden automated coverage.
   - Add tests for Jellyfin model mapping.
   - Add tests for cache migration and legacy cache loading.
   - Add tests for playlist merge behavior.
   - Add tests for playback-order and shuffle behavior.

3. Improve runtime diagnostics.
   - Make playback fallback events easier to inspect.
   - Improve user-facing messages for missing codecs and plugins.
   - Consider a small diagnostic export that redacts private server data.

4. Refine large-library behavior.
   - Continue reducing first-refresh latency.
   - Review memory use for large playlist sets.
   - Evaluate persistent artwork indexing instead of temporary artwork files.

## Later Candidates

- Persist queue state across app restarts.
- Add multiple saved Jellyfin servers or accounts.
- Add richer album and artist detail metadata.
- Add distribution-native packages after the release process is stable.
- Add optional advanced playback controls if they fit the desktop UI.

## Out of Scope

- Live lyrics, synced lyrics, unsynced lyrics, embedded lyrics, and Jellyfin
  lyrics display.
- Local-first library management.
- Editing Jellyfin metadata.
- Replacing Jellyfin with a separate music library backend.
- Rebuilding the app as an iTunes clone.
- Mobile and web clients.
- DRM-protected streaming services.

## Contribution Candidates

- Confirm dependency package names on additional distributions.
- Improve empty, loading, and error states.
- Improve documentation for AppImage runtime requirements.
- Add focused tests around cache, Jellyfin, waveform, and playback helpers.
