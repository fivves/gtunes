use base64::Engine as _;
use base64::engine::general_purpose;
use reqwest::StatusCode;
use ring::digest;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use url::Url;

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const SEARCH_URL: &str = "https://www.googleapis.com/youtube/v3/search";
const VIDEOS_URL: &str = "https://www.googleapis.com/youtube/v3/videos";
const YOUTUBE_SCOPE: &str = "https://www.googleapis.com/auth/youtube.readonly";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct YouTubeAuthSession {
    pub client_id: String,
    #[serde(default)]
    pub client_secret: Option<String>,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub token_type: String,
    pub scope: Option<String>,
    pub expires_at_epoch: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct YouTubeMusicTrack {
    pub video_id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: String,
    pub quality: String,
    pub thumbnail_url: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedYouTubeStream {
    pub stream_url: String,
    pub http_headers: Vec<(String, String)>,
}

pub struct PendingYouTubeAuth {
    pub auth_url: String,
    pub receiver: mpsc::Receiver<Result<YouTubeAuthSession, YouTubeError>>,
}

#[derive(Debug)]
pub enum YouTubeError {
    Http(String),
    Io(std::io::Error),
    OAuth(String),
    Parse(String),
    TokenExpired,
}

impl fmt::Display for YouTubeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http(message) => formatter.write_str(message),
            Self::Io(error) => write!(formatter, "io error: {error}"),
            Self::OAuth(message) => formatter.write_str(message),
            Self::Parse(message) => formatter.write_str(message),
            Self::TokenExpired => formatter.write_str("YouTube sign-in expired; sign in again"),
        }
    }
}

impl std::error::Error for YouTubeError {}

impl From<std::io::Error> for YouTubeError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Clone)]
pub struct YouTubeClient {
    http: reqwest::blocking::Client,
}

impl YouTubeClient {
    pub fn new() -> Result<Self, YouTubeError> {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("gtunes-youtube-music-prototype")
            .build()
            .map_err(|error| YouTubeError::Http(format!("request client failed: {error}")))?;
        Ok(Self { http })
    }

