use discord_rich_presence::{
    DiscordIpc, DiscordIpcClient,
    activity::{self, Activity},
};
use std::collections::{HashMap, HashSet};
use std::env;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::cache::CacheDatabase;

const CLIENT_ID_ENV: &str = "GTUNES_DISCORD_CLIENT_ID";
const DEFAULT_CLIENT_ID: &str = "1519118864787574935";
const LARGE_IMAGE_KEY_ENV: &str = "GTUNES_DISCORD_LARGE_IMAGE_KEY";
const SMALL_IMAGE_KEY_ENV: &str = "GTUNES_DISCORD_SMALL_IMAGE_KEY";
const PICTSHARE_UPLOAD_URL: &str = "https://img.fvvs.me/api/upload.php";
const DISCORD_RETRY_INTERVAL: Duration = Duration::from_secs(30);
// A cleared presence is re-sent on this cadence. Discord keeps a stale track on
// the profile when a single clear goes missing, and the app only pushes presence
// updates when playback changes, so re-asserting the cleared state is what makes
// a dropped clear self-heal.
const PRESENCE_RECONCILE_INTERVAL: Duration = Duration::from_secs(20);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PresencePlaybackState {
    Playing,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PresenceActivity {
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub artwork_source_url: Option<String>,
    pub playback_state: PresencePlaybackState,
    pub position: Option<Duration>,
    pub duration: Option<Duration>,
}

pub struct DiscordPresence {
    sender: mpsc::Sender<PresenceCommand>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PresenceCommand {
    Set(PresenceActivity),
    ArtworkUploaded {
        source_url: String,
        public_url: String,
    },
    ArtworkUploadFailed {
        source_url: String,
    },
    Clear,
    Shutdown,
}

#[derive(Debug)]
struct PresenceConfig {
    client_id: String,
    large_image_key: Option<String>,
    small_image_key: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct PictShareUploadResponse {
    status: String,
    url: Option<String>,
    reason: Option<String>,
}

impl DiscordPresence {
    pub fn from_env() -> Option<Self> {
        let client_id = env::var(CLIENT_ID_ENV).unwrap_or_else(|_| DEFAULT_CLIENT_ID.to_string());
        let client_id = client_id.trim().to_string();
        if client_id.is_empty() {
            return None;
        }

        let config = PresenceConfig {
            client_id,
            large_image_key: env_optional(LARGE_IMAGE_KEY_ENV),
            small_image_key: env_optional(SMALL_IMAGE_KEY_ENV),
        };
        let (sender, receiver) = mpsc::channel();
        let worker_sender = sender.clone();
        std::thread::Builder::new()
            .name("gtunes-discord-rpc".to_string())
            .spawn(move || run_presence_worker(config, receiver, worker_sender))
            .ok()?;

        Some(Self { sender })
    }

    pub fn set_activity(&self, activity: PresenceActivity) {
        let _ = self.sender.send(PresenceCommand::Set(activity));
    }

    pub fn clear_activity(&self) {
        let _ = self.sender.send(PresenceCommand::Clear);
    }
}

pub fn artwork_cache_path(url: &str) -> PathBuf {
    std::env::temp_dir().join(format!("gtunes-artwork-{}", artwork_cache_id(url)))
}

fn artwork_cache_id(url: &str) -> String {
    // FNV-1a 64-bit: stable across Rust versions, zero dependencies
    const FNV_OFFSET: u64 = 14695981039346656037;
    const FNV_PRIME: u64 = 1099511628211;
    let hash = url.bytes().fold(FNV_OFFSET, |acc, byte| {
        (acc ^ byte as u64).wrapping_mul(FNV_PRIME)
    });
    format!("{hash:x}")
}

impl Drop for DiscordPresence {
    fn drop(&mut self) {
        let _ = self.sender.send(PresenceCommand::Shutdown);
    }
}

fn run_presence_worker(
    config: PresenceConfig,
    receiver: mpsc::Receiver<PresenceCommand>,
    sender: mpsc::Sender<PresenceCommand>,
) {
    let mut worker = PresenceWorker::new(config, sender);

    loop {
        match receiver.recv_timeout(PRESENCE_RECONCILE_INTERVAL) {
            Ok(PresenceCommand::Shutdown) => break,
            Ok(command) => worker.handle(command),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    worker.shutdown();
}

/// Owns the Discord IPC connection and keeps the published presence in sync with
/// the playback state the UI last reported.
struct PresenceWorker {
    config: PresenceConfig,
    client: DiscordIpcClient,
    connected: bool,
    next_connect_attempt: Instant,
    next_clear_assert: Instant,
    artwork_cache: HashMap<String, String>,
    uploading_artwork: HashSet<String>,
    sender: mpsc::Sender<PresenceCommand>,
    desired: Option<PresenceActivity>,
    applied: Option<PresenceActivity>,
}

impl PresenceWorker {
    fn new(config: PresenceConfig, sender: mpsc::Sender<PresenceCommand>) -> Self {
        Self::with_artwork_cache(config, sender, load_persisted_artwork_cache())
    }

    fn with_artwork_cache(
        config: PresenceConfig,
        sender: mpsc::Sender<PresenceCommand>,
        artwork_cache: HashMap<String, String>,
    ) -> Self {
        Self {
            client: DiscordIpcClient::new(&config.client_id),
            config,
            connected: false,
            next_connect_attempt: Instant::now(),
            next_clear_assert: Instant::now(),
            artwork_cache,
            uploading_artwork: HashSet::new(),
            sender,
            desired: None,
            applied: None,
        }
    }

    fn handle(&mut self, command: PresenceCommand) {
        match command {
            PresenceCommand::Set(activity) => {
                self.desired = Some(activity.clone());
                queue_artwork_upload(
                    &activity,
                    &self.artwork_cache,
                    &mut self.uploading_artwork,
                    &self.sender,
                );
            }
            PresenceCommand::ArtworkUploaded {
                source_url,
                public_url,
            } => {
                let cache_id = artwork_cache_id(&source_url);
                self.uploading_artwork.remove(&cache_id);
                self.artwork_cache.insert(cache_id, public_url.clone());
                persist_artwork_url(&source_url, &public_url);
            }
            PresenceCommand::ArtworkUploadFailed { source_url } => {
                self.uploading_artwork
                    .remove(&artwork_cache_id(&source_url));
            }
            PresenceCommand::Clear => {
                self.desired = None;
            }
            // Shutdown is handled by the worker loop, which breaks out of it.
            PresenceCommand::Shutdown => return,
        }

        self.reconcile();
    }

    /// Publishes the presence when it changed, and re-asserts a cleared one on a
    /// cadence.
    ///
    /// The cleared state is re-sent because Discord keeps a stale track on the
    /// profile when a single clear is dropped, and the app only pushes presence
    /// updates when playback changes. A playing activity is not repeated on a
    /// cadence: its timestamps come from the position captured on the last sync,
    /// so repeating it would restart the elapsed time Discord shows.
    fn reconcile(&mut self) {
        let published = self.published();
        let now = Instant::now();
        let due_for_clear_assert =
            published.is_none() && self.connected && now >= self.next_clear_assert;

        if published == self.applied && !due_for_clear_assert {
            return;
        }

        if !self.ensure_connected() {
            return;
        }

        match self.publish(published.as_ref()) {
            Ok(true) => {
                if published.is_none() {
                    self.next_clear_assert = now + PRESENCE_RECONCILE_INTERVAL;
                }
                self.applied = published;
            }
            // Discord refused the update, so leave it unconfirmed and publish it
            // again on the next reconcile.
            Ok(false) => {}
            Err(error) => {
                tracing::debug!(%error, "failed to update Discord Rich Presence");
                self.disconnect();
            }
        }
    }

    /// The activity as it should be published, with mirrored artwork folded in.
    fn published(&self) -> Option<PresenceActivity> {
        self.desired
            .as_ref()
            .map(|activity| activity.with_cached_artwork(&self.artwork_cache))
    }

    /// Writes one presence command and reads the reply Discord sends back,
    /// returning whether Discord accepted the update.
    ///
    /// Reading replies is required rather than optional. The IPC server answers
    /// every command, and those answers used to go unread, piling up in the
    /// socket until writes started to fail, which silently dropped presence
    /// updates such as the clear sent when playback pauses.
    fn publish(
        &mut self,
        published: Option<&PresenceActivity>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        match published {
            Some(activity) => self
                .client
                .set_activity(discord_activity(activity, &self.config))?,
            None => self.client.clear_activity()?,
        }

        let (opcode, payload) = self.client.recv()?;
        if payload.get("evt").and_then(|value| value.as_str()) == Some("ERROR") {
            tracing::debug!(opcode, %payload, "Discord rejected the presence update");
            return Ok(false);
        }

        Ok(true)
    }

    fn ensure_connected(&mut self) -> bool {
        if self.connected {
            return true;
        }

        let now = Instant::now();
        if now < self.next_connect_attempt {
            return false;
        }

        match self.client.connect() {
            Ok(()) => {
                self.connected = true;
                // Discord starts from a clean slate on a new connection, so the
                // current state has to be published again.
                self.applied = None;
                self.next_clear_assert = now;
                true
            }
            Err(error) => {
                tracing::debug!(%error, "failed to connect to Discord Rich Presence");
                self.next_connect_attempt = now + DISCORD_RETRY_INTERVAL;
                false
            }
        }
    }

    fn disconnect(&mut self) {
        self.connected = false;
        let _ = self.client.close();
        self.next_connect_attempt = Instant::now() + DISCORD_RETRY_INTERVAL;
    }

    fn shutdown(&mut self) {
        let _ = self.client.clear_activity();
        let _ = self.client.close();
    }
}

impl PresenceActivity {
    fn with_cached_artwork(&self, cache: &HashMap<String, String>) -> Self {
        if let Some(url) = self
            .artwork_source_url
            .as_deref()
            .and_then(|source_url| cache.get(&artwork_cache_id(source_url)))
        {
            self.with_public_artwork(url.clone())
        } else {
            self.clone()
        }
    }

    fn with_public_artwork(&self, public_url: String) -> Self {
        let mut activity = self.clone();
        activity.artwork_source_url = Some(public_url);
        activity
    }
}

fn queue_artwork_upload(
    activity: &PresenceActivity,
    cache: &HashMap<String, String>,
    uploading: &mut HashSet<String>,
    sender: &mpsc::Sender<PresenceCommand>,
) {
    let Some(source_url) = activity.artwork_source_url.clone() else {
        return;
    };

    let cache_id = artwork_cache_id(&source_url);
    if cache.contains_key(&cache_id) || uploading.contains(&cache_id) {
        return;
    }

    let upload_cache_id = cache_id.clone();
    uploading.insert(cache_id);
    let sender = sender.clone();
    let spawn_result = std::thread::Builder::new()
        .name("gtunes-discord-artwork-upload".to_string())
        .spawn(move || match upload_artwork_file(&source_url) {
            Ok(public_url) => {
                let _ = sender.send(PresenceCommand::ArtworkUploaded {
                    source_url,
                    public_url,
                });
            }
            Err(error) => {
                tracing::warn!(%error, "failed to upload Discord artwork");
                let _ = sender.send(PresenceCommand::ArtworkUploadFailed { source_url });
            }
        });
    if let Err(error) = spawn_result {
        tracing::warn!(%error, "failed to start Discord artwork upload worker");
        uploading.remove(&upload_cache_id);
    }
}

fn load_persisted_artwork_cache() -> HashMap<String, String> {
    match CacheDatabase::open_default().and_then(|cache| cache.load_discord_artwork_urls()) {
        Ok(urls) => urls
            .into_iter()
            .filter(|(_, url)| validate_pictshare_url(url).is_ok())
            .collect(),
        Err(error) => {
            tracing::debug!(%error, "failed to load Discord artwork URL cache");
            HashMap::new()
        }
    }
}

fn persist_artwork_url(source_url: &str, public_url: &str) {
    let hash = artwork_cache_id(source_url);
    if let Err(error) = CacheDatabase::open_default()
        .and_then(|cache| cache.save_discord_artwork_url(&hash, public_url))
    {
        tracing::debug!(%error, "failed to persist Discord artwork URL");
    }
}

fn upload_artwork_file(source_url: &str) -> Result<String, String> {
    let bytes = cached_or_downloaded_artwork_bytes(source_url)?;
    let (file_name, content_type) = discord_artwork_file_metadata(&bytes);
    let part = reqwest::blocking::multipart::Part::bytes(bytes)
        .file_name(file_name)
        .mime_str(content_type)
        .map_err(|error| error.to_string())?;
    let form = reqwest::blocking::multipart::Form::new().part("file", part);
    let response = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| error.to_string())?
        .post(PICTSHARE_UPLOAD_URL)
        .multipart(form)
        .send()
        .map_err(|error| error.to_string())?;

    if !response.status().is_success() {
        return Err(format!("PictShare returned HTTP {}", response.status()));
    }

    let upload = response
        .json::<PictShareUploadResponse>()
        .map_err(|error| error.to_string())?;
    if upload.status != "ok" {
        return Err(upload
            .reason
            .unwrap_or_else(|| "PictShare upload failed".to_string()));
    }

    let url = upload.url.ok_or("PictShare upload did not return a URL")?;
    validate_pictshare_url(&url)?;
    Ok(url)
}

fn cached_or_downloaded_artwork_bytes(source_url: &str) -> Result<Vec<u8>, String> {
    let path = artwork_cache_path(source_url);
    if path.exists() {
        return std::fs::read(&path).map_err(|error| error.to_string());
    }

    let response = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| error.to_string())?
        .get(source_url)
        .send()
        .map_err(|error| error.to_string())?;

    if !response.status().is_success() {
        return Err(format!(
            "artwork request returned HTTP {}",
            response.status()
        ));
    }

    let bytes = response
        .bytes()
        .map_err(|error| error.to_string())?
        .to_vec();
    std::fs::write(&path, &bytes).map_err(|error| error.to_string())?;
    Ok(bytes)
}

fn discord_artwork_file_metadata(bytes: &[u8]) -> (&'static str, &'static str) {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        ("cover.png", "image/png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        ("cover.jpg", "image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        ("cover.gif", "image/gif")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        ("cover.webp", "image/webp")
    } else {
        ("cover.jpg", "application/octet-stream")
    }
}

fn validate_pictshare_url(raw_url: &str) -> Result<(), String> {
    let url = url::Url::parse(raw_url).map_err(|error| error.to_string())?;
    if url.scheme() != "https" || url.host_str() != Some("img.fvvs.me") {
        return Err(format!("unexpected PictShare URL: {raw_url}"));
    }
    Ok(())
}

fn discord_activity<'a>(
    presence: &'a PresenceActivity,
    config: &'a PresenceConfig,
) -> Activity<'a> {
    let mut activity = activity::Activity::new()
        .details(truncate_discord_text(&presence.title))
        .state(truncate_discord_text(&presence.artist))
        .activity_type(activity::ActivityType::Listening)
        .status_display_type(activity::StatusDisplayType::State)
        .assets(discord_assets(presence, config));

    if presence.playback_state == PresencePlaybackState::Playing
        && let Some(timestamps) = discord_timestamps(presence.position, presence.duration)
    {
        activity = activity.timestamps(timestamps);
    }

    activity
}

fn discord_assets<'a>(
    presence: &'a PresenceActivity,
    config: &'a PresenceConfig,
) -> activity::Assets<'a> {
    // Only publish artwork once it has been mirrored to the public PictShare
    // host; the original Jellyfin URL embeds the private server address and
    // access token, which must never be sent to Discord.
    let large_image = presence
        .artwork_source_url
        .as_deref()
        .filter(|url| validate_pictshare_url(url).is_ok())
        .or(config.large_image_key.as_deref());
    let large_text = presence.album.as_deref().unwrap_or("gTunes");

    let mut assets = activity::Assets::new().large_text(truncate_discord_text(large_text));
    if let Some(image) = large_image {
        assets = assets.large_image(image);
    }
    if let Some(image) = config.small_image_key.as_deref() {
        assets = assets.small_image(image).small_text("gTunes");
    }
    assets
}

