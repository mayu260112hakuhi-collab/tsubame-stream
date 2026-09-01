use yaoyorozu_stream::scene::OverlaySource;

#[test]
fn disabled_overlay_does_not_change_frame() {
    let overlay = OverlaySource::default();
    let mut rgba = vec![10u8; 64 * 36 * 4];
    let before = rgba.clone();
    overlay.compose_test_overlay(&mut rgba, 64, 36, 0.0);
    assert_eq!(rgba, before);
}

#[test]
fn enabled_overlay_changes_pixels_without_resizing_frame() {
    let mut overlay = OverlaySource::default();
    overlay.enabled = true;
    overlay.width_percent = 30.0;
    overlay.x_percent = 50.0;
    overlay.y_percent = 50.0;
    let mut rgba = vec![0u8; 160 * 90 * 4];
    let len = rgba.len();
    overlay.compose_test_overlay(&mut rgba, 160, 90, 0.25);
    assert_eq!(rgba.len(), len);
    assert!(rgba.iter().any(|&v| v != 0));
}

#[test]
fn drag_delta_moves_overlay_in_preview_percent_space() {
    let mut overlay = OverlaySource::default();
    overlay.x_percent = 50.0;
    overlay.y_percent = 50.0;
    overlay.move_by_preview_delta(30.0, -15.0, 300.0, 150.0, 1920, 1080);
    assert!((overlay.x_percent - 60.0).abs() < 0.001);
    assert!((overlay.y_percent - 40.0).abs() < 0.001);
}

#[test]
fn overlay_drag_is_clamped_inside_frame() {
    let mut overlay = OverlaySource::default();
    overlay.width_percent = 20.0;
    overlay.x_percent = 50.0;
    overlay.y_percent = 50.0;
    overlay.move_by_preview_delta(-1000.0, 1000.0, 300.0, 150.0, 1920, 1080);
    assert!(overlay.x_percent >= 10.0);
    assert!(overlay.y_percent < 100.0);
}

#[test]
fn resize_delta_changes_width_and_respects_limits() {
    let mut overlay = OverlaySource::default();
    overlay.width_percent = 16.0;
    overlay.resize_by_preview_delta(30.0, 300.0, 1920, 1080);
    assert!((overlay.width_percent - 26.0).abs() < 0.001);
    overlay.resize_by_preview_delta(1000.0, 300.0, 1920, 1080);
    assert!((overlay.width_percent - 45.0).abs() < 0.001);
    overlay.resize_by_preview_delta(-1000.0, 300.0, 1920, 1080);
    assert!((overlay.width_percent - 5.0).abs() < 0.001);
}