    pub fn begin_installed_auth(
        client_id: String,
        client_secret: String,
    ) -> Result<PendingYouTubeAuth, YouTubeError> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let redirect_uri = format!("http://{}", listener.local_addr()?);
        let state = random_token(48)?;
        let code_verifier = random_token(96)?;
        let code_challenge = code_challenge_s256(&code_verifier);
        let client_secret = Some(client_secret).filter(|secret| !secret.is_empty());
        let auth_url = Url::parse_with_params(
            AUTH_URL,
            &[
                ("client_id", client_id.as_str()),
                ("redirect_uri", redirect_uri.as_str()),
                ("response_type", "code"),
                ("scope", YOUTUBE_SCOPE),
                ("access_type", "offline"),
                ("prompt", "consent"),
                ("code_challenge", code_challenge.as_str()),
                ("code_challenge_method", "S256"),
                ("state", state.as_str()),
            ],
        )
        .map_err(|error| YouTubeError::Parse(format!("auth URL failed: {error}")))?
        .to_string();

        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let result = wait_for_auth_redirect(listener, &state).and_then(|code| {
                exchange_code(
                    &client_id,
                    client_secret.as_deref(),
                    &redirect_uri,
                    &code_verifier,
                    &code,
                )
            });
            let _ = sender.send(result);
        });

        Ok(PendingYouTubeAuth { auth_url, receiver })
    }

    pub fn search_music(
        &self,
        session: &mut YouTubeAuthSession,
        query: &str,
    ) -> Result<Vec<YouTubeMusicTrack>, YouTubeError> {
        self.refresh_if_needed(session)?;
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }

        let search_url = url_with_params(
            SEARCH_URL,
            &[
                ("part", "snippet"),
                ("type", "video"),
                ("videoCategoryId", "10"),
                ("maxResults", "25"),
                ("q", query),
                (
                    "fields",
                    "items(id/videoId,snippet/title,snippet/channelTitle,snippet/thumbnails/default/url,snippet/thumbnails/medium/url,snippet/thumbnails/high/url)",
                ),
            ],
        )?;
        let response = self
            .http
            .get(search_url)
            .bearer_auth(&session.access_token)
            .send()
            .map_err(|error| YouTubeError::Http(format!("YouTube search failed: {error}")))?;
        let status = response.status();
        let body = response
            .text()
            .map_err(|error| YouTubeError::Http(format!("YouTube search body failed: {error}")))?;
        if !status.is_success() {
            return Err(YouTubeError::Http(api_error_message(status, &body)));
        }

        let search: SearchResponse = serde_json::from_str(&body)
            .map_err(|error| YouTubeError::Parse(format!("YouTube search JSON failed: {error}")))?;
        let ids = search
            .items
            .iter()
            .filter_map(|item| item.id.video_id.as_deref())
            .collect::<Vec<_>>();
        let metadata = self.video_metadata(session, &ids)?;

        Ok(search
            .items
            .into_iter()
            .filter_map(|item| {
                let video_id = item.id.video_id?;
                let video_metadata = metadata.get(&video_id);
                let title = video_metadata
                    .map(|metadata| metadata.title.as_str())
                    .unwrap_or(item.snippet.title.as_str());
                let channel_title = video_metadata
                    .map(|metadata| metadata.channel_title.as_str())
                    .unwrap_or(item.snippet.channel_title.as_str());
                let description = video_metadata
                    .map(|metadata| metadata.description.as_str())
                    .unwrap_or_default();
                let parsed = parse_music_metadata(title, channel_title, description);
                let duration = video_metadata
                    .map(|metadata| metadata.duration.clone())
                    .unwrap_or_else(|| "--:--".to_string());
                Some(YouTubeMusicTrack {
                    video_id,
                    title: parsed.title,
                    artist: parsed.artist,
                    album: parsed.album,
                    duration,
                    quality: "Premium".to_string(),
                    thumbnail_url: item.snippet.best_thumbnail_url(),
                })
            })
            .collect())
    }

    pub fn resolve_music_stream(video_id: &str) -> Result<ResolvedYouTubeStream, YouTubeError> {
        let url = format!("https://music.youtube.com/watch?v={video_id}");
        let mut last_error = "yt-dlp could not resolve this YouTube Music stream".to_string();

        for cookie_browser in [
            None,
            Some("firefox"),
            Some("chrome"),
            Some("chromium"),
            Some("brave"),
        ] {
            match resolve_music_stream_with_ytdlp(&url, cookie_browser) {
                Ok(stream_url) => {
                    return Ok(ResolvedYouTubeStream {
                        stream_url,
                        http_headers: youtube_stream_headers(),
                    });
                }
                Err(error) => last_error = error,
            }
        }

        Err(YouTubeError::Http(last_error))
    }

    fn refresh_if_needed(&self, session: &mut YouTubeAuthSession) -> Result<(), YouTubeError> {
        let now = current_epoch_seconds();
        if session.expires_at_epoch > now + 60 {
            return Ok(());
        }
        let Some(refresh_token) = session.refresh_token.as_deref() else {
            return Err(YouTubeError::TokenExpired);
        };

        let mut params = vec![
            ("client_id", session.client_id.as_str()),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ];
        if let Some(client_secret) = session
            .client_secret
            .as_deref()
            .filter(|secret| !secret.is_empty())
        {
            params.push(("client_secret", client_secret));
        }
        let response = self
            .http
            .post(TOKEN_URL)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(encoded_form(&params))
            .send()
            .map_err(|error| YouTubeError::Http(format!("token refresh failed: {error}")))?;
        let status = response.status();
        let body = response
            .text()
            .map_err(|error| YouTubeError::Http(format!("token refresh body failed: {error}")))?;
        if !status.is_success() {
            return Err(YouTubeError::OAuth(api_error_message(status, &body)));
        }
        let token: TokenResponse = serde_json::from_str(&body)
            .map_err(|error| YouTubeError::Parse(format!("token refresh JSON failed: {error}")))?;
        session.access_token = token.access_token;
        session.token_type = token.token_type.unwrap_or_else(|| "Bearer".to_string());
        session.scope = token.scope;
        session.expires_at_epoch = current_epoch_seconds() + token.expires_in.unwrap_or(3600);
        Ok(())
    }

    fn video_metadata(
        &self,
        session: &YouTubeAuthSession,
        video_ids: &[&str],
    ) -> Result<HashMap<String, YouTubeVideoMetadata>, YouTubeError> {
        if video_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let ids = video_ids.join(",");
        let videos_url = url_with_params(
            VIDEOS_URL,
            &[
                ("part", "snippet,contentDetails"),
                ("id", &ids),
                (
                    "fields",
                    "items(id,snippet/title,snippet/channelTitle,snippet/description,contentDetails/duration)",
                ),
            ],
        )?;
        let response = self
            .http
            .get(videos_url)
            .bearer_auth(&session.access_token)
            .send()
            .map_err(|error| YouTubeError::Http(format!("video details failed: {error}")))?;
        let status = response.status();
        let body = response
            .text()
            .map_err(|error| YouTubeError::Http(format!("video details body failed: {error}")))?;
        if !status.is_success() {
            return Err(YouTubeError::Http(api_error_message(status, &body)));
        }

        let response: VideosResponse = serde_json::from_str(&body)
            .map_err(|error| YouTubeError::Parse(format!("video details JSON failed: {error}")))?;
        Ok(response
            .items
            .into_iter()
            .map(|item| {
                (
                    item.id,
                    YouTubeVideoMetadata {
                        title: decode_html_entities(&item.snippet.title),
                        channel_title: decode_html_entities(&item.snippet.channel_title),
                        description: decode_html_entities(&item.snippet.description),
                        duration: format_iso8601_duration(&item.content_details.duration),
                    },
                )
            })
            .collect())
    }
}

