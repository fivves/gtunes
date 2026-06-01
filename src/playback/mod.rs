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
    pub title: String,
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
    state: PlaybackState,
    current_item_id: Option<String>,
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
        let gapless_for_signal = gapless.clone();
        playbin.connect("about-to-finish", false, move |values| {
            let Some(playbin) = values
                .first()
                .and_then(|value| value.get::<gst::Element>().ok())
            else {
                return None;
            };
            let Some(request) = gapless_for_signal
                .lock()
                .ok()
                .and_then(|mut gapless| gapless.pending_next.take())
            else {
                return None;
            };

            tracing::info!(
                item_id = %request.item_id,
                title = %request.title,
                "queueing gapless Jellyfin stream"
            );
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
            state: PlaybackState::Stopped,
            current_item_id: None,
        })
    }

    pub fn state(&self) -> &PlaybackState {
        &self.state
    }

    pub fn current_item_id(&self) -> Option<&str> {
        self.current_item_id.as_deref()
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
        self.state = PlaybackState::Stopped;
        self.playbin
            .set_property("uri", request.stream_url.as_str());
        self.playbin.set_state(gst::State::Playing)?;
        self.current_item_id = Some(request.item_id);
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

    pub fn take_end_of_stream(&mut self) -> bool {
        let mut ended = false;

        while let Some(message) = self.bus.pop() {
            match message.view() {
                gst::MessageView::Eos(..) => {
                    self.current_item_id = None;
                    self.state = PlaybackState::Stopped;
                    ended = true;
                }
                gst::MessageView::Error(error) => {
                    let message = error.error().to_string();
                    tracing::warn!(error = %message, "GStreamer playback error");
                    self.current_item_id = None;
                    self.state = PlaybackState::Error(message);
                }
                _ => {}
            }
        }

        ended
    }

    pub fn stop(&mut self) -> Result<(), PlaybackError> {
        self.playbin.set_state(gst::State::Null)?;
        self.clear_gapless_state();
        self.current_item_id = None;
        self.state = PlaybackState::Stopped;
        Ok(())
    }

    fn clear_gapless_state(&mut self) {
        if let Ok(mut gapless) = self.gapless.lock() {
            gapless.pending_next = None;
            gapless.transitions.clear();
        }
    }
}

impl Drop for PlaybackEngine {
    fn drop(&mut self) {
        let _ = self.playbin.set_state(gst::State::Null);
    }
}
