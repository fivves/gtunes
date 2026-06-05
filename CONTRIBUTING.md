# Contributing to gTunes

gTunes is a released Linux desktop Jellyfin music client. Keep changes focused,
preserve the existing module boundaries, and verify the desktop app still builds
before opening a pull request.

Read [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) before larger changes. It covers
local dependencies, architecture, data storage, troubleshooting, and the expected
verification path.

## Development Flow

1. Branch from the active development branch.
2. Keep changes scoped to one behavior, bug fix, or maintenance task.
3. Run formatting and verification commands before pushing.
4. Include a short summary, test notes, and any manual Jellyfin coverage in the
   pull request.

## Verification

Run these commands before submitting work:

```sh
cargo fmt -- --check
cargo check
cargo test
```

For changes touching shared behavior, also run:

```sh
cargo clippy --all-targets --all-features -- -D warnings
```

Manual verification is expected for changes that affect login, library refresh,
playlists, playback, queue behavior, artwork, waveforms, MPRIS, reconnect, or
cache reset.

## Code Guidelines

- Prefer existing module boundaries over adding new abstractions.
- Keep UI work consistent with GTK4 and Libadwaita conventions.
- Do not block the GTK UI thread on network, disk, image decoding, cache reset,
  or waveform work.
- Keep Jellyfin API response handling typed through `serde` models.
- Keep playback changes routed through the playback module.
- Keep local databases, credentials, tokens, stream URLs, logs, screenshots with
  private server details, and environment files out of git.