fn resolve_music_stream_with_ytdlp(
    url: &str,
    cookie_browser: Option<&str>,
) -> Result<String, String> {
    let mut command = Command::new("yt-dlp");
    command.args([
        "--no-playlist",
        "--no-warnings",
        "--quiet",
        "--format",
        "bestaudio/best",
        "--extractor-args",
        "youtube:player_client=web_music,web",
        "--get-url",
    ]);
    if let Some(browser) = cookie_browser {
        command.args(["--cookies-from-browser", browser]);
    }
    command.arg(url);

    let output = command
        .output()
        .map_err(|error| format!("yt-dlp failed to start: {error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if output.status.success()
        && let Some(stream_url) = stdout.lines().find(|line| line.starts_with("http"))
    {
        return Ok(stream_url.trim().to_string());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let context = cookie_browser
        .map(|browser| format!(" with {browser} cookies"))
        .unwrap_or_default();
    let detail = stderr
        .lines()
        .find(|line| !line.trim().is_empty())
        .or_else(|| stdout.lines().find(|line| !line.trim().is_empty()))
        .unwrap_or("no stream URL returned");
    Err(format!("yt-dlp{context}: {detail}"))
}

fn youtube_stream_headers() -> Vec<(String, String)> {
    vec![
        (
            "User-Agent".to_string(),
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0 Safari/537.36".to_string(),
        ),
        ("Referer".to_string(), "https://music.youtube.com/".to_string()),
    ]
}

fn wait_for_auth_redirect(
    listener: TcpListener,
    expected_state: &str,
) -> Result<String, YouTubeError> {
    listener.set_nonblocking(true)?;
    let deadline = Instant::now() + Duration::from_secs(600);
    let (mut stream, _) = loop {
        match listener.accept() {
            Ok(connection) => break connection,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(YouTubeError::OAuth(
                        "Google sign-in timed out; try again".to_string(),
                    ));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(error) => return Err(YouTubeError::Io(error)),
        }
    };
    let mut buffer = [0; 8192];
    let read = stream.read(&mut buffer)?;
    let request = String::from_utf8_lossy(&buffer[..read]);
    let request_line = request
        .lines()
        .next()
        .ok_or_else(|| YouTubeError::Parse("OAuth redirect request was empty".to_string()))?;
    let target = request_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| YouTubeError::Parse("OAuth redirect target was missing".to_string()))?;
    let redirect_url = Url::parse(&format!("http://127.0.0.1{target}"))
        .map_err(|error| YouTubeError::Parse(format!("OAuth redirect URL failed: {error}")))?;
    let params = redirect_url
        .query_pairs()
        .into_owned()
        .collect::<HashMap<String, String>>();

    let response_message = if params.get("state").map(String::as_str) != Some(expected_state) {
        Err(YouTubeError::OAuth(
            "OAuth state mismatch; sign-in was rejected".to_string(),
        ))
    } else if let Some(error) = params.get("error") {
        Err(YouTubeError::OAuth(format!(
            "Google rejected sign-in: {error}"
        )))
    } else {
        params
            .get("code")
            .cloned()
            .ok_or_else(|| YouTubeError::OAuth("Google did not return an auth code".to_string()))
    };

    let html = match &response_message {
        Ok(_) => {
            "<!doctype html><title>gTunes YouTube Music</title><h1>gTunes authorization received</h1><p>Return to gTunes to finish sign-in.</p>"
        }
        Err(_) => {
            "<!doctype html><title>gTunes YouTube Music</title><h1>gTunes sign-in failed</h1><p>Return to gTunes and try again.</p>"
        }
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        html.len(),
        html
    );
    let _ = stream.write_all(response.as_bytes());

    response_message
}

fn exchange_code(
    client_id: &str,
    client_secret: Option<&str>,
    redirect_uri: &str,
    code_verifier: &str,
    code: &str,
) -> Result<YouTubeAuthSession, YouTubeError> {
    let http = YouTubeClient::new()?.http;
    let mut params = vec![
        ("client_id", client_id),
        ("code", code),
        ("code_verifier", code_verifier),
        ("grant_type", "authorization_code"),
        ("redirect_uri", redirect_uri),
    ];
    if let Some(client_secret) = client_secret.filter(|secret| !secret.is_empty()) {
        params.push(("client_secret", client_secret));
    }
    let response = http
        .post(TOKEN_URL)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(encoded_form(&params))
        .send()
        .map_err(|error| YouTubeError::Http(format!("token exchange failed: {error}")))?;
    let status = response.status();
    let body = response
        .text()
        .map_err(|error| YouTubeError::Http(format!("token exchange body failed: {error}")))?;
    if !status.is_success() {
        return Err(YouTubeError::OAuth(api_error_message(status, &body)));
    }

    let token: TokenResponse = serde_json::from_str(&body)
        .map_err(|error| YouTubeError::Parse(format!("token exchange JSON failed: {error}")))?;
    Ok(YouTubeAuthSession {
        client_id: client_id.to_string(),
        client_secret: client_secret.map(str::to_string),
        access_token: token.access_token,
        refresh_token: token.refresh_token,
        token_type: token.token_type.unwrap_or_else(|| "Bearer".to_string()),
        scope: token.scope,
        expires_at_epoch: current_epoch_seconds() + token.expires_in.unwrap_or(3600),
    })
}

fn random_token(length: usize) -> Result<String, YouTubeError> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
    let mut bytes = vec![0; length];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes
        .into_iter()
        .map(|byte| ALPHABET[byte as usize % ALPHABET.len()] as char)
        .collect())
}

