pub(crate) mod session;

use gst::prelude::*;
use std::collections::VecDeque;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long the reported position may stand still on a PLAYING pipeline before
/// the stream is treated as stalled.
const STALL_FROZEN_AFTER: Duration = Duration::from_millis(2_500);
/// How long a stalled pipeline stays paused before the source is poked again.
/// Only used when the source never posts a closing buffering message.
const STALL_RETRY_AFTER: Duration = Duration::from_millis(4_000);
/// A healthy source still flaps between "full" and "nearly full" (a LAN stream
/// reported 70% and back to 100% inside a millisecond), and the pipeline is
/// paused and resumed for each of those. Holding the audio is the right call,
/// but the UI must not blink for it, so the spinner and the status line only
/// appear once a stall has lasted this long.
const STALL_UI_DELAY: Duration = Duration::from_millis(400);
use thiserror::Error;
use url::Url;

const YOUTUBE_AUDIO_FORMAT: &str =
    "bestaudio[acodec!=none][vcodec=none]/bestaudio[acodec!=none]/best[acodec!=none]";

#[derive(Debug, Error)]
pub enum PlaybackError {
    #[error("GStreamer playbin element is unavailable")]
    PlaybinUnavailable,
    #[error("GStreamer state change failed: {0}")]
    StateChange(#[from] gst::StateChangeError),
    #[error("GStreamer seek failed: {0}")]
    Seek(#[from] gst::glib::BoolError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalStreamSource {
    YouTube,
    Twitch,
}

#[derive(Debug, Error)]
pub enum ExternalStreamError {
    #[error("{program} is required to resolve this stream")]
    MissingResolver {
        program: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("{program} failed with status {status}: {stderr}")]
    ResolverFailed {
        program: &'static str,
        status: std::process::ExitStatus,
        stderr: String,
    },
    #[error("{program} did not return a stream URL")]
    EmptyResolverOutput { program: &'static str },
    #[error("resolver returned an invalid stream URL: {0}")]
    InvalidResolvedUrl(#[from] url::ParseError),
}

#[derive(Clone, Debug)]
pub struct PlaybackRequest {
    pub item_id: String,
    pub stream_url: Url,
    pub http_headers: Vec<(String, String)>,
    pub stream_kind: PlaybackStreamKind,
    pub title: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlaybackStreamKind {
    Direct,
    Transcode,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlaybackEvent {
    EndOfStream,
    Error {
        item_id: Option<String>,
        stream_kind: Option<PlaybackStreamKind>,
        message: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlaybackState {
    Stopped,
    Playing,
    Paused,
    Error(String),
}

pub fn resolve_external_stream_url(
    source: ExternalStreamSource,
    page_url: &Url,
) -> Result<Url, ExternalStreamError> {
    match source {
        ExternalStreamSource::YouTube => resolve_stream_with_command(
            "yt-dlp",
            &[
                "--no-playlist",
                "--quiet",
                "--no-warnings",
                "--format",
                YOUTUBE_AUDIO_FORMAT,
                "--get-url",
                page_url.as_str(),
            ],
        ),
        ExternalStreamSource::Twitch => resolve_stream_with_command(
            "streamlink",
            &["--stream-url", page_url.as_str(), "audio_only"],
        ),
    }
}

fn resolve_stream_with_command(
    program: &'static str,
    args: &[&str],
) -> Result<Url, ExternalStreamError> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|source| ExternalStreamError::MissingResolver { program, source })?;

    if !output.status.success() {
        return Err(ExternalStreamError::ResolverFailed {
            program,
            status: output.status,
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stream_url = stdout
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .ok_or(ExternalStreamError::EmptyResolverOutput { program })?;
    Ok(Url::parse(stream_url)?)
}

#[derive(Debug)]
pub struct PlaybackEngine {
    playbin: gst::Element,
    bus: gst::Bus,
    gapless: Arc<Mutex<GaplessState>>,
    http_headers: Arc<Mutex<Vec<(String, String)>>>,
    state: PlaybackState,
    current_item_id: Option<String>,
    current_stream_kind: Option<PlaybackStreamKind>,
    is_buffering: bool,
    /// The last buffering percentage the source reported, if any.
    buffering_percent: Option<u8>,
    /// We paused the pipeline ourselves because the source ran dry while the
    /// user's intent is still "playing". The pipeline must stay paused until
    /// data is flowing again, otherwise the sink renders silence and the
    /// reported position walks ahead of the audio.
    stalled: bool,
    stalled_since: Option<Instant>,
    /// Position seen on the last watchdog tick, and since when it has not moved.
    last_position: Option<Duration>,
    frozen_since: Option<Instant>,
}

#[derive(Debug, Default)]
struct GaplessState {
    pending_next: Option<PlaybackRequest>,
    transitions: VecDeque<PlaybackRequest>,
}

impl PlaybackEngine {
    pub fn new() -> Result<Self, PlaybackError> {
        let playbin = gst::ElementFactory::make("playbin")
            .build()
            .map_err(|_| PlaybackError::PlaybinUnavailable)?;
        let video_sink = gst::ElementFactory::make("fakesink")
            .property("sync", false)
            .build()
            .map_err(|_| PlaybackError::PlaybinUnavailable)?;
        playbin.set_property("video-sink", &video_sink);
        let bus = playbin.bus().ok_or(PlaybackError::PlaybinUnavailable)?;
        let gapless = Arc::new(Mutex::new(GaplessState::default()));
        let http_headers = Arc::new(Mutex::new(Vec::new()));
        let http_headers_for_signal = http_headers.clone();
        playbin.connect("source-setup", false, move |values| {
            let source = values
                .get(1)
                .and_then(|value| value.get::<gst::Element>().ok())?;
            let headers = http_headers_for_signal
                .lock()
                .map(|headers| headers.clone())
                .unwrap_or_default();
            configure_http_source_headers(&source, &headers);
            None
        });

        let gapless_for_signal = gapless.clone();
        let http_headers_for_gapless = http_headers.clone();
        playbin.connect("about-to-finish", false, move |values| {
            let playbin = values
                .first()
                .and_then(|value| value.get::<gst::Element>().ok())?;
            let request = gapless_for_signal
                .lock()
                .ok()
                .and_then(|mut gapless| gapless.pending_next.take())?;

            tracing::info!(
                item_id = %request.item_id,
                title = %request.title,
                "queueing gapless Jellyfin stream"
            );
            if let Ok(mut headers) = http_headers_for_gapless.lock() {
                *headers = request.http_headers.clone();
            }
            playbin.set_property("uri", request.stream_url.as_str());
            if let Ok(mut gapless) = gapless_for_signal.lock() {
                gapless.transitions.push_back(request);
            }
            None
        });

        Ok(Self {
            playbin,
            bus,
            gapless,
            http_headers,
            state: PlaybackState::Stopped,
            current_item_id: None,
            current_stream_kind: None,
            is_buffering: false,
            buffering_percent: None,
            stalled: false,
            stalled_since: None,
            last_position: None,
            frozen_since: None,
        })
    }

    pub fn state(&self) -> &PlaybackState {
        &self.state
    }

    /// Whether the UI should show a loading state. A stall that clears within
    /// `STALL_UI_DELAY` is a source flap, not something to flash at the user.
    pub fn is_buffering(&self) -> bool {
        if self.stalled {
            return self
                .stalled_since
                .is_some_and(|since| since.elapsed() >= STALL_UI_DELAY);
        }
        self.is_buffering
    }

    /// True while the pipeline is being held paused because the source ran dry.
    pub fn is_stalled(&self) -> bool {
        self.stalled
    }

    pub fn buffering_percent(&self) -> Option<u8> {
        self.buffering_percent
    }

    /// Drops the initial-loading flag once audio is flowing. A stall keeps the
    /// flag: the spinner is how the app says "waiting", not "playing".
    pub fn clear_initial_loading(&mut self) {
        if self.stalled {
            return;
        }
        self.is_buffering = false;
    }

    pub fn current_stream_kind(&self) -> Option<PlaybackStreamKind> {
        self.current_stream_kind
    }

    pub fn play(&mut self, request: PlaybackRequest) -> Result<(), PlaybackError> {
        tracing::info!(
            item_id = %request.item_id,
            title = %request.title,
            "starting Jellyfin stream"
        );
        self.playbin.set_state(gst::State::Null)?;
        self.clear_gapless_state();
        self.current_item_id = None;
        self.current_stream_kind = None;
        self.state = PlaybackState::Stopped;
        self.is_buffering = true;
        self.buffering_percent = None;
        self.stalled = false;
        self.stalled_since = None;
        self.last_position = None;
        self.frozen_since = None;
        self.set_http_headers(request.http_headers.clone());
        self.playbin
            .set_property("uri", request.stream_url.as_str());
        self.playbin.set_state(gst::State::Playing)?;
        self.current_item_id = Some(request.item_id);
        self.current_stream_kind = Some(request.stream_kind);
        self.state = PlaybackState::Playing;
        Ok(())
    }

    pub fn set_next(&mut self, request: Option<PlaybackRequest>) {
        if let Ok(mut gapless) = self.gapless.lock() {
            gapless.pending_next = request;
        }
    }

    pub fn take_gapless_transition(&mut self) -> Option<PlaybackRequest> {
        let request = self
            .gapless
            .lock()
            .ok()
            .and_then(|mut gapless| gapless.transitions.pop_front())?;
        self.current_item_id = Some(request.item_id.clone());
        self.current_stream_kind = Some(request.stream_kind);
        self.state = PlaybackState::Playing;
        self.is_buffering = true;
        Some(request)
    }

    pub fn resume(&mut self) -> Result<(), PlaybackError> {
        self.state = PlaybackState::Playing;
        if self.stalled {
            // Still dry: stay paused and let the next buffering message (or the
            // watchdog's retry) start the stream, so play does not restart the
            // clock ahead of the audio.
            self.is_buffering = true;
            return Ok(());
        }
        self.playbin.set_state(gst::State::Playing)?;
        Ok(())
    }

    pub fn pause(&mut self) -> Result<(), PlaybackError> {
        self.playbin.set_state(gst::State::Paused)?;
        self.state = PlaybackState::Paused;
        self.is_buffering = false;
        Ok(())
    }

    pub fn pause_live_stream(&mut self) -> Result<(), PlaybackError> {
        self.playbin.set_state(gst::State::Null)?;
        self.clear_gapless_state();
        self.state = PlaybackState::Paused;
        Ok(())
    }

    pub fn position(&self) -> Option<Duration> {
        self.playbin
            .query_position::<gst::ClockTime>()
            .map(|time| Duration::from_nanos(time.nseconds()))
    }

    pub fn duration(&self) -> Option<Duration> {
        self.playbin
            .query_duration::<gst::ClockTime>()
            .map(|time| Duration::from_nanos(time.nseconds()))
    }

    pub fn seek(&mut self, position: Duration) -> Result<(), PlaybackError> {
        self.playbin.seek_simple(
            gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT,
            gst::ClockTime::from_nseconds(position.as_nanos().min(u64::MAX as u128) as u64),
        )?;
        Ok(())
    }

    pub fn take_playback_event(&mut self) -> Option<PlaybackEvent> {
        let mut event = None;

        while let Some(message) = self.bus.pop() {
            match message.view() {
                gst::MessageView::Eos(..) => {
                    if self.state == PlaybackState::Paused {
                        // EOS while paused means the HTTP keep-alive dropped, not a real
                        // track end — advancing here would auto-play without user input.
                        tracing::debug!("ignoring EOS while paused (HTTP connection dropped)");
                    } else if matches!(self.state, PlaybackState::Error(_)) {
                        // A source that fails mid-stream posts the error and then an
                        // EOS for the same read. Treating that EOS as a track end
                        // would swallow the error and skip to the next track.
                        tracing::debug!("ignoring EOS that followed a stream error");
                    } else {
                        self.current_item_id = None;
                        self.current_stream_kind = None;
                        self.state = PlaybackState::Stopped;
                        self.is_buffering = false;
                        event = Some(PlaybackEvent::EndOfStream);
                    }
                }
                gst::MessageView::Buffering(buffering) => {
                    self.handle_buffering(buffering.percent());
                }
                gst::MessageView::Error(error) => {
                    if self.state == PlaybackState::Paused {
                        // A stream error while paused means the HTTP keep-alive dropped, not
                        // a playback failure. Recovering here would restart the track and
                        // republish it as playing without user input.
                        tracing::debug!(
                            "ignoring stream error while paused (HTTP connection dropped)"
                        );
                    } else {
                        let message = error.error().to_string();
                        tracing::warn!(error = %message, "GStreamer playback error");
                        let item_id = self.current_item_id.clone();
                        let stream_kind = self.current_stream_kind;
                        self.state = PlaybackState::Error(message.clone());
                        self.is_buffering = false;
                        event = Some(PlaybackEvent::Error {
                            item_id,
                            stream_kind,
                            message,
                        });
                    }
                }
                _ => {}
            }
        }

        event
    }

    /// A buffering message is the source saying how much it has queued. While it
    /// is refilling, the pipeline is held paused: a PLAYING pipeline whose sink
    /// has run dry keeps consuming its clock (and renders silence), so the
    /// reported position walks ahead of the audio and the track skips forward by
    /// the length of the outage when the network comes back.
    fn handle_buffering(&mut self, percent: i32) {
        let percent = percent.clamp(0, 100) as u8;
        self.buffering_percent = Some(percent);
        if percent >= 100 {
            self.is_buffering = false;
            self.resume_after_stall();
        } else {
            self.is_buffering = true;
            if self.state == PlaybackState::Playing {
                self.begin_stall();
            }
        }
    }

    fn begin_stall(&mut self) {
        if self.stalled {
            return;
        }
        if self.playbin.set_state(gst::State::Paused).is_err() {
            return;
        }
        self.stalled = true;
        self.stalled_since = Some(Instant::now());
        self.frozen_since = None;
        self.is_buffering = true;
        tracing::warn!(
            position = ?self.last_position,
            percent = ?self.buffering_percent,
            "stream stalled: holding playback until the source catches up"
        );
    }

    fn resume_after_stall(&mut self) {
        if !self.stalled {
            return;
        }
        self.stalled = false;
        self.stalled_since = None;
        self.frozen_since = None;
        if self.state == PlaybackState::Playing {
            match self.playbin.set_state(gst::State::Playing) {
                Ok(_) => tracing::info!("stream buffered: playback resumed"),
                Err(error) => tracing::warn!(%error, "failed to resume after buffering"),
            }
        }
    }

    /// Watchdog for transports that stall without saying so. Called from the UI
    /// tick: a PLAYING pipeline whose position has not moved for
    /// `STALL_FROZEN_AFTER` is held paused, and poked again every
    /// `STALL_RETRY_AFTER` until the position moves, so a source that never posts
    /// a closing buffering message cannot leave playback paused forever.
    pub fn poll_stall(&mut self) {
        let position = self.position();
        let moved = match (position, self.last_position) {
            (Some(current), Some(previous)) => current != previous,
            (Some(_), None) => true,
            _ => false,
        };
        let playing = matches!(self.state, PlaybackState::Playing);

        if moved {
            self.frozen_since = None;
            if self.stalled {
                // The source caught up before it reported 100%.
                self.stalled = false;
                self.stalled_since = None;
                self.is_buffering = false;
            }
        } else if playing
            && !self.stalled
            && self.frozen_since.is_none()
            && matches!(position, Some(current) if !current.is_zero())
        {
            self.frozen_since = Some(Instant::now());
        }
        self.last_position = position;

        let action = stall_action(
            playing,
            self.stalled,
            moved,
            self.stalled_since.map(|since| since.elapsed()),
            self.frozen_since.map(|since| since.elapsed()),
        );

        match action {
            StallAction::Pause => self.begin_stall(),
            StallAction::Retry => {
                self.stalled = false;
                self.stalled_since = None;
                self.frozen_since = None;
                let _ = self.playbin.set_state(gst::State::Playing);
                tracing::debug!("retrying a stalled stream");
            }
            StallAction::Idle | StallAction::Watching => {}
        }
    }

    pub fn stop(&mut self) -> Result<(), PlaybackError> {
        self.playbin.set_state(gst::State::Null)?;
        self.clear_gapless_state();
        self.current_item_id = None;
        self.current_stream_kind = None;
        self.state = PlaybackState::Stopped;
        self.is_buffering = false;
        self.buffering_percent = None;
        self.stalled = false;
        self.stalled_since = None;
        self.last_position = None;
        self.frozen_since = None;
        Ok(())
    }

    fn clear_gapless_state(&mut self) {
        if let Ok(mut gapless) = self.gapless.lock() {
            gapless.pending_next = None;
            gapless.transitions.clear();
        }
    }

    fn set_http_headers(&mut self, headers: Vec<(String, String)>) {
        if let Ok(mut current_headers) = self.http_headers.lock() {
            *current_headers = headers;
        }
    }
}

impl Drop for PlaybackEngine {
    fn drop(&mut self) {
        let _ = self.playbin.set_state(gst::State::Null);
    }
}

/// What the stall watchdog decided on this tick. Split out of the engine so the
/// policy is a pure function that can be tested without GStreamer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StallAction {
    /// Nothing to do: not playing, or the position is moving.
    Idle,
    /// The position is frozen, but not for long enough to act on yet.
    Watching,
    /// The stream is dry; hold the pipeline paused.
    Pause,
    /// Paused by us and still no progress; poke the source again.
    Retry,
}

fn stall_action(
    playing: bool,
    stalled: bool,
    position_moved: bool,
    stalled_for: Option<Duration>,
    frozen_for: Option<Duration>,
) -> StallAction {
    if stalled {
        if playing && stalled_for.is_some_and(|elapsed| elapsed >= STALL_RETRY_AFTER) {
            StallAction::Retry
        } else {
            StallAction::Idle
        }
    } else if !playing || position_moved {
        StallAction::Idle
    } else if frozen_for.is_some_and(|elapsed| elapsed >= STALL_FROZEN_AFTER) {
        StallAction::Pause
    } else {
        StallAction::Watching
    }
}

fn configure_http_source_headers(source: &gst::Element, headers: &[(String, String)]) {
    if headers.is_empty() || source.find_property("extra-headers").is_none() {
        return;
    }

    let mut builder = gst::Structure::builder("extra-headers");
    for (name, value) in headers {
        builder = builder.field(name.as_str(), value.as_str());
    }
    let headers = builder.build();
    source.set_property("extra-headers", &headers);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn since(millis: u64) -> Option<Duration> {
        Some(Duration::from_millis(millis))
    }

    #[test]
    fn a_moving_position_never_pauses_playback() {
        assert_eq!(
            stall_action(true, false, true, None, since(9_000)),
            StallAction::Idle
        );
    }

    #[test]
    fn a_frozen_position_pauses_only_after_the_grace_period() {
        assert_eq!(
            stall_action(true, false, false, None, since(400)),
            StallAction::Watching
        );
        assert_eq!(
            stall_action(true, false, false, None, since(2_500)),
            StallAction::Pause
        );
    }

    #[test]
    fn a_paused_player_is_never_treated_as_stalled() {
        // The user paused: a frozen position is expected, not a stall.
        assert_eq!(
            stall_action(false, false, false, None, since(30_000)),
            StallAction::Idle
        );
    }

    #[test]
    fn a_stalled_pipeline_is_retried_rather_than_paused_twice() {
        assert_eq!(
            stall_action(true, true, false, since(1_000), None),
            StallAction::Idle
        );
        assert_eq!(
            stall_action(true, true, false, since(4_000), None),
            StallAction::Retry
        );
    }

    #[test]
    fn a_stall_keeps_holding_while_the_user_is_paused() {
        // Nothing was resumed, so the pipeline must not be restarted.
        assert_eq!(
            stall_action(false, true, false, since(30_000), None),
            StallAction::Idle
        );
    }

    /// A 16-bit mono WAV of silence, so the sink has real audio to run dry on.
    fn wav_silence(seconds: u32, sample_rate: u32) -> Vec<u8> {
        let samples = seconds * sample_rate;
        let data_len = samples * 2;
        let mut bytes = Vec::with_capacity(44 + data_len as usize);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
        bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
        bytes.extend_from_slice(&sample_rate.to_le_bytes());
        bytes.extend_from_slice(&(sample_rate * 2).to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        bytes.resize(44 + data_len as usize, 0);
        bytes
    }

    /// Serve `body` over HTTP once, pacing it to real time and pausing for
    /// `stall` after `stall_at` bytes, the way a bad wifi link drops out.
    fn serve_stalling_body(body: Vec<u8>, stall_at: usize, stall: Duration) -> Url {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let total = body.len();
        std::thread::spawn(move || {
            let (mut stream, _) = match listener.accept() {
                Ok(pair) => pair,
                Err(_) => return,
            };
            use std::io::{BufRead, BufReader, Write};
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let mut request_line = String::new();
            let _ = reader.read_line(&mut request_line);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) if line == "\r\n" => break,
                    Ok(_) => {}
                    Err(_) => return,
                }
            }
            let _ = write!(
                stream,
                "HTTP/1.0 200 OK\r\nContent-Type: audio/wav\r\nContent-Length: {total}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n"
            );
            // Real time: send one second of audio per second of wall clock, so
            // the client's queue stays short enough for a stall to underrun it.
            let bytes_per_sec = (total as f64 / 24.0).max(1.0);
            let started = Instant::now();
            let mut sent = 0usize;
            let mut stalled = false;
            let chunk = 2_048usize;
            while sent < total {
                let end = (sent + chunk).min(total);
                if std::io::Write::write_all(&mut stream, &body[sent..end]).is_err() {
                    return;
                }
                sent = end;
                if !stalled && sent >= stall_at {
                    stalled = true;
                    std::thread::sleep(stall);
                }
                let target = started.elapsed().as_secs_f64();
                let paced = sent as f64 / bytes_per_sec;
                if paced > target {
                    std::thread::sleep(Duration::from_secs_f64(paced - target));
                }
            }
            let _ = stream.flush();
            let _ = std::io::copy(&mut reader.into_inner(), &mut std::io::sink());
        });
        format!("http://127.0.0.1:{port}/stall.wav")
            .parse()
            .expect("url")
    }

    /// End-to-end harness for the stall fix: the pipeline must hold the playhead
    /// while the source is dry instead of letting it walk ahead.
    ///
    /// Needs a working audio sink, because the silence a real sink renders during
    /// an underrun is the whole bug, so it only runs on request:
    /// `GTUNES_AUDIO_HARNESS=1 cargo test --offline -- --nocapture stall_holds`
    #[test]
    fn stall_holds_the_playhead_while_the_source_is_dry() {
        if std::env::var_os("GTUNES_AUDIO_HARNESS").is_none() {
            return;
        }
        let _ = gst::init();
        let body = wav_silence(24, 8_000);
        let total = body.len();
        let url = serve_stalling_body(body, total / 3, Duration::from_secs(6));

        let mut engine = PlaybackEngine::new().expect("playbin");
        engine.playbin.set_property("volume", 0.0);
        engine
            .play(PlaybackRequest {
                item_id: "harness".to_string(),
                stream_url: url,
                http_headers: Vec::new(),
                stream_kind: PlaybackStreamKind::Direct,
                title: "stall harness".to_string(),
            })
            .expect("play");

        let started = Instant::now();
        let mut samples: Vec<(f64, f64, bool)> = Vec::new();
        while started.elapsed() < Duration::from_secs(20) {
            engine.poll_stall();
            let _ = engine.take_playback_event();
            let position = engine.position().map(|p| p.as_secs_f64()).unwrap_or(-1.0);
            samples.push((
                started.elapsed().as_secs_f64(),
                position,
                engine.is_stalled(),
            ));
            std::thread::sleep(Duration::from_millis(100));
        }
        for (at, position, stalled) in &samples {
            println!("t={at:5.2} pos={position:6.2} stalled={stalled}");
        }

        // The stall really happened: at least two seconds of samples where the
        // position did not move at all.
        let mut frozen_run = 0usize;
        let mut longest_frozen = 0usize;
        for pair in samples.windows(2) {
            if pair[0].1 >= 0.0 && pair[1].1 >= 0.0 && (pair[1].1 - pair[0].1).abs() < 0.05 {
                frozen_run += 1;
                longest_frozen = longest_frozen.max(frozen_run);
            } else {
                frozen_run = 0;
            }
        }
        assert!(
            longest_frozen >= 15,
            "the harness never saw a stall (longest frozen run {longest_frozen} samples)"
        );

        // The asserted behaviour: when the source comes back the playhead must
        // carry on from where it stopped, not skip forward over the outage. A
        // single sample-to-sample jump larger than the sample interval means the
        // stream leapt ahead and that audio was never heard.
        let mut biggest_jump = 0.0f64;
        let mut jump_at = 0.0f64;
        for pair in samples.windows(2) {
            // Skip the frames before the pipeline reports a position at all.
            if pair[0].1 < 0.0 || pair[1].1 < 0.0 {
                continue;
            }
            let delta = pair[1].1 - pair[0].1;
            if delta > biggest_jump {
                biggest_jump = delta;
                jump_at = pair[1].0;
            }
        }
        assert!(
            biggest_jump < 1.0,
            "playhead jumped {biggest_jump:.2}s at t={jump_at:.2}s instead of holding still"
        );

        // And it has to carry on afterwards instead of staying parked.
        let last = samples
            .last()
            .map(|(_, position, _)| *position)
            .unwrap_or(-1.0);
        let frozen_position = samples
            .iter()
            .map(|(_, position, _)| *position)
            .fold(f64::MIN, f64::max);
        assert!(
            last > frozen_position - 1.0 && last > 6.0,
            "playback did not continue after the stall (last={last})"
        );
    }
}
