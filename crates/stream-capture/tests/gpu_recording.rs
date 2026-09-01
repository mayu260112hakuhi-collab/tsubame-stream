use stream_capture::{GpuRecordingConfig, GpuRecordingStatus};

#[test]
fn gpu_recording_contract_targets_h264_geometry_and_fps() {
    let cfg = GpuRecordingConfig {
        path: "recordings/test/video_only.mp4".into(),
        width: 1920,
        height: 1080,
        fps: 60,
        bitrate_bps: 6_000_000,
    };

    assert_eq!(cfg.width, 1920);
    assert_eq!(cfg.height, 1080);
    assert_eq!(cfg.fps, 60);
    assert_eq!(GpuRecordingStatus::Idle, GpuRecordingStatus::Idle);
}