fn code_challenge_s256(code_verifier: &str) -> String {
    let digest = digest::digest(&digest::SHA256, code_verifier.as_bytes());
    general_purpose::URL_SAFE_NO_PAD.encode(digest.as_ref())
}

fn url_with_params(base: &str, params: &[(&str, &str)]) -> Result<Url, YouTubeError> {
    let mut url =
        Url::parse(base).map_err(|error| YouTubeError::Parse(format!("URL failed: {error}")))?;
    url.query_pairs_mut().extend_pairs(params.iter().copied());
    Ok(url)
}

fn encoded_form(params: &[(&str, &str)]) -> String {
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (key, value) in params {
        serializer.append_pair(key, value);
    }
    serializer.finish()
}

fn current_epoch_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn api_error_message(status: StatusCode, body: &str) -> String {
    if let Ok(error) = serde_json::from_str::<OAuthErrorResponse>(body) {
        return match error.error_description {
            Some(description) if !description.is_empty() => {
                format!(
                    "Google OAuth error {status}: {} - {description}",
                    error.error
                )
            }
            _ => format!("Google OAuth error {status}: {}", error.error),
        };
    }

    if let Ok(error) = serde_json::from_str::<ApiErrorResponse>(body)
        && let Some(message) = error.error.message
    {
        return format!("YouTube API error {status}: {message}");
    }
    format!("YouTube API error {status}")
}

