// Shared imports for the ui module tree. Every ui module pulls this in, so
// the modules split out of the former monolithic window.rs share one import
// surface instead of each curating its own list.
pub(crate) use adw::prelude::*;
pub(crate) use gtk::glib::object::IsA;
pub(crate) use gtk::{Align, Orientation};
pub(crate) use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, PlatformConfig,
};
pub(crate) use std::cell::{Cell, RefCell};
pub(crate) use std::cmp::Ordering;
pub(crate) use std::collections::{HashMap, HashSet};
pub(crate) use std::fmt;
pub(crate) use std::path::PathBuf;
pub(crate) use std::rc::Rc;
pub(crate) use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
pub(crate) use std::sync::{Mutex, mpsc};
pub(crate) use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(crate) use crate::cache::{CacheDatabase, JellyfinSession};
pub(crate) use crate::cast::{self, CastDevice, CastDeviceKind, CastEvent};
pub(crate) use crate::config;
pub(crate) use crate::discord::{
    DiscordPresence, PresenceActivity, PresencePlaybackState, artwork_cache_path,
};
pub(crate) use crate::jellyfin::{
    JellyfinClient, JellyfinClientError, JellyfinItemSummary, JellyfinPlaylist, JellyfinTrack,
    stream_http_headers_for_token,
};
pub(crate) use crate::playback::{
    ExternalStreamSource, PlaybackEngine, PlaybackEvent, PlaybackRequest, PlaybackState,
    PlaybackStreamKind, resolve_external_stream_url, session,
};
pub(crate) use crate::waveform::{WaveformKey, WaveformSummary};

pub(crate) use super::artwork::*;
pub(crate) use super::connection::*;
pub(crate) use super::integrations::*;
pub(crate) use super::library::*;
pub(crate) use super::models::*;
pub(crate) use super::persistence::*;
pub(crate) use super::shortcuts::*;
pub(crate) use super::state::*;
pub(crate) use super::widgets::*;
pub(crate) use super::window::*;
