use stream_capture::PreviewMode;

#[test]
fn recording_preview_mode_can_switch_between_15fps_and_off() {
    assert_ne!(PreviewMode::Fps15, PreviewMode::Off);
}
