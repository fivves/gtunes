# gTunes

<img width="1896" height="1017" alt="image" src="https://github.com/user-attachments/assets/f2f18885-e19c-4a74-8bd0-6ed74932a98e" />

gTunes is a native Linux desktop music client for Jellyfin-hosted music
libraries. Version 1.1.1 focuses on fast library browsing, reliable Jellyfin
streaming, cached sessions, album artwork, waveform scrubbing, queue control,
and desktop media-key integration.

The app is built with Rust, GTK4, Libadwaita, GStreamer, SQLite, Reqwest, and
Souvlaki.

## Features

- Connect to a Jellyfin server with username and password credentials.
- Save the active Jellyfin session locally so the app can reopen without
  requiring a fresh login.
- Load cached library data immediately when available, then refresh from
  Jellyfin on demand.
- Browse tracks, albums, artists, and playlists from a Libadwaita interface.
- View album artwork, artist images, playlist artwork, and full-size cover art.
- Search the visible library with the search field or type-to-jump navigation.
- Sort the track table by title, artist, album, or duration.
- Open album, artist, and playlist detail views with back navigation.
- Play Jellyfin audio streams through GStreamer.
- Prefer direct Jellyfin streams and fall back to transcoded streams when a
  direct stream fails.
- Use previous, play/pause, next, shuffle, and queue controls.
- Preview the next 15 tracks in the queue and jump directly to upcoming tracks.
- Generate, cache, display, and scrub waveforms for Jellyfin audio streams.
- Show elapsed and remaining playback time.
- Keep playback running after the window is closed, when enabled in settings.
- Expose playback controls and metadata through MPRIS for desktop media keys and
  compatible shells.
- Refresh Jellyfin libraries incrementally using Jellyfin change timestamps when
  available.
- Reconnect with a saved account after an expired or revoked Jellyfin token.
- Reset saved login, cached library data, artwork, and waveform files from the
  settings menu.

gTunes 1.1.1 does not include live, synced, unsynced, embedded, or Jellyfin lyrics
support.

## Requirements

- Linux desktop environment with GTK4 support.
- Rust toolchain with edition 2024 support.
- GTK4 development libraries.
- Libadwaita development libraries.
- GStreamer runtime and development libraries.
- GStreamer base plugins.
- SQLite development libraries.
- DBus development libraries for MPRIS media controls.
- `pkg-config`.
- A Jellyfin server with a music library.

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

## Getting Started

Clone and build the app:

```sh
git clone git@github.com:fivves/gtunes.git
cd gtunes
cargo build
```

Run gTunes:

```sh
cargo run
```

When the app opens, enter your Jellyfin server URL, username, and password. The
server URL must include a scheme:

```text
https://jellyfin.example.com
```

For a local Jellyfin server, use a URL such as:

```text
http://localhost:8096
```

After login, gTunes syncs your Jellyfin music library, saves the session, and
caches the library for faster startup.

## Using gTunes

The left sidebar switches between Tracks, Albums, Artists, and Playlists. The
main view changes between a sortable track table and artwork grids for
collections. Selecting an album, artist, or playlist opens a detail view with
matching tracks.

The player bar contains transport controls, now-playing metadata, the waveform,
elapsed and remaining time, search, shuffle, and settings. Click the artist or
album in the now-playing area to jump to that collection. Click the cover art in
the sidebar to open a larger artwork view.

The waveform is generated from the Jellyfin stream and cached locally. Click or
drag on the waveform to seek within the current track after the waveform is
available.

Settings include:

- Keep playing while closed.
- Refresh library.
- Keyboard shortcuts.
- About gTunes.
- Reset database and cache.
- Quit.

Keyboard shortcuts:

- `Ctrl+F`: focus search.
- `Ctrl+1`: Tracks.
- `Ctrl+2`: Albums.
- `Ctrl+3`: Artists.
- `Ctrl+4`: Playlists.
- Type while the library has focus: jump to matching tracks, albums, artists, or
  playlists.
- `Return`: play the selected type-to-jump result.

## AppImage

Build an AppImage:

```sh
scripts/build-appimage.sh --release
```

The AppImage is written to `dist/`. If `appimagetool` is not installed, the
script downloads a local copy under `target/appimage-tools/`.

The AppImage bundles the gTunes binary and desktop metadata. It still expects
GTK4, Libadwaita, GStreamer, and the required GStreamer plugins to be available
on the target system.

## Development Commands

Format the code:

```sh
cargo fmt
```

Check formatting:

```sh
cargo fmt -- --check
```

Compile and type-check:

```sh
cargo check
```

Run tests:

```sh
cargo test
```

Run Clippy:

```sh
cargo clippy --all-targets --all-features -- -D warnings
```

## Repository Layout

```text
src/
  app.rs              GTK application startup and activation.
  cache/              SQLite schema, settings, sessions, and cache helpers.
  config.rs           Application identifiers and package metadata.
  jellyfin/           Jellyfin HTTP client and typed API models.
  main.rs             Tracing and GStreamer initialization.
  playback/           GStreamer playback engine, queue handoff, and seeking.
  ui/                 GTK4/Libadwaita window, widgets, state, and styles.
  waveform/           Waveform generation, summaries, and cache files.
docs/
  DEVELOPMENT.md      Contributor setup, workflow, and troubleshooting.
  ROADMAP.md          Post-1.0 direction and non-goals.
  TECHNICAL_DESIGN.md Architecture and release behavior.
```

## Data Storage

gTunes uses the platform data and cache directories resolved by the
`directories` crate. On Linux, local development data is stored under paths like:

```text
~/.local/share/dev/fivves/gTunes/
~/.cache/dev/fivves/gTunes/
```

The local data can include Jellyfin access tokens, cached library JSON, saved
settings, and waveform cache records. Temporary artwork cache files use names
like `gtunes-artwork-*` in the system temporary directory.

Do not commit local databases, logs, credentials, access tokens, stream URLs, or
server details.

## Documentation

- [Technical Design](docs/TECHNICAL_DESIGN.md)
- [Development Guide](docs/DEVELOPMENT.md)
- [Roadmap](docs/ROADMAP.md)
- [Security Policy](SECURITY.md)
- [Contributing](CONTRIBUTING.md)

## License

gTunes is licensed under the [MIT License](LICENSE).
