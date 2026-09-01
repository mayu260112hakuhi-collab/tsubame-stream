use stream_capture::{CaptureFrame, FrameQueue};

#[test]
fn preview_and_recording_queues_can_hold_independent_latest_frames() {
    let preview = FrameQueue::new(1);
    let recording = FrameQueue::new(1);

    preview.push_latest(CaptureFrame::test(1, 2, 2));
    recording.push_latest(CaptureFrame::test(2, 2, 2));

    assert_eq!(preview.try_recv().unwrap().sequence, 1);
    assert_eq!(recording.try_recv().unwrap().sequence, 2);
}
