# YouTube Music OAuth Setup

This guide explains how to create the Google OAuth client ID that gTunes needs
for the YouTube Music prototype, then how a new user signs in so gTunes can
create and store an OAuth token locally.

The token is created by Google after the user signs in. Users should not create
or paste raw access tokens manually.

## What This Enables

- gTunes can ask Google for permission to search YouTube music videos.
- gTunes stores the returned OAuth session in its local SQLite settings store.
- gTunes refreshes the access token when possible.
- Search results and metadata come from the official YouTube Data API.
- Native audio playback is resolved with `yt-dlp` and played by GStreamer.

The current prototype uses the official YouTube Data API. Google does not expose
raw YouTube Music audio stream URLs through that API, so in-app GStreamer
playback uses `yt-dlp` to resolve the selected result into a playable stream URL.

## Requirements

- A Google account.
- A YouTube Music Premium account for playback in YouTube Music.
- Access to Google Cloud Console.
- A Google Cloud project for gTunes.
- YouTube Data API v3 enabled on that project.
- A Desktop OAuth client ID and client secret.
- `yt-dlp` installed on the user's system for native audio stream resolution.
- Browser cookies for account-only YouTube Music streams, when required.

## Step 1: Create or Select a Google Cloud Project

1. Open Google Cloud Console:
   <https://console.cloud.google.com/>
2. Use the project selector at the top of the page.
3. Select an existing gTunes project, or create a new project.
4. Give the project a clear name, for example:

   ```text
   gTunes YouTube Music
   ```

## Step 2: Enable YouTube Data API v3

1. Open APIs and Services in Google Cloud Console.
2. Go to Library.
3. Search for:

   ```text
   YouTube Data API v3
   ```

4. Open the API page.
5. Click Enable.

gTunes uses this API for authenticated search and video metadata lookup.

## Step 3: Configure the OAuth Consent Screen

1. Open APIs and Services.
2. Go to OAuth consent screen or Google Auth Platform.
3. Choose the user type:
   - Internal: only available for Google Workspace organizations.
   - External: use this for normal Google accounts.
4. Fill in the required app information:
   - App name: `gTunes`
   - User support email: your support email
   - Developer contact email: your developer email
5. Save and continue.
6. Add this scope:

   ```text
   https://www.googleapis.com/auth/youtube.readonly
   ```

7. If the app is in Testing mode, add every test user's Google account under
   Test users.
8. Save the consent screen.

For development builds, Testing mode is usually enough. For distribution to
users outside the test-user list, publish the app and complete any Google
verification steps Google requires for the requested scope.

## Step 4: Create a Desktop OAuth Client ID

1. Open APIs and Services.
2. Go to Credentials or Google Auth Platform > Clients.
3. Click Create credentials.
4. Choose OAuth client ID.
5. Select Desktop app as the application type.
6. Name it:

   ```text
   gTunes Desktop
   ```

7. Click Create.
8. Copy the generated Client ID.
9. Copy the generated Client secret.

The Client ID usually looks like this:

```text
1234567890-abcdefghijklmnopqrstuvwxyz.apps.googleusercontent.com
```

Do not use a Web application client for gTunes. The prototype uses Google's
installed-app OAuth flow with a temporary loopback redirect on `127.0.0.1`.

## Step 5: Give the Client ID and Secret to a New User

Send the user the Desktop app Client ID and Client secret from Step 4.

For this desktop prototype, Google may require the client secret when gTunes
exchanges the authorization code for access and refresh tokens.

Do not send:

- Access tokens.
- Refresh tokens.
- Your Google account password.

Do not use a Web application client secret. Use the secret from the Desktop app
OAuth client.

## Step 6: User Signs In Inside gTunes

1. Start gTunes.
2. Open the Streaming section in the left sidebar.
3. Select YouTube Music.
4. Paste the Google OAuth Client ID into the OAuth client ID field.
5. Paste the Google OAuth Client secret into the OAuth client secret field.
6. Click Sign in.
7. gTunes opens the system browser.
8. Sign in with the Google account that has YouTube Music Premium.
9. Review the consent prompt.
10. Approve access.
11. The browser should show a small gTunes authorization received page.
12. Return to gTunes.

After Google accepts the token exchange, gTunes stores the OAuth session
locally. The user should not need to sign in again until the token is expired,
revoked, or the local database is reset.

## Step 7: Search YouTube Music

1. Stay on the YouTube Music page.
2. Type a query into Search YouTube Music.
3. Press Enter or click Search.
4. Results appear in the list view.
5. Double-click a result, or activate it from the list.

gTunes asks `yt-dlp` to resolve the selected result:

```text
https://music.youtube.com/watch?v=VIDEO_ID
```

The resolved audio stream is passed to the native GStreamer player.

If the stream requires a signed-in YouTube Music account, sign in to YouTube
Music in a supported browser first. gTunes tries `yt-dlp` without cookies, then
with cookies from common browsers such as Firefox, Chrome, Chromium, and Brave.

## Troubleshooting

### The User Sees Access Blocked

Check the OAuth consent screen:

- The user may need to be added under Test users.
- The app may still be in Testing mode.
- The requested scope may require app verification for broader distribution.

### The User Sees redirect_uri_mismatch

Make sure the OAuth client type is Desktop app, not Web application.

gTunes creates a temporary loopback redirect URI at runtime, such as:

```text
http://127.0.0.1:49152
```

Desktop OAuth clients support this installed-app loopback flow.

### The Browser Says Authorization Received, But gTunes Shows An Error

This means gTunes received Google's authorization code, but Google rejected the
code-to-token exchange.

Read the full error shown in gTunes:

- `invalid_client`: the OAuth client ID is wrong, deleted, or not a Desktop app
  client, or the client secret is wrong.
- `client secret is missing`: paste the Desktop app client secret into gTunes
  and sign in again.
- `invalid_grant`: retry sign-in. If it repeats, recreate the Desktop OAuth
  client and make sure the app is using the latest gTunes build.
- `redirect_uri_mismatch`: create a Desktop app OAuth client. Do not use a Web
  application client.
- `access_denied`: the user denied access or is not allowed by the OAuth
  consent screen.

### Search Fails After Sign-In

Check these items:

- YouTube Data API v3 is enabled for the same Google Cloud project.
- The OAuth client ID belongs to that project.
- The user approved the YouTube readonly scope.
- The user's Google account is allowed to use the app.
- The local network can reach Google APIs.

### Playback Does Not Start In gTunes

Check these items:

- `yt-dlp` is installed and available on `PATH`.
- `yt-dlp` can resolve the same URL from a terminal.
- If the content requires an account, the user is signed in to YouTube Music in
  Firefox, Chrome, Chromium, or Brave.
- Browser cookie storage is accessible to `yt-dlp`.

The Google OAuth token is used for YouTube search and metadata. It is not a
YouTube Music playback token and cannot be used by the official API to fetch raw
audio streams.

## References

- Google OAuth for desktop apps:
  <https://developers.google.com/identity/protocols/oauth2/native-app>
- YouTube Data API authentication:
  <https://developers.google.com/youtube/v3/guides/authentication>
- YouTube Data API overview:
  <https://developers.google.com/youtube/v3/getting-started>
