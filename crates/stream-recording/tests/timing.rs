use std::time::Duration;
use stream_recording::frames_due;

#[test]
fn ten_seconds_at_sixty_fps_is_six_hundred_frames() {
    assert_eq!(frames_due(Duration::from_secs(10), 60), 600);
}

#[test]
fn half_second_at_thirty_fps_is_fifteen_frames() {
    assert_eq!(frames_due(Duration::from_millis(500), 30), 15);
}

#[test]
fn wall_clock_not_callback_count_controls_duration() {
    // Even if WGC only supplied a handful of unique frames, the output
    // scheduler must still produce 120 timeline frames after 2 seconds @60.
    assert_eq!(frames_due(Duration::from_secs(2), 60), 120);
}
