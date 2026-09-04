use tsubame_stream::app::fit_aspect;

#[test]
fn preview_fit_preserves_16_by_9_inside_square() {
    let (w, h) = fit_aspect(1920.0, 1080.0, 1000.0, 1000.0);
    assert!((w - 1000.0).abs() < 0.01);
    assert!((h - 562.5).abs() < 0.01);
}

#[test]
fn preview_fit_never_exceeds_available_bounds() {
    let (w, h) = fit_aspect(1080.0, 1920.0, 800.0, 600.0);
    assert!(w <= 800.0);
    assert!(h <= 600.0);
}
