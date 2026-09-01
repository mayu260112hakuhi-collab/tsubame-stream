use stream_core::{SessionConfig, StreamPreset};
#[test]
fn presets_match_design() {
    assert_eq!(StreamPreset::Game.dimensions(), (1920, 1080, 60));
    assert_eq!(StreamPreset::Work.dimensions(), (1920, 1080, 30));
    assert_eq!(StreamPreset::Light.dimensions(), (1280, 720, 30));
}
#[test]
fn session_defaults_to_game() {
    assert_eq!(SessionConfig::default().preset, StreamPreset::Game);
}
