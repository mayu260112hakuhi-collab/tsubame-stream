use std::sync::Arc;
use stream_capture::CaptureFrame;

#[test]
fn cloned_capture_frames_share_the_same_pixel_storage() {
    let a = CaptureFrame::test(1, 1920, 1080);
    let b = a.clone();

    assert_eq!(a.rgba.len(), b.rgba.len());
    assert!(Arc::ptr_eq(&a.rgba, &b.rgba));
}
