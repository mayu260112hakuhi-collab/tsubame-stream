use std::path::PathBuf;
use stream_recording::RecordingPaths;

#[test]
fn recording_paths_are_independent_from_ffmpeg_location() {
    let p = RecordingPaths::for_root(PathBuf::from("recordings/test"));
    assert!(p.final_video.ends_with("recording.mp4"));
}
