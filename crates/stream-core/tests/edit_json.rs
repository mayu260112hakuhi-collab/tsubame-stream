use stream_core::{EditManifest, EditMarker, MarkerKind, SessionConfig};
#[test]
fn manifest_has_stable_format_and_marker_time() {
    let mut m = EditManifest::new(SessionConfig::default());
    m.markers
        .push(EditMarker::new(185420, MarkerKind::Short, "ここ使う"));
    let v: serde_json::Value = serde_json::from_str(&m.to_json_pretty().unwrap()).unwrap();
    assert_eq!(v["format"], "yaoyorozu_stream_edit");
    assert_eq!(v["version"], 1);
    assert_eq!(v["markers"][0]["time_ms"], 185420);
    assert_eq!(v["media"]["video"], "recording.mp4");
}
