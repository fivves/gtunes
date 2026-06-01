#![allow(dead_code)]

use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use gst::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use thiserror::Error;

use crate::cache::CacheDatabase;
use crate::config;

const TARGET_SAMPLE_COUNT: usize = 360;
const WAVEFORM_CACHE_VERSION: u32 = 2;
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct WaveformKey {
    pub item_id: String,
    pub media_source_id: String,
}

#[derive(Clone, Debug)]
pub enum WaveformState {
    Missing,
    Queued,
    Generating,
    Ready(PathBuf),
    Failed(String),
}

#[derive(Clone, Debug)]
pub struct WaveformSummary {
    pub key: WaveformKey,
    pub sample_count: usize,
    pub peaks: Vec<f32>,
}

impl WaveformSummary {
    pub fn empty(key: WaveformKey) -> Self {
        Self {
            key,
            sample_count: 0,
            peaks: Vec::new(),
        }
    }
}

#[derive(Debug, Error)]
pub enum WaveformError {
    #[error("sqlite error: {0}")]
    Cache(#[from] crate::cache::CacheError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("project directory is unavailable")]
    ProjectDirectoryUnavailable,
    #[error("GStreamer element is unavailable: {0}")]
    ElementUnavailable(&'static str),
    #[error("failed to build waveform pipeline")]
    PipelineBuildFailed,
    #[error("GStreamer state change failed: {0}")]
    StateChange(#[from] gst::StateChangeError),
    #[error("GStreamer error: {0}")]
    GStreamer(String),
    #[error("decoded audio did not produce waveform samples")]
    NoSamples,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct WaveformFile {
    version: u32,
    item_id: String,
    media_source_id: String,
    sample_count: usize,
    peaks: Vec<f32>,
}

pub fn load_or_generate(
    key: WaveformKey,
    stream_url: &str,
) -> Result<WaveformSummary, WaveformError> {
    let database = CacheDatabase::open_default()?;
    if let Some(path) = database.waveform_cache_path(&key.item_id, &key.media_source_id)?
        && path.exists()
    {
        match read_summary(&path, key.clone()) {
            Ok(summary) => return Ok(summary),
            Err(error) => {
                tracing::debug!(%error, path = %path.display(), "regenerating stale waveform cache");
            }
        }
    }

    let peaks = generate_from_uri(stream_url, TARGET_SAMPLE_COUNT)?;
    let summary = WaveformSummary {
        key,
        sample_count: peaks.len(),
        peaks,
    };
    let path = summary_cache_path(&summary.key)?;
    write_summary(&path, &summary)?;
    database.save_waveform_cache(
        &summary.key.item_id,
        &summary.key.media_source_id,
        summary.sample_count,
        &path,
    )?;
    Ok(summary)
}

fn read_summary(path: &Path, key: WaveformKey) -> Result<WaveformSummary, WaveformError> {
    let file = std::fs::File::open(path)?;
    let payload: WaveformFile = serde_json::from_reader(file)?;
    if payload.version != WAVEFORM_CACHE_VERSION || payload.sample_count != TARGET_SAMPLE_COUNT {
        return Err(WaveformError::NoSamples);
    }

    Ok(WaveformSummary {
        key,
        sample_count: payload.sample_count,
        peaks: payload.peaks,
    })
}

fn write_summary(path: &Path, summary: &WaveformSummary) -> Result<(), WaveformError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::File::create(path)?;
    serde_json::to_writer(
        file,
        &WaveformFile {
            version: WAVEFORM_CACHE_VERSION,
            item_id: summary.key.item_id.clone(),
            media_source_id: summary.key.media_source_id.clone(),
            sample_count: summary.sample_count,
            peaks: summary.peaks.clone(),
        },
    )?;
    Ok(())
}

fn summary_cache_path(key: &WaveformKey) -> Result<PathBuf, WaveformError> {
    let project_dirs = ProjectDirs::from("dev", config::DEVELOPER_NAME, config::APP_NAME)
        .ok_or(WaveformError::ProjectDirectoryUnavailable)?;
    let filename = format!(
        "{}-{}-v{}.json",
        sanitize_cache_component(&key.item_id),
        sanitize_cache_component(&key.media_source_id),
        WAVEFORM_CACHE_VERSION
    );
    Ok(project_dirs.cache_dir().join("waveforms").join(filename))
}

fn sanitize_cache_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn generate_from_uri(uri: &str, target_count: usize) -> Result<Vec<f32>, WaveformError> {
    let pipeline = gst::Pipeline::new();
    let source = make_element("uridecodebin")?;
    let convert = make_element("audioconvert")?;
    let resample = make_element("audioresample")?;
    let capsfilter = make_element("capsfilter")?;
    let sink = make_element("fakesink")?;

    source.set_property("uri", uri);
    capsfilter.set_property(
        "caps",
        gst::Caps::builder("audio/x-raw")
            .field("format", "F32LE")
            .field("channels", 1_i32)
            .build(),
    );
    sink.set_property("sync", false);
    sink.set_property("signal-handoffs", true);

    let peaks = Arc::new(Mutex::new(Vec::new()));
    let handoff_peaks = peaks.clone();
    sink.connect("handoff", false, move |values| {
        let buffer = values
            .get(1)
            .and_then(|value| value.get::<gst::Buffer>().ok())?;
        let Ok(map) = buffer.map_readable() else {
            return None;
        };
        let mut peak = 0.0_f32;
        let mut sum_squares = 0.0_f32;
        let mut sample_count = 0_usize;
        for sample in map
            .as_slice()
            .chunks_exact(4)
            .map(|bytes| f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]).abs())
        {
            peak = peak.max(sample);
            sum_squares += sample * sample;
            sample_count += 1;
        }
        if sample_count == 0 {
            return None;
        }
        let rms = (sum_squares / sample_count as f32).sqrt();
        let peak = (rms * 0.82 + peak * 0.18).clamp(0.0, 1.0);
        if let Ok(mut peaks) = handoff_peaks.lock() {
            peaks.push(peak);
        }
        None
    });

    pipeline
        .add_many([&source, &convert, &resample, &capsfilter, &sink])
        .map_err(|_| WaveformError::PipelineBuildFailed)?;
    gst::Element::link_many([&convert, &resample, &capsfilter, &sink])
        .map_err(|_| WaveformError::PipelineBuildFailed)?;

    let convert_sink = convert
        .static_pad("sink")
        .ok_or(WaveformError::PipelineBuildFailed)?;
    source.connect_pad_added(move |_, pad| {
        if convert_sink.is_linked() {
            return;
        }
        let caps = pad.current_caps().unwrap_or_else(|| pad.query_caps(None));
        let Some(structure) = caps.structure(0) else {
            return;
        };
        if structure.name().starts_with("audio/") {
            let _ = pad.link(&convert_sink);
        }
    });

    let bus = pipeline.bus().ok_or(WaveformError::PipelineBuildFailed)?;
    pipeline.set_state(gst::State::Playing)?;

    loop {
        let Some(message) = bus.timed_pop(gst::ClockTime::from_nseconds(250_000_000)) else {
            continue;
        };

        match message.view() {
            gst::MessageView::Eos(_) => break,
            gst::MessageView::Error(error) => {
                let _ = pipeline.set_state(gst::State::Null);
                return Err(WaveformError::GStreamer(error.error().to_string()));
            }
            _ => {}
        }
    }

    pipeline.set_state(gst::State::Null)?;
    let peaks = peaks
        .lock()
        .map_err(|_| WaveformError::GStreamer("waveform sample lock was poisoned".to_string()))?
        .clone();

    if peaks.is_empty() {
        return Err(WaveformError::NoSamples);
    }

    Ok(shape_peaks(resample_peaks(&peaks, target_count)))
}

fn make_element(name: &'static str) -> Result<gst::Element, WaveformError> {
    gst::ElementFactory::make(name)
        .build()
        .map_err(|_| WaveformError::ElementUnavailable(name))
}

fn resample_peaks(peaks: &[f32], target_count: usize) -> Vec<f32> {
    if peaks.len() == target_count {
        return peaks.to_vec();
    }

    let bucket_width = peaks.len() as f64 / target_count as f64;
    (0..target_count)
        .map(|index| {
            let start = (index as f64 * bucket_width).floor() as usize;
            let end = (((index + 1) as f64 * bucket_width).ceil() as usize).min(peaks.len());
            let bucket = &peaks[start..end];
            if bucket.is_empty() {
                return 0.02;
            }
            let average = bucket.iter().copied().sum::<f32>() / bucket.len() as f32;
            let peak = bucket.iter().copied().fold(0.0_f32, f32::max);
            (average * 0.74 + peak * 0.26).max(0.02)
        })
        .collect()
}

fn shape_peaks(mut peaks: Vec<f32>) -> Vec<f32> {
    let mut sorted = peaks.clone();
    sorted.sort_by(f32::total_cmp);
    let reference_index = ((sorted.len().saturating_sub(1)) as f32 * 0.95).round() as usize;
    let reference = sorted
        .get(reference_index)
        .copied()
        .unwrap_or(1.0)
        .max(0.05);

    for peak in &mut peaks {
        let normalized = (*peak / reference).clamp(0.0, 1.0);
        *peak = normalized.powf(0.72).clamp(0.03, 0.92);
    }
    peaks
}