fn discord_timestamps(
    position: Option<Duration>,
    duration: Option<Duration>,
) -> Option<activity::Timestamps> {
    let duration = duration?;
    if duration.is_zero() {
        return None;
    }

    let now = unix_timestamp_secs();
    let position = position.unwrap_or_default().min(duration);
    let start = now.saturating_sub(position.as_secs() as i64);
    let end = start.saturating_add(duration.as_secs() as i64);
    Some(activity::Timestamps::new().start(start).end(end))
}

fn unix_timestamp_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or_default()
}

fn env_optional(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn truncate_discord_text(text: &str) -> &str {
    const LIMIT: usize = 128;

    if text.len() <= LIMIT {
        return text;
    }

    let mut end = LIMIT;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_on_utf8_boundary() {
        let text = "a".repeat(127) + "é";

        assert_eq!(truncate_discord_text(&text), "a".repeat(127));
    }

    #[test]
    fn timestamp_uses_position_to_compute_elapsed_start() {
        let presence = PresenceActivity {
            title: "Song".to_string(),
            artist: "Artist".to_string(),
            album: Some("Album".to_string()),
            artwork_source_url: None,
            playback_state: PresencePlaybackState::Playing,
            position: Some(Duration::from_secs(30)),
            duration: Some(Duration::from_secs(120)),
        };
        let config = PresenceConfig {
            client_id: "123".to_string(),
            large_image_key: None,
            small_image_key: None,
        };

        let payload =
            serde_json::to_value(discord_activity(&presence, &config)).expect("serialize");
        let timestamps = payload
            .get("timestamps")
            .expect("timestamps should be present");
        let start = timestamps
            .get("start")
            .and_then(serde_json::Value::as_i64)
            .expect("start timestamp");
        let end = timestamps
            .get("end")
            .and_then(serde_json::Value::as_i64)
            .expect("end timestamp");

        let now = unix_timestamp_secs();
        assert!(start <= now - 30);
        assert!(end >= now + 89);
    }

    #[test]
    fn activity_uses_listening_status_with_artist_display() {
        let presence = PresenceActivity {
            title: "Song".to_string(),
            artist: "Artist".to_string(),
            album: Some("Album".to_string()),
            artwork_source_url: None,
            playback_state: PresencePlaybackState::Playing,
            position: None,
            duration: None,
        };
        let config = PresenceConfig {
            client_id: "123".to_string(),
            large_image_key: None,
            small_image_key: None,
        };

        let payload =
            serde_json::to_value(discord_activity(&presence, &config)).expect("serialize");

        assert_eq!(
            payload.get("type").and_then(serde_json::Value::as_u64),
            Some(2)
        );
        assert_eq!(
            payload
                .get("status_display_type")
                .and_then(serde_json::Value::as_u64),
            Some(1)
        );
        assert_eq!(
            payload.get("state").and_then(serde_json::Value::as_str),
            Some("Artist")
        );
    }

    #[test]
    fn activity_never_sends_private_artwork_urls_to_discord() {
        let presence = PresenceActivity {
            title: "Song".to_string(),
            artist: "Artist".to_string(),
            album: Some("Album".to_string()),
            artwork_source_url: Some(
                "https://jellyfin.example/Items/1/Images/Primary?api_key=secret".to_string(),
            ),
            playback_state: PresencePlaybackState::Playing,
            position: None,
            duration: None,
        };
        let config = PresenceConfig {
            client_id: "123".to_string(),
            large_image_key: None,
            small_image_key: None,
        };

        let payload =
            serde_json::to_value(discord_activity(&presence, &config)).expect("serialize");
        assert!(!payload.to_string().contains("secret"));

        let public = PresenceActivity {
            artwork_source_url: Some("https://img.fvvs.me/abc.jpg".to_string()),
            ..presence
        };
        let payload = serde_json::to_value(discord_activity(&public, &config)).expect("serialize");
        assert!(payload.to_string().contains("img.fvvs.me"));
    }

    #[test]
    fn validates_only_configured_pictshare_urls() {
        assert!(validate_pictshare_url("https://img.fvvs.me/abc123.jpg").is_ok());
        assert!(validate_pictshare_url("https://example.com/abc123.jpg").is_err());
        assert!(validate_pictshare_url("http://img.fvvs.me/abc123.jpg").is_err());
    }

    #[test]
    fn artwork_upload_uses_extension_from_image_bytes() {
        assert_eq!(
            discord_artwork_file_metadata(b"\x89PNG\r\n\x1a\nrest"),
            ("cover.png", "image/png")
        );
        assert_eq!(
            discord_artwork_file_metadata(b"\xff\xd8\xffrest"),
            ("cover.jpg", "image/jpeg")
        );
        assert_eq!(
            discord_artwork_file_metadata(b"RIFFxxxxWEBPrest"),
            ("cover.webp", "image/webp")
        );
    }

    #[test]
    fn cached_artwork_replaces_the_source_url() {
        let source_url = "https://jellyfin.example/Items/1/Images/Primary?api_key=secret";
        let activity = PresenceActivity {
            title: "Song".to_string(),
            artist: "Artist".to_string(),
            album: Some("Album".to_string()),
            artwork_source_url: Some(source_url.to_string()),
            playback_state: PresencePlaybackState::Playing,
            position: None,
            duration: None,
        };
        let mut cache = HashMap::new();
        cache.insert(
            artwork_cache_id(source_url),
            "https://img.fvvs.me/abc.jpg".to_string(),
        );

        let published = activity.with_cached_artwork(&cache);

        assert_eq!(
            published.artwork_source_url.as_deref(),
            Some("https://img.fvvs.me/abc.jpg")
        );
    }

    #[test]
    fn artwork_cache_path_uses_hashed_source_url() {
        let path =
            artwork_cache_path("https://jellyfin.example/Items/1/Images/Primary?api_key=secret");

        assert!(path.starts_with(std::env::temp_dir()));
        assert!(!path.to_string_lossy().contains("secret"));
    }

    /// Binds a fake Discord IPC socket and answers commands the way the real
    /// server does: a READY frame for the handshake, and one reply per command.
    /// Queued replies are used in order, so a test can reject a specific update.
    fn fake_ipc_server(
        dir: &std::path::Path,
    ) -> (
        mpsc::Receiver<serde_json::Value>,
        mpsc::Sender<serde_json::Value>,
    ) {
        let path = dir.join("discord-ipc-0");
        let listener =
            std::os::unix::net::UnixListener::bind(&path).expect("bind fake Discord IPC socket");
        let (frame_sender, frame_receiver) = mpsc::channel();
        let (reply_sender, reply_receiver) = mpsc::channel::<serde_json::Value>();

        std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            loop {
                let Some((opcode, payload)) = read_frame(&mut stream) else {
                    return;
                };
                if frame_sender.send(payload).is_err() {
                    return;
                }

                let reply = if opcode == 0 {
                    serde_json::json!({"cmd": "DISPATCH", "evt": "READY", "data": {}})
                } else {
                    reply_receiver.try_recv().unwrap_or_else(
                        |_| serde_json::json!({"cmd": "SET_ACTIVITY", "data": null, "evt": null}),
                    )
                };
                if write_frame(&mut stream, 1, &reply).is_err() {
                    return;
                }
            }
        });

        (frame_receiver, reply_sender)
    }

    fn read_frame(stream: &mut std::os::unix::net::UnixStream) -> Option<(u32, serde_json::Value)> {
        use std::io::Read;

        let mut header = [0_u8; 8];
        stream.read_exact(&mut header).ok()?;
        let opcode = u32::from_le_bytes(header[0..4].try_into().ok()?);
        let length = u32::from_le_bytes(header[4..8].try_into().ok()?) as usize;
        let mut body = vec![0_u8; length];
        stream.read_exact(&mut body).ok()?;

        serde_json::from_slice(&body)
            .ok()
            .map(|json| (opcode, json))
    }

    fn write_frame(
        stream: &mut std::os::unix::net::UnixStream,
        opcode: u32,
        payload: &serde_json::Value,
    ) -> std::io::Result<()> {
        use std::io::Write;

        let body = payload.to_string();
        stream.write_all(&opcode.to_le_bytes())?;
        stream.write_all(&(body.len() as u32).to_le_bytes())?;
        stream.write_all(body.as_bytes())
    }

    #[test]
    fn presence_worker_publishes_retries_and_clears() {
        let dir = std::env::temp_dir().join(format!("gtunes-discord-ipc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create fake IPC directory");
        // SAFETY: the worker started below resolves the Discord socket path from
        // this variable, and no other test in this binary reads it.
        unsafe { std::env::set_var("XDG_RUNTIME_DIR", &dir) };

        let (frames, replies) = fake_ipc_server(&dir);
        let (sender, _commands) = mpsc::channel();
        let mut worker = PresenceWorker::with_artwork_cache(
            PresenceConfig {
                client_id: "test".to_string(),
                large_image_key: None,
                small_image_key: None,
            },
            sender,
            HashMap::new(),
        );

        let activity = PresenceActivity {
            title: "Song".to_string(),
            artist: "Artist".to_string(),
            album: Some("Album".to_string()),
            artwork_source_url: None,
            playback_state: PresencePlaybackState::Playing,
            position: Some(Duration::from_secs(10)),
            duration: Some(Duration::from_secs(200)),
        };

        replies
            .send(
                serde_json::json!({"cmd": "SET_ACTIVITY", "evt": "ERROR", "data": {"code": 1000}}),
            )
            .expect("queue a rejected update");

        worker.handle(PresenceCommand::Set(activity.clone()));

        let handshake = frames
            .recv_timeout(Duration::from_secs(5))
            .expect("handshake frame");
        assert_eq!(handshake["v"], 1);
        let published = frames
            .recv_timeout(Duration::from_secs(5))
            .expect("presence frame");
        assert_eq!(published["args"]["activity"]["details"], "Song");
        assert_eq!(published["args"]["activity"]["type"], 2);

        // Discord refused that update, so the next reconcile publishes it again
        // rather than remembering it as applied.
        worker.reconcile();
        let retried = frames
            .recv_timeout(Duration::from_secs(5))
            .expect("retried presence frame");
        assert_eq!(retried["args"]["activity"]["details"], "Song");

        // An accepted update is published once and then left alone, so the
        // elapsed time Discord shows is not restarted.
        worker.reconcile();
        assert!(
            frames.recv_timeout(Duration::from_millis(400)).is_err(),
            "an unchanged presence should not be republished"
        );

        // Pausing clears the presence, and the immediate reconcile stays quiet
        // until the clear needs re-asserting.
        worker.handle(PresenceCommand::Clear);
        let cleared = frames
            .recv_timeout(Duration::from_secs(5))
            .expect("clear frame");
        assert!(cleared["args"]["activity"].is_null());
        worker.reconcile();
        assert!(
            frames.recv_timeout(Duration::from_millis(400)).is_err(),
            "a fresh clear should not be repeated immediately"
        );

        worker.shutdown();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
