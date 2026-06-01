# Roadmap

gTunes is moving from MVP toward a reliable V1 Jellyfin music client. This file
tracks the intended development direction for contributors.

## Current MVP Baseline

- Native GTK4/Libadwaita application shell.
- Jellyfin login form.
- Saved Jellyfin session loading.
- Library sync for Jellyfin audio items.
- Track, album, and artist views.
- Search, sort, and queue controls.
- Album artwork loading.
- GStreamer-backed playback.
- MPRIS media control integration.
- SQLite cache for settings, sessions, library JSON, and waveform records.
- Waveform and lyrics UI placeholders.

## Completed V1 Reliability Work

1. Harden playback.
   - Prefer direct play where possible.
   - Fall back to Jellyfin transcoding when direct playback fails.
   - Surface clear playback errors in the UI.
   - Keep queue and now-playing state coherent after failures.

2. Finish waveform behavior.
   - Generate real waveform summaries consistently.
   - Cache summaries safely.
   - Support scrubbing without blocking the UI.
   - Recover cleanly from interrupted waveform generation.

3. Improve library reliability.
   - Handle expired or revoked Jellyfin tokens.
   - Improve offline and slow-server states.
   - Make cache migration failures actionable.
   - Add focused tests around cache and Jellyfin model handling.

## Remaining V1 Priorities

1. Add lyrics support.
   - Display Jellyfin-provided lyrics where available.
   - Support embedded unsynced lyrics when practical.
   - Add `.lrc` synced lyrics support after playback timing is stable.

2. Prepare packaging.
   - Decide initial package target.
   - Add desktop metadata and icons.
   - Document runtime plugin requirements.
   - Create a versioned release process.

## Out of Scope for V1

- Replacing Jellyfin with local-first library management.
- Rebuilding the app as an iTunes clone.
- Mobile or web clients.
- Multi-server account management beyond what is needed for a reliable first
  desktop release.

## Contribution Candidates

- Improve empty, loading, and error states in the connection panel.
- Document confirmed distribution-specific dependency names.
