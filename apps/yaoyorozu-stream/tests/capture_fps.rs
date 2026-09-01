use stream_core::StreamPreset;

#[test]
fn game_preset_requests_sixty_fps_capture() {
    assert_eq!(StreamPreset::Game.dimensions().2, 60);
}

#[test]
fn work_and_light_presets_request_thirty_fps_capture() {
    assert_eq!(StreamPreset::Work.dimensions().2, 30);
    assert_eq!(StreamPreset::Light.dimensions().2, 30);
}
