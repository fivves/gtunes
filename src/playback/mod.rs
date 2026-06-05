#![allow(dead_code)]

use gst::prelude::*;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use thiserror::Error;
use url::Url;

#[derive(Debug, Error)]
pub enum PlaybackError {
    #[error("GStreamer playbin element is unavailable")]
    PlaybinUnavailable,
    #[error("GStreamer state change failed: {0}")]
    StateChange(#[from] gst::StateChangeError),
    #[error("GStreamer seek failed: {0}")]
    Seek(#[from] gst::glib::BoolError),
    #[error("invalid stream URL: {0}")]
    InvalidUrl(#[from] url::ParseError),
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

#[derive(Debug)]
pub struct PlaybackEngine {
    playbin: gst::Element,
    bus: gst::Bus,
    gapless: Arc<Mutex<GaplessState>>,
    http_headers: Arc<Mutex<Vec<(String, String)>>>,
    state: PlaybackState,
    current_item_id: Option<String>,
    current_stream_kind: Option<PlaybackStreamKind>,
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
        })
    }

    pub fn state(&self) -> &PlaybackState {
        &self.state
    }

    pub fn current_item_id(&self) -> Option<&str> {
        self.current_item_id.as_deref()
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
        Some(request)
    }

    pub fn resume(&mut self) -> Result<(), PlaybackError> {
        self.playbin.set_state(gst::State::Playing)?;
        self.state = PlaybackState::Playing;
        Ok(())
    }

    pub fn pause(&mut self) -> Result<(), PlaybackError> {
        self.playbin.set_state(gst::State::Paused)?;
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
                    self.current_item_id = None;
                    self.current_stream_kind = None;
                    self.state = PlaybackState::Stopped;
                    event = Some(PlaybackEvent::EndOfStream);
                }
                gst::MessageView::Error(error) => {
                    let message = error.error().to_string();
                    tracing::warn!(error = %message, "GStreamer playback error");
                    let item_id = self.current_item_id.clone();
                    let stream_kind = self.current_stream_kind;
                    self.state = PlaybackState::Error(message.clone());
                    event = Some(PlaybackEvent::Error {
                        item_id,
                        stream_kind,
                        message,
                    });
                }
                _ => {}
            }
        }

        event
    }

    pub fn stop(&mut self) -> Result<(), PlaybackError> {
        self.playbin.set_state(gst::State::Null)?;
        self.clear_gapless_state();
        self.current_item_id = None;
        self.current_stream_kind = None;
        self.state = PlaybackState::Stopped;
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
