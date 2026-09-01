use std::time::Duration;
use stream_capture::{PreviewMode, PreviewPolicy};

#[test]
fn recording_default_preview_is_fifteen_fps() {
    let policy = PreviewPolicy::for_recording(PreviewMode::Fps15);
    assert_eq!(policy.target_fps(), Some(15));
    assert_eq!(policy.interval(), Some(Duration::from_secs_f64(1.0 / 15.0)));
}

#[test]
fn disabled_preview_requests_no_cpu_readback() {
    let policy = PreviewPolicy::for_recording(PreviewMode::Off);
    assert_eq!(policy.target_fps(), None);
    assert_eq!(policy.interval(), None);
}

#[test]
fn normal_preview_remains_thirty_fps() {
    assert_eq!(PreviewPolicy::normal().target_fps(), Some(30));
}
