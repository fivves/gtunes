# Development Guide

This guide is for contributors working on gTunes 1.0 and later.

## Project Shape

gTunes is a native Linux desktop app written in Rust. The UI is GTK4 and
Libadwaita, playback runs through GStreamer, Jellyfin communication uses Reqwest,
and local state is stored in SQLite.

Main module boundaries:

- `src/app.rs`: application startup, CSS setup, and GTK activation.
- `src/config.rs`: app ID, app name, developer name, and package version.
- `src/ui/`: Libadwaita window, widgets, state wiring, CSS, background worker
  polling, library views, queue, artwork, waveform UI, and settings.
- `src/jellyfin/`: Jellyfin client methods, stream/image URL construction, HTTP
  headers, pagination, playlists, and typed API models.
- `src/playback/`: GStreamer playback state, source header setup, seeking,
  stream handoff, bus polling, and stop behavior.
- `src/cache/`: SQLite migrations, app settings, saved sessions, waveform cache
  indexes, and cache reset helpers.
- `src/waveform/`: waveform cache keys, summary generation, JSON cache files,
  and GStreamer decode pipeline.

Architecture details live in [TECHNICAL_DESIGN.md](TECHNICAL_DESIGN.md).

## Prerequisites

Install Rust and the native libraries needed by GTK, Libadwaita, GStreamer,
SQLite, and DBus.

Install `yt-dlp` and `streamlink` when testing YouTube or Twitch radio stations.

Arch Linux:

```sh
sudo pacman -S rust gtk4 libadwaita gstreamer gst-plugins-base sqlite dbus pkgconf
```

Ubuntu or Debian:

```sh
sudo apt install build-essential pkg-config libgtk-4-dev libadwaita-1-dev \
  libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev libsqlite3-dev \
  libdbus-1-dev
```

Fedora:

```sh
sudo dnf install rust cargo gtk4-devel libadwaita-devel gstreamer1-devel \
  gstreamer1-plugins-base-devel sqlite-devel dbus-devel pkgconf-pkg-config
```

## First Run

Clone, check, and run:

```sh
git clone git@github.com:fivves/gtunes.git
cd gtunes
cargo check
cargo run
```

Use a Jellyfin URL with a scheme:

```text
http://localhost:8096
```

or:

```text
https://jellyfin.example.com
```

The app saves the Jellyfin access token and cached library metadata locally
after a successful login.

## Verification

Run the standard checks before opening a pull request:

```sh
cargo fmt -- --check
cargo check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

Manual verification is still important for changes that affect Jellyfin login,
library refresh, playlists, playback, stream fallback, artwork, waveforms,
MPRIS, cache reset, or reconnect behavior.

## Local Data

Local development data is stored through the `directories` crate. On Linux, the
main locations are typically:

```text
~/.local/share/dev/fivves/gTunes/
~/.cache/dev/fivves/gTunes/
```

Temporary artwork files are written to the system temporary directory with names
starting with `gtunes-artwork-`.

To reset local state from the UI, open Settings and choose "Reset database and
cache." For a manual reset during development, remove the data and cache
directories and any matching temporary artwork files.

Never commit local databases, credentials, access tokens, stream URLs, logs,
screenshots with private server details, or `.env` files.

## Development Workflow

1. Branch from the active development branch.
2. Keep changes focused around one behavior or maintenance task.
3. Preserve module ownership boundaries.
4. Add focused tests for isolated logic when practical.
5. Run the verification commands.
6. Include manual Jellyfin notes when the change affects runtime behavior.

## UI Guidelines

- Keep the GTK UI thread responsive.
- Do network, disk, image decoding, cache reset, and waveform work outside the
  UI thread.
- Use GTK4 and Libadwaita widgets, style classes, and theme colors unless a
  custom style is already established locally.
- Keep transport controls, now-playing state, and the bottom connection status
  useful at practical tiled window sizes.
- Keep track-table behavior dense and predictable.
- Use batched rendering for collection grids that may contain many items.
- Keep labels ellipsized rather than allowing layout-breaking text overflow.

## Keyboard Shortcuts

The main window keeps a small set of global shortcuts for navigation and
playback:

- `Ctrl+F`: focus library search.
- `Ctrl+1`: open Tracks.
- `Ctrl+2`: open Albums.
- `Ctrl+3`: open Artists.
- `Ctrl+4`: open Playlists.
- `Ctrl+5`: open Radio.
- `Ctrl+S`: toggle shuffle.
- `Return`: play the selected search result.

## Jellyfin Notes

- Keep Jellyfin response parsing typed with `serde` models in `src/jellyfin`.
- Keep URL construction centralized in `JellyfinClient`.
- Include the scheme in manual test server URLs.
- Avoid logging passwords, tokens, direct stream URLs, transcoded stream URLs,
  and private server addresses.
- When changing refresh behavior, test saved-session startup, manual refresh,
  and reconnect after an invalid token.
- When changing playlist behavior, test both playlist grids and playlist detail
  playback order.

## Playback Notes

- GStreamer initialization happens in `src/main.rs`.
- `PlaybackEngine` owns `playbin`, source header setup, bus polling, playback
  state, seeking, stream handoff, and stop behavior.
- UI code should request playback through the playback module rather than
  configuring GStreamer directly.
- Playback changes should be tested with at least one Jellyfin item that direct
  plays and one scenario that exercises error handling or fallback messaging.
- MPRIS state and metadata should stay in sync with play, pause, stop, previous,
  next, seek, and end-of-stream behavior.

## Waveform Notes

- Waveform generation uses a separate GStreamer pipeline from playback.
- Cache file format changes should bump the waveform cache version.
- Cache writes should remain atomic through a temporary file followed by rename.
- UI changes should preserve loading, ready, seek, and failure states.
- Waveform work must not block the GTK UI thread.

## SQLite and Migrations

Schema SQL lives in `src/cache/schema.rs`; cache access lives in
`src/cache/db.rs`.

When changing persisted data:

- Add migrations in a way that preserves existing local databases when possible.
- Update `SCHEMA_VERSION` only with matching migration behavior.
- Add or update cache tests when practical.
- Document new local data or secret implications in [SECURITY.md](../SECURITY.md).

## AppImage

Build a release AppImage with:

```sh
scripts/build-appimage.sh --release
```

Useful options:

```sh
scripts/build-appimage.sh --output-dir /tmp/gtunes-dist
scripts/build-appimage.sh --appimagetool /path/to/appimagetool
```

The generated AppImage expects GTK4, Libadwaita, GStreamer, and the needed
GStreamer plugins on the target system.

## Troubleshooting

If GTK, Libadwaita, DBus, SQLite, or GStreamer fails to build, confirm the
development packages and `pkg-config` are installed.

If GStreamer playback fails, verify the base plugins are installed and test with
a known playable Jellyfin audio item.

If the app opens with stale account or library data, reset the database and
cache from Settings.

If a saved login stops working, use the Reconnect button and enter the Jellyfin
password for the saved account.

If CI fails while local checks pass, compare native package versions and the
Rust toolchain reported by the workflow logs.
