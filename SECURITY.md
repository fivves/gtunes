# Security Policy

Security work for gTunes prioritizes protecting Jellyfin credentials, access
tokens, private server URLs, playback stream URLs, local cache data, and personal
library metadata.

## Supported Versions

The 1.0 release line and the active development branch are supported for
security fixes.

## Reporting a Vulnerability

Report security concerns privately to the repository owner instead of opening a
public issue. Include:

- A concise description of the issue.
- Steps to reproduce it.
- The version or commit hash tested.
- Whether credentials, Jellyfin access tokens, stream URLs, local databases,
  logs, screenshots, or personal library metadata may be exposed.
- Any relevant operating system, desktop environment, and Jellyfin version.

## Secret Handling

- Do not commit Jellyfin passwords, API keys, access tokens, stream URLs,
  screenshots containing private server details, local databases, logs, or
  `.env` files.
- The app stores Jellyfin access tokens in its local SQLite database.
- Passwords are used to authenticate with Jellyfin and are not intentionally
  persisted by the app.
- Direct and transcoded playback URLs can grant access to private media while
  valid and should be treated as sensitive.
- Local development databases and cache files should stay local.

## Local Data

gTunes stores app data through the platform directories resolved by the
`directories` crate. On Linux, local development data is typically under:

```text
~/.local/share/dev/fivves/gTunes/
~/.cache/dev/fivves/gTunes/
```

Temporary artwork cache files are written to the system temporary directory with
names starting with `gtunes-artwork-`.

Use Settings > Reset database and cache to clear saved Jellyfin login, cached
library data, artwork, and waveform files from the app. For manual development
cleanup, remove the data and cache directories and matching temporary artwork
files.
