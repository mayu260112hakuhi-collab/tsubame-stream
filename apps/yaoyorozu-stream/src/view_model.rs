use stream_core::{EditMarker, MarkerKind, SessionConfig, StreamPreset};

#[derive(Debug)]
pub struct StreamViewModel {
    pub session: SessionConfig,
    pub is_live: bool,
    pub has_finished_session: bool,
    pub markers: Vec<EditMarker>,
}
impl Default for StreamViewModel {
    fn default() -> Self {
        Self {
            session: SessionConfig::default(),
            is_live: false,
            has_finished_session: false,
            markers: Vec::new(),
        }
    }
}
impl StreamViewModel {
    pub fn start(&mut self) {
        self.is_live = true;
        self.has_finished_session = false;
    }
    pub fn stop(&mut self) {
        self.is_live = false;
        self.has_finished_session = true;
    }
    pub fn can_send_to_aviutl2(&self) -> bool {
        !self.is_live && self.has_finished_session
    }
    pub fn add_marker(&mut self, time_ms: u64, kind: MarkerKind, label: impl Into<String>) {
        self.markers.push(EditMarker::new(time_ms, kind, label));
    }
    pub fn set_preset(&mut self, preset: StreamPreset) {
        self.session.preset = preset;
    }
}
