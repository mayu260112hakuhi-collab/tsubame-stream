use stream_capture::{CaptureBackend, CaptureFrame, CaptureSource, FrameQueue, WindowInfo};

#[test]
fn full_preview_queue_keeps_latest_frame_instead_of_blocking() {
    let q = FrameQueue::new(1);
    q.push_latest(CaptureFrame::test(1, 2, 2));
    q.push_latest(CaptureFrame::test(2, 2, 2));
    let f = q.try_recv().unwrap();
    assert_eq!(f.sequence, 2);
}

#[test]
fn capture_frame_uses_rgba_byte_count() {
    let f = CaptureFrame::test(7, 3, 4);
    assert_eq!(f.rgba.len(), 3 * 4 * 4);
}

#[test]
fn capture_source_has_human_readable_name() {
    assert_eq!(CaptureSource::Desktop.display_name(), "デスクトップ全体");

    let source = CaptureSource::Window(WindowInfo {
        hwnd: 123,
        title: "Blender".to_owned(),
    });
    assert_eq!(source.display_name(), "Blender");
}

#[test]
fn phase5_default_backend_is_windows_graphics_capture() {
    assert_eq!(
        CaptureBackend::default(),
        CaptureBackend::WindowsGraphicsCapture
    );
}
