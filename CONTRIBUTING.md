# Contributing to gTunes

gTunes is in MVP-stage development. Keep changes focused and verify the desktop
app still builds before opening a pull request.

Read [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) before larger changes. It covers
local dependencies, architecture, data storage, troubleshooting, and the expected
verification path.

## Development Flow

1. Branch from `dev`.
2. Keep changes scoped to one behavior or maintenance task.
3. Run formatting and verification commands before pushing.
4. Include a short summary and test notes in the pull request.

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

## Code Guidelines

- Prefer existing module boundaries over adding new abstractions.
- Keep UI work consistent with GTK4 and Libadwaita conventions.
- Do not block the GTK UI thread on network, disk, image decoding, or waveform
  work.
- Keep Jellyfin API response handling typed through `serde` models.
- Keep local databases, credentials, tokens, and environment files out of git.
