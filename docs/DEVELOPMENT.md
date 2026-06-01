# Development Guide

This guide is for developers working on gTunes from the `dev` branch.

## Project Shape

gTunes is a native Linux desktop app written in Rust. The UI is built with GTK4
and Libadwaita, playback runs through GStreamer, Jellyfin communication uses
Reqwest, and local state is stored in SQLite.

The main module boundaries are:

- `src/app.rs`: application startup, style setup, and GTK activation.
- `src/ui/`: Libadwaita window, widgets, state wiring, and CSS.
- `src/jellyfin/`: Jellyfin client methods and typed API models.
- `src/playback/`: GStreamer playback state, seeking, and stream control.
- `src/cache/`: SQLite migrations, app settings, sessions, and cache lookups.
- `src/waveform/`: waveform cache keys, summaries, and generation.
- `src/lyrics/`: placeholder module for upcoming lyrics support.

The architecture intent is documented in
[TECHNICAL_DESIGN.md](TECHNICAL_DESIGN.md).

## Prerequisites

Install Rust and the native libraries needed by GTK, Libadwaita, GStreamer, and
SQLite.

Arch Linux:

```sh
sudo pacman -S rust gtk4 libadwaita gstreamer gst-plugins-base sqlite pkgconf
```

Ubuntu or Debian:

```sh
sudo apt install build-essential pkg-config libgtk-4-dev libadwaita-1-dev \
  libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev libsqlite3-dev
```

Fedora:

```sh
sudo dnf install rust cargo gtk4-devel libadwaita-devel gstreamer1-devel \
  gstreamer1-plugins-base-devel sqlite-devel pkgconf-pkg-config
```

## First Run

Clone and build from `dev`:

```sh
git clone git@github.com:fivves/gtunes.git
cd gtunes
git switch dev
cargo check
cargo run
```

In the connection panel, use a Jellyfin URL with a scheme:

```text
http://localhost:8096
```

or:

```text
https://jellyfin.example.com
```

## Verification

Run the same checks that CI runs:

```sh
cargo fmt -- --check
cargo check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

`cargo test` currently runs the test harness but there are no unit tests yet.
Add focused tests when changing isolated logic such as cache keys, Jellyfin model
mapping, formatting helpers, or playback state behavior.

## Local Data and Resetting State

The cache database is created through the `directories` crate. On Linux,
development data is usually stored at:

```text
~/.local/share/dev/fivves/gTunes/
```

That directory can contain Jellyfin access tokens, cached library metadata, and
waveform cache records. Remove it when you need a clean local session:

```sh
rm -rf ~/.local/share/dev/fivves/gTunes
```

Do not commit local databases, logs, `.env` files, credentials, tokens, or test
server details.

## Development Workflow

1. Branch from `dev`.
2. Keep changes focused around one behavior or maintenance task.
3. Preserve the current module ownership boundaries.
4. Run the verification commands before opening a pull request.
5. Include manual Jellyfin playback notes when a change affects login, sync,
   queueing, artwork, waveform, or playback.

## UI Guidelines

- Keep the GTK UI thread responsive.
- Do network, disk, image decoding, and waveform work off the UI thread.
- Prefer GTK4 and Libadwaita widgets, style classes, and theme colors over
  fixed custom visuals.
- Keep transport controls and now-playing state visible at practical window
  widths.
- Treat Hyprland tiling and frequent resizing as normal usage.

## Jellyfin Development Notes

- Keep Jellyfin response parsing typed with `serde` models in `src/jellyfin`.
- Keep URL construction centralized in the Jellyfin client.
- Include the scheme in server URLs during manual testing.
- Avoid logging passwords, tokens, or stream URLs.
- When touching playback URLs, test both direct playback and failure messaging.

## Playback Development Notes

- GStreamer initialization happens in `src/main.rs`.
- `PlaybackEngine` owns `playbin`, bus polling, state transitions, seeking, and
  stop behavior.
- UI code should interact with playback through the playback module rather than
  configuring GStreamer directly.
- Playback changes should be manually tested with at least one Jellyfin audio
  item.

## SQLite and Migrations

Schema SQL lives in `src/cache/schema.rs`. Cache access lives in
`src/cache/db.rs`.

When changing persisted data:

- Add migrations in a way that preserves existing local development databases.
- Update `SCHEMA_VERSION`.
- Add or update cache tests when practical.
- Document new local data or secret implications in `SECURITY.md` if relevant.

## Troubleshooting

If GTK or Libadwaita fails to build, confirm the development packages and
`pkg-config` are installed.

If GStreamer playback fails, verify the base plugins are installed and test with
a known playable Jellyfin audio item.

If the app opens with stale account or library data, remove the local data
directory described above.

If CI fails while local checks pass, compare installed native package versions
and the Rust toolchain reported by the workflow logs.
