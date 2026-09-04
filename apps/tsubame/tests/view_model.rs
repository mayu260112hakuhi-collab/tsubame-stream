use stream_core::{MarkerKind, StreamPreset};
use tsubame_stream::view_model::StreamViewModel;

#[test]
fn compact_ui_model_defaults_to_game_and_exposes_post_stream_bridge() {
    let mut vm = StreamViewModel::default();
    assert_eq!(vm.session.preset, StreamPreset::Game);
    assert!(!vm.can_send_to_aviutl2());
    vm.start();
    vm.add_marker(1000, MarkerKind::Cut, "cut");
    vm.add_marker(2000, MarkerKind::Short, "short");
    vm.add_marker(3000, MarkerKind::Chapter, "chapter");
    vm.add_marker(4000, MarkerKind::Note, "note");
    assert_eq!(vm.markers.len(), 4);
    assert!(!vm.can_send_to_aviutl2());
    vm.stop();
    assert!(vm.can_send_to_aviutl2());
}
