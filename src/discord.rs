use discord_rich_presence::{
    DiscordIpc, DiscordIpcClient,
    activity::{self, Activity},
};
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::env;
use std::hash::{Hash, Hasher};
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PresencePlaybackState {
    Playing,
    Paused,
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
        generation: u64,
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
    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);
    format!("{:x}", hasher.finish())
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
    let mut client = DiscordIpcClient::new(&config.client_id);
    let mut connected = false;
    let mut next_connect_attempt = Instant::now();
    let mut artwork_cache = load_persisted_artwork_cache();
    let mut uploading_artwork = HashSet::<String>::new();
    let mut current_generation = 0_u64;
    let mut current_activity = None::<PresenceActivity>;

    while let Ok(command) = receiver.recv() {
        match command {
            PresenceCommand::Set(activity) => {
                current_generation = current_generation.saturating_add(1);
                current_activity = Some(activity.clone());

                if ensure_discord_connected(&mut client, &mut connected, &mut next_connect_attempt)
                {
                    let activity_with_cached_art = activity.with_cached_artwork(&artwork_cache);
                    if let Err(error) =
                        client.set_activity(discord_activity(&activity_with_cached_art, &config))
                    {
                        tracing::debug!(%error, "failed to update Discord Rich Presence");
                        connected = false;
                        let _ = client.close();
                        next_connect_attempt = Instant::now() + DISCORD_RETRY_INTERVAL;
                    }
                }

                queue_artwork_upload(
                    &activity,
                    current_generation,
                    &artwork_cache,
                    &mut uploading_artwork,
                    &sender,
                );
            }
            PresenceCommand::ArtworkUploaded {
                generation,
                source_url,
                public_url,
            } => {
                let cache_id = artwork_cache_id(&source_url);
                uploading_artwork.remove(&cache_id);
                artwork_cache.insert(cache_id, public_url.clone());
                persist_artwork_url(&source_url, &public_url);

                if generation == current_generation
                    && current_activity
                        .as_ref()
                        .and_then(|activity| activity.artwork_source_url.as_deref())
                        == Some(source_url.as_str())
                    && ensure_discord_connected(
                        &mut client,
                        &mut connected,
                        &mut next_connect_attempt,
                    )
                    && let Some(activity) = current_activity.as_ref()
                {
                    let activity_with_art = activity.with_public_artwork(public_url);
                    if let Err(error) =
                        client.set_activity(discord_activity(&activity_with_art, &config))
                    {
                        tracing::debug!(%error, "failed to update Discord artwork");
                        connected = false;
                        let _ = client.close();
                        next_connect_attempt = Instant::now() + DISCORD_RETRY_INTERVAL;
                    }
                }
            }
            PresenceCommand::ArtworkUploadFailed { source_url } => {
                uploading_artwork.remove(&artwork_cache_id(&source_url));
            }
            PresenceCommand::Clear => {
                current_generation = current_generation.saturating_add(1);
                current_activity = None;
                if ensure_discord_connected(&mut client, &mut connected, &mut next_connect_attempt)
                    && let Err(error) = client.clear_activity()
                {
                    tracing::debug!(%error, "failed to clear Discord Rich Presence");
                    connected = false;
                    let _ = client.close();
                    next_connect_attempt = Instant::now() + DISCORD_RETRY_INTERVAL;
                }
            }
            PresenceCommand::Shutdown => {
                let _ = client.clear_activity();
                let _ = client.close();
                return;
            }
        }
    }

    if connected {
        let _ = client.clear_activity();
        let _ = client.close();
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

fn ensure_discord_connected(
    client: &mut DiscordIpcClient,
    connected: &mut bool,
    next_connect_attempt: &mut Instant,
) -> bool {
    if *connected {
        return true;
    }

    let now = Instant::now();
    if now < *next_connect_attempt {
        return false;
    }

    match client.connect() {
        Ok(()) => {
            *connected = true;
            true
        }
        Err(error) => {
            tracing::debug!(%error, "failed to connect to Discord Rich Presence");
            *next_connect_attempt = now + DISCORD_RETRY_INTERVAL;
            false
        }
    }
}

fn queue_artwork_upload(
    activity: &PresenceActivity,
    generation: u64,
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

    let path = artwork_cache_path(&source_url);
    if !path.exists() {
        return;
    }

    let upload_cache_id = cache_id.clone();
    uploading.insert(cache_id);
    let sender = sender.clone();
    let spawn_result = std::thread::Builder::new()
        .name("gtunes-discord-artwork-upload".to_string())
        .spawn(move || match upload_artwork_file(&path) {
            Ok(public_url) => {
                let _ = sender.send(PresenceCommand::ArtworkUploaded {
                    generation,
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

fn upload_artwork_file(path: &PathBuf) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
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
    let large_image = presence
        .artwork_source_url
        .as_deref()
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
    fn artwork_cache_path_uses_hashed_source_url() {
        let path =
            artwork_cache_path("https://jellyfin.example/Items/1/Images/Primary?api_key=secret");

        assert!(path.starts_with(std::env::temp_dir()));
        assert!(!path.to_string_lossy().contains("secret"));
    }
}
