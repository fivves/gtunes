# Discord Rich Presence

gTunes can publish the current song to Discord Rich Presence through the bundled
Discord application ID. Set `GTUNES_DISCORD_CLIENT_ID` only when you want to use
a different Discord application for testing.

## Setup

1. Open <https://discord.com/developers/applications>.
2. Click **New Application**.
3. Name it `gTunes` and create it.
4. On the application's **General Information** page, copy the **Application ID**.
5. Start gTunes with that ID if you want to override the bundled gTunes Discord
   application:

   ```sh
   GTUNES_DISCORD_CLIENT_ID=your_application_id cargo run
   ```

For normal local development with the bundled ID, `cargo run` is enough. When a
Jellyfin track is playing, Discord shows the song title, artist, album, album
art, and the track timer. When playback stops, gTunes clears the activity.

## Optional App Art

Discord application assets can be used as fallback art when no album image is
available. Discord recommends 1024 x 1024 images for Rich Presence assets.

1. In the same Discord application, open **Rich Presence**.
2. Upload an image asset for gTunes.
3. Copy the asset key/name. Discord lowercases asset keys after upload, so use
   the lowercase key shown in the portal.
4. Start gTunes with the key:

   ```sh
   GTUNES_DISCORD_LARGE_IMAGE_KEY=your_asset_key cargo run
   ```

You can also set `GTUNES_DISCORD_SMALL_IMAGE_KEY` to show a small gTunes badge
over the album art.

## Album Art Privacy

Jellyfin artwork URLs can expose your server address or access tokens, so gTunes
does not send Jellyfin image URLs to Discord.

Instead, gTunes uploads the cached album art image bytes to `https://img.fvvs.me`
and sends Discord the returned public image URL. Discord receives an
`img.fvvs.me` URL, not the original Jellyfin URL.

Successful uploads are cached in the local gTunes database by artwork hash, so
the same cover can be reused across app restarts without uploading it again. If
an upload fails, gTunes falls back to `GTUNES_DISCORD_LARGE_IMAGE_KEY` when that
is configured.

If Discord is not running, gTunes keeps playback working normally and retries
the Discord IPC connection in the background.
