use stream_capture::{CaptureFrame, FrameQueue};

#[test]
fn latest_preview_snapshot_does_not_consume_delivery_queue() {
    let queue = FrameQueue::new(2);
    queue.push_latest(CaptureFrame::test(1, 320, 180));
    queue.push_latest(CaptureFrame::test(2, 320, 180));

    let latest = queue.latest_frame().expect("latest frame");
    assert_eq!(latest.sequence, 2);

    // Reading the snapshot must not steal the frame used by the existing
    // consumer path (streaming / main-thread polling).
    let delivered = queue.try_recv().expect("queued frame remains available");
    assert!(matches!(delivered.sequence, 1 | 2));
}
