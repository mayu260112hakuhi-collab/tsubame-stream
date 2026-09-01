use stream_recording::RecordingPaths;

#[test]
fn recording_paths_keep_editor_tracks_separate() {
    let p = RecordingPaths::for_root("recordings/test");
    assert!(p.video_only.ends_with("video_only.mp4"));
    assert!(p.final_video.ends_with("recording.mp4"));
    assert!(p.microphone.ends_with("microphone.wav"));
    assert!(p.desktop_audio.ends_with("desktop.wav"));
    assert!(p.edit_json.ends_with("yaoyorozu_edit.json"));
}

#[test]
fn application_track_path_is_stable_and_safe() {
    let p = RecordingPaths::for_root("recordings/test");
    let path = p.application_audio(100, "Discord");
    assert!(path.ends_with("application_100_discord.wav"));
}
