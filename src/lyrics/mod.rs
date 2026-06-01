#![allow(dead_code)]

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LyricsAvailability {
    ComingSoon,
    Missing,
    Unsynced,
    Synced,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LyricLine {
    pub timestamp_ms: Option<u64>,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LyricsDocument {
    pub availability: LyricsAvailability,
    pub lines: Vec<LyricLine>,
}

impl LyricsDocument {
    pub fn coming_soon() -> Self {
        Self {
            availability: LyricsAvailability::ComingSoon,
            lines: Vec::new(),
        }
    }
}
