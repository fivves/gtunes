# Security Policy

gTunes is pre-release software and is not yet packaged for general distribution.
Security work should prioritize protecting Jellyfin credentials, access tokens,
local cache data, and playback URLs.

## Supported Versions

Only the `dev` branch is currently supported. Tagged releases and release
branches will be added when packaging begins.

## Reporting a Vulnerability

For now, report security concerns privately to the repository owner instead of
opening a public issue. Include:

- A concise description of the issue.
- Steps to reproduce it.
- Whether credentials, Jellyfin access tokens, stream URLs, local databases, or
  personal library metadata may be exposed.
- The commit hash tested.

## Secret Handling

- Do not commit Jellyfin passwords, API keys, access tokens, stream URLs,
  screenshots containing private server details, local databases, logs, or
  `.env` files.
- The app stores Jellyfin access tokens in its local SQLite database.
- Passwords are used to authenticate with Jellyfin and are not intentionally
  persisted by the app.
- Local development databases are ignored by git and should stay local.

## Local Data

Development builds store app data under the platform data directory resolved by
the `directories` crate. On Linux, that is typically:

```text
~/.local/share/dev/fivves/gTunes/
```

Delete that directory to remove cached sessions and library data during local
testing.
