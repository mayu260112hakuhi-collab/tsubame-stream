use crossbeam_channel::{bounded, Receiver, Sender};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamPreset {
    Game,
    Work,
    Light,
}
impl StreamPreset {
    pub fn dimensions(self) -> (u32, u32, u32) {
        match self {
            Self::Game => (1920, 1080, 60),
            Self::Work => (1920, 1080, 30),
            Self::Light => (1280, 720, 30),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionConfig {
    pub preset: StreamPreset,
}
impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            preset: StreamPreset::Game,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MarkerKind {
    Cut,
    Short,
    Chapter,
    Note,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditMarker {
    pub time_ms: u64,
    pub kind: MarkerKind,
    pub label: String,
}
impl EditMarker {
    pub fn new(time_ms: u64, kind: MarkerKind, label: impl Into<String>) -> Self {
        Self {
            time_ms,
            kind,
            label: label.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaPaths {
    pub video: String,
    pub microphone: String,
    pub desktop_audio: String,
}
impl Default for MediaPaths {
    fn default() -> Self {
        Self {
            video: "recording.mp4".into(),
            microphone: "microphone.wav".into(),
            desktop_audio: "desktop.wav".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditManifest {
    pub format: String,
    pub version: u32,
    pub session: SessionConfig,
    pub media: MediaPaths,
    pub markers: Vec<EditMarker>,
}
impl EditManifest {
    pub fn new(session: SessionConfig) -> Self {
        Self {
            format: "yaoyorozu_stream_edit".into(),
            version: 1,
            session,
            media: MediaPaths::default(),
            markers: Vec::new(),
        }
    }
    pub fn to_json_pretty(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerCommand {
    Start,
    Stop,
    Shutdown,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerStatus {
    Idle,
    Starting,
    Running,
    Stopping,
    Failed(String),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerEvent {
    Status(WorkerStatus),
    DroppedFrame(u64),
}

pub fn bounded_worker_channel(capacity: usize) -> (Sender<WorkerCommand>, Receiver<WorkerCommand>) {
    bounded(capacity.max(1))
}
