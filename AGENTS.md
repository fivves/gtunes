# Repository Guidelines

## Project Structure & Module Organization

gTunes is a Rust 2024 GTK4/Libadwaita desktop client for Jellyfin music
libraries. Source lives in `src/`: `main.rs` initializes tracing and
GStreamer, `app.rs` starts the GTK application, and `config.rs` stores package
metadata. Feature areas are split into `src/ui/` for windows, widgets, styles,
and state wiring; `src/jellyfin/` for the typed API client and models;
`src/playback/` for GStreamer playback; `src/cache/` for SQLite schema and
local state; and `src/waveform/` for waveform generation and cache files.
Contributor docs are under `docs/`, and AppImage packaging is in
`scripts/build-appimage.sh`.

## Build, Test, and Development Commands

- `cargo run`: build and launch the local desktop app.
- `cargo check`: type-check quickly without producing a release binary.
- `cargo test`: run the Rust test suite.
- `cargo fmt -- --check`: verify standard Rust formatting.
- `cargo clippy --all-targets --all-features -- -D warnings`: run lint checks
  with warnings treated as failures.
- `scripts/build-appimage.sh --release`: build a release AppImage in `dist/`.

## Coding Style & Naming Conventions

Use `rustfmt` defaults and keep modules aligned with existing ownership
boundaries. Prefer typed `serde` models for Jellyfin responses and keep URL
construction centralized in the Jellyfin client. Route playback changes through
`src/playback/` instead of configuring GStreamer directly from UI code. Use
snake_case for functions, variables, and modules; UpperCamelCase for types; and
SCREAMING_SNAKE_CASE for constants.

## Testing Guidelines

Add focused Rust tests for isolated cache, parsing, or state logic when
practical. Run `cargo test` before submitting changes, and include manual
verification notes for behavior that needs a live Jellyfin server: login,
library refresh, playlists, playback, stream fallback, artwork, waveforms,
MPRIS, reconnect, and cache reset.

## Commit & Pull Request Guidelines

Recent commits use short, imperative subjects such as `Add Next Up queue page`
or `Fix now playing navigation links`. Keep each commit scoped to one behavior
or maintenance task. Pull requests should include a concise summary, linked
issues when applicable, test results, and manual Jellyfin coverage. Add
screenshots or screen recordings for visible UI changes.

## Release Notes Guidelines

Release pages under `docs/releases/` should focus on user-facing changes and
not include a `Release Artifact` section or generic packaging boilerplate.
Future release notes should describe what changed and why it matters, while the
GitHub release asset list speaks for itself.

## Security & Configuration Tips

Never commit local databases, credentials, access tokens, stream URLs, private
server addresses, `.env` files, or screenshots with sensitive server details.
Local data is stored through the `directories` crate, typically under
`~/.local/share/dev/fivves/gTunes/` and `~/.cache/dev/fivves/gTunes/` on Linux.
