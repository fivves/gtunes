# gTunes

gTunes is a Linux desktop music client for Jellyfin-hosted libraries. It is built
with Rust, GTK4, Libadwaita, GStreamer, SQLite, and Reqwest.

The project is currently in MVP-stage development. The app has a runnable
desktop shell, Jellyfin authentication, library sync, cached sessions, track
browsing, queue controls, GStreamer playback, MPRIS integration, album art, and
waveform scaffolding. The next development cycle is focused on hardening the
Jellyfin playback workflow and preparing the app for a reliable V1 release.

## Features

- Connect to a Jellyfin server with username and password credentials.
- Cache the active Jellyfin session and synced library metadata locally.
- Browse tracks, albums, and artists in a GTK4/Libadwaita interface.
- Search and sort tracks.
- Play Jellyfin audio streams through GStreamer.
- Control playback with previous, play/pause, next, shuffle, and queue actions.
- Display Jellyfin album artwork where available.
- Expose media controls through MPRIS with `souvlaki`.
- Maintain SQLite-backed app settings, session state, library cache, and
  waveform cache indexes.
- Reserve UI space for lyrics and waveform work planned after playback
  hardening.

## Project Status

This repository is the active development branch for the MVP-to-V1 cycle.

What works today:

- The application builds and launches as a native Linux GTK app.
- Jellyfin credentials can be submitted through the UI.
- Music library data can be fetched and cached from Jellyfin.
- Tracks can be selected and streamed through GStreamer when Jellyfin returns a
  playable stream URL.

Known V1 work:

- Improve playback error recovery and direct-play/transcoding fallback.
- Expand test coverage around Jellyfin models, cache migration, and playback
  state transitions.
- Finish waveform generation and scrubbing behavior.
- Add synced and unsynced lyrics support.
- Add packaging for target Linux distributions.

See [docs/TECHNICAL_DESIGN.md](docs/TECHNICAL_DESIGN.md) for the architecture
and V1 boundary.

Developer onboarding lives in [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md). The
current product plan lives in [docs/ROADMAP.md](docs/ROADMAP.md).

## Requirements

- Linux desktop environment with GTK4 support.
- Rust toolchain with edition 2024 support.
- GTK4 development libraries.
- Libadwaita development libraries.
- GStreamer runtime and development libraries.
- SQLite development libraries.
- `pkg-config`.
- A Jellyfin server with a music library.

On Arch Linux:

```sh
sudo pacman -S rust gtk4 libadwaita gstreamer gst-plugins-base sqlite pkgconf
```

On Ubuntu or Debian:

```sh
sudo apt install build-essential pkg-config libgtk-4-dev libadwaita-1-dev \
  libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev libsqlite3-dev
```

On Fedora:

```sh
sudo dnf install rust cargo gtk4-devel libadwaita-devel gstreamer1-devel \
  gstreamer1-plugins-base-devel sqlite-devel pkgconf-pkg-config
```

## Getting Started

Clone the repository:

```sh
git clone git@github.com:fivves/gtunes.git
cd gtunes
git switch dev
```

Build the project:

```sh
cargo build
```

Run the app:

```sh
cargo run
```

When the app opens, enter your Jellyfin server URL, username, and password in the
connection panel. The server URL should include the scheme, for example:

```text
https://jellyfin.example.com
```

For local development, a typical URL is:

```text
http://localhost:8096
```

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
  app.rs              Application startup and GTK activation.
  cache/              SQLite migrations, settings, sessions, and cache helpers.
  config.rs           Centralized application metadata.
  jellyfin/           Jellyfin HTTP client and typed API models.
  lyrics/             Placeholder module for planned lyrics support.
  playback/           GStreamer playback engine and playback state.
  ui/                 GTK4/Libadwaita window, widgets, and styles.
  waveform/           Waveform cache keys, generation, and summaries.
docs/
  TECHNICAL_DESIGN.md Architecture, product direction, and V1 boundary.
```

## Data Storage

gTunes uses the platform data directory resolved by the `directories` crate. On
Linux, development builds store data under a path similar to:

```text
~/.local/share/dev/fivves/gTunes/
```

The SQLite cache currently stores app settings, Jellyfin session metadata,
cached library JSON, and waveform cache paths. Do not commit local databases or
session material.

## Security Notes

- Jellyfin access tokens are stored locally in the app SQLite database.
- Passwords are used for authentication and are not intentionally persisted by
  the app.
- Keep `.env` files, local databases, logs, and test credentials out of git.

## Branching

The active development branch is `dev`. Keep feature work branched from `dev`
and merge back into `dev` after review. Release branches can be created later
when packaging and versioned distribution are ready.

## Contributing

1. Start from the `dev` branch.
2. Read [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md).
3. Run `cargo fmt -- --check`.
4. Run `cargo check`.
5. Run `cargo test`.
6. Open a pull request with a concise summary and verification notes.

For vulnerability handling and secret hygiene, see [SECURITY.md](SECURITY.md).

## License

gTunes is licensed under the [MIT License](LICENSE).
