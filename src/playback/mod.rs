#![allow(dead_code)]

use gst::prelude::*;
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
    state: PlaybackState,
    current_item_id: Option<String>,
}

impl PlaybackEngine {
    pub fn new() -> Result<Self, PlaybackError> {
        let playbin = gst::ElementFactory::make("playbin")
            .build()
            .map_err(|_| PlaybackError::PlaybinUnavailable)?;
        let bus = playbin.bus().ok_or(PlaybackError::PlaybinUnavailable)?;
        Ok(Self {
            playbin,
            bus,
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
        self.current_item_id = None;
        self.state = PlaybackState::Stopped;
        self.playbin
            .set_property("uri", request.stream_url.as_str());
        self.playbin.set_state(gst::State::Playing)?;
        self.current_item_id = Some(request.item_id);
        self.state = PlaybackState::Playing;
        Ok(())
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
        self.current_item_id = None;
        self.state = PlaybackState::Stopped;
        Ok(())
    }
}

impl Drop for PlaybackEngine {
    fn drop(&mut self) {
        let _ = self.playbin.set_state(gst::State::Null);
    }
}