fn decode_html_entities(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ParsedMusicMetadata {
    title: String,
    artist: String,
    album: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct YouTubeVideoMetadata {
    title: String,
    channel_title: String,
    description: String,
    duration: String,
}

fn parse_music_metadata(
    raw_title: &str,
    raw_channel_title: &str,
    raw_description: &str,
) -> ParsedMusicMetadata {
    let fallback_title = clean_video_title(&decode_html_entities(raw_title));
    let fallback_artist = clean_channel_artist(&decode_html_entities(raw_channel_title))
        .unwrap_or_else(|| "Unknown Artist".to_string());

    if let Some(parsed) = parse_provided_to_youtube_description(raw_description) {
        return ParsedMusicMetadata {
            title: clean_video_title(&parsed.title),
            artist: parsed.artist,
            album: parsed.album,
        };
    }

    if let Some((title, artist)) = split_title_artist_dot(&fallback_title) {
        return ParsedMusicMetadata {
            title: clean_video_title(&title),
            artist,
            album: "Unknown Album".to_string(),
        };
    }

    if let Some((title, artist)) = split_dash_title(&fallback_title, &fallback_artist) {
        return ParsedMusicMetadata {
            title: clean_video_title(&title),
            artist,
            album: "Unknown Album".to_string(),
        };
    }

    ParsedMusicMetadata {
        title: fallback_title,
        artist: fallback_artist,
        album: "Unknown Album".to_string(),
    }
}

fn parse_provided_to_youtube_description(description: &str) -> Option<ParsedMusicMetadata> {
    let lines = description
        .lines()
        .map(clean_metadata_line)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let start = lines
        .iter()
        .position(|line| line.to_lowercase().starts_with("provided to youtube by"))?;
    let metadata_lines = &lines[start + 1..];
    let (title_index, title, artist) =
        metadata_lines
            .iter()
            .enumerate()
            .find_map(|(index, line)| {
                split_title_artist_dot(line).map(|(title, artist)| (index, title, artist))
            })?;
    let album = metadata_lines
        .iter()
        .skip(title_index + 1)
        .find(|line| is_album_description_line(line))
        .cloned()
        .unwrap_or_else(|| "Unknown Album".to_string());

    Some(ParsedMusicMetadata {
        title,
        artist,
        album,
    })
}

fn split_title_artist_dot(value: &str) -> Option<(String, String)> {
    let separator = "\u{00b7}";
    let (title, artist) = value.split_once(separator)?;
    let title = clean_metadata_line(title);
    let artist = clean_metadata_line(artist);
    (!title.is_empty() && !artist.is_empty()).then_some((title, artist))
}

fn split_dash_title(value: &str, fallback_artist: &str) -> Option<(String, String)> {
    for separator in [" - ", " \u{2013} ", " \u{2014} "] {
        let Some((left, right)) = value.split_once(separator) else {
            continue;
        };
        let left = clean_metadata_line(left);
        let right = clean_metadata_line(right);
        if left.is_empty() || right.is_empty() {
            continue;
        }

        if !fallback_artist.eq_ignore_ascii_case("unknown artist") {
            if normalized_name(&right) == normalized_name(fallback_artist) {
                return Some((left, fallback_artist.to_string()));
            }
            if normalized_name(&left) == normalized_name(fallback_artist) {
                return Some((right, fallback_artist.to_string()));
            }
        }

        return Some((right, left));
    }

    None
}

fn clean_video_title(value: &str) -> String {
    let mut title = clean_metadata_line(value);
    loop {
        let trimmed = strip_one_trailing_qualifier(&title);
        if trimmed == title {
            break;
        }
        title = trimmed;
    }
    title
        .trim_matches('"')
        .trim_matches('\'')
        .trim()
        .to_string()
}

fn strip_one_trailing_qualifier(value: &str) -> String {
    let Some(close) = value.chars().last() else {
        return value.to_string();
    };
    let open = match close {
        ')' => '(',
        ']' => '[',
        _ => return value.to_string(),
    };
    let Some(open_index) = value.rfind(open) else {
        return value.to_string();
    };
    let qualifier = value[open_index + 1..value.len() - close.len_utf8()].to_lowercase();
    if [
        "official",
        "official audio",
        "official music video",
        "official video",
        "audio",
        "music video",
        "lyric video",
        "lyrics",
        "visualizer",
        "remastered",
        "remaster",
        "hd",
        "4k",
    ]
    .iter()
    .any(|term| qualifier == *term || qualifier.contains(term))
    {
        value[..open_index].trim().to_string()
    } else {
        value.to_string()
    }
}

fn clean_channel_artist(value: &str) -> Option<String> {
    let mut artist = clean_metadata_line(value);
    for suffix in [
        " - Topic",
        " Topic",
        " - YouTube",
        " Official",
        " Official Artist Channel",
    ] {
        if artist.ends_with(suffix) {
            artist.truncate(artist.len() - suffix.len());
            artist = clean_metadata_line(&artist);
        }
    }
    if artist.ends_with("VEVO") && artist.len() > 4 {
        artist.truncate(artist.len() - 4);
        artist = clean_metadata_line(&artist);
    }
    (!artist.is_empty()).then_some(artist)
}

fn is_album_description_line(line: &str) -> bool {
    let lower = line.to_lowercase();
    !line.starts_with('\u{2117}')
        && !lower.starts_with("released on:")
        && !lower.starts_with("auto-generated by youtube")
        && !lower.contains("provided to youtube by")
        && !lower.starts_with("composer")
        && !lower.starts_with("producer")
        && !lower.starts_with("lyricist")
}

fn clean_metadata_line(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .trim_matches('-')
        .trim()
        .to_string()
}

fn normalized_name(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .filter(|character| character.is_alphanumeric())
        .collect()
}

fn format_iso8601_duration(value: &str) -> String {
    let Some(rest) = value.strip_prefix("PT") else {
        return "--:--".to_string();
    };
    let mut hours = 0;
    let mut minutes = 0;
    let mut seconds = 0;
    let mut number = String::new();
    for character in rest.chars() {
        if character.is_ascii_digit() {
            number.push(character);
            continue;
        }
        let parsed = number.parse::<u64>().unwrap_or(0);
        number.clear();
        match character {
            'H' => hours = parsed,
            'M' => minutes = parsed,
            'S' => seconds = parsed,
            _ => {}
        }
    }

    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    token_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    items: Vec<SearchItem>,
}

#[derive(Debug, Deserialize)]
struct SearchItem {
    id: SearchItemId,
    snippet: SearchSnippet,
}

#[derive(Debug, Deserialize)]
struct SearchItemId {
    #[serde(rename = "videoId")]
    video_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SearchSnippet {
    title: String,
    #[serde(rename = "channelTitle")]
    channel_title: String,
    #[serde(default)]
    thumbnails: HashMap<String, SearchThumbnail>,
}

impl SearchSnippet {
    fn best_thumbnail_url(&self) -> Option<String> {
        self.thumbnails
            .get("high")
            .or_else(|| self.thumbnails.get("medium"))
            .or_else(|| self.thumbnails.get("default"))
            .map(|thumbnail| thumbnail.url.clone())
    }
}

#[derive(Debug, Deserialize)]
struct SearchThumbnail {
    url: String,
}

#[derive(Debug, Deserialize)]
struct VideosResponse {
    #[serde(default)]
    items: Vec<VideoItem>,
}

#[derive(Debug, Deserialize)]
struct VideoItem {
    id: String,
    snippet: VideoSnippet,
    #[serde(rename = "contentDetails")]
    content_details: VideoContentDetails,
}

#[derive(Debug, Deserialize)]
struct VideoSnippet {
    title: String,
    #[serde(rename = "channelTitle")]
    channel_title: String,
    #[serde(default)]
    description: String,
}

#[derive(Debug, Deserialize)]
struct VideoContentDetails {
    duration: String,
}

#[derive(Debug, Deserialize)]
struct ApiErrorResponse {
    error: ApiError,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OAuthErrorResponse {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_provided_to_youtube_metadata_with_album() {
        let metadata = parse_music_metadata(
            "Sweet Child O' Mine",
            "Guns N' Roses - Topic",
            "Provided to YouTube by Universal Music Group\n\nSweet Child O' Mine \u{00b7} Guns N' Roses\n\nAppetite For Destruction\n\n\u{2117} 1987 UMG Recordings\n\nReleased on: 1987-01-01\n\nAuto-generated by YouTube.",
        );

        assert_eq!(metadata.title, "Sweet Child O' Mine");
        assert_eq!(metadata.artist, "Guns N' Roses");
        assert_eq!(metadata.album, "Appetite For Destruction");
    }

    #[test]
    fn parses_artist_dash_song_title() {
        let metadata = parse_music_metadata(
            "Chappell Roan - Good Luck, Babe! (Official Lyric Video)",
            "Chappell Roan",
            "",
        );

        assert_eq!(metadata.title, "Good Luck, Babe!");
        assert_eq!(metadata.artist, "Chappell Roan");
        assert_eq!(metadata.album, "Unknown Album");
    }

    #[test]
    fn parses_song_artist_dot_title() {
        let metadata = parse_music_metadata(
            "Blinding Lights \u{00b7} The Weeknd",
            "The Weeknd - Topic",
            "",
        );

        assert_eq!(metadata.title, "Blinding Lights");
        assert_eq!(metadata.artist, "The Weeknd");
        assert_eq!(metadata.album, "Unknown Album");
    }

    #[test]
    fn cleans_topic_channel_when_title_is_already_song() {
        let metadata = parse_music_metadata("Bad Habit", "Steve Lacy - Topic", "");

        assert_eq!(metadata.title, "Bad Habit");
        assert_eq!(metadata.artist, "Steve Lacy");
        assert_eq!(metadata.album, "Unknown Album");
    }
}
