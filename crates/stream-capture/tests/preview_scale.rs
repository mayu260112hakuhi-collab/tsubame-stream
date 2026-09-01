use stream_capture::preview_dimensions;

#[test]
fn full_hd_preview_scales_to_960_by_540() {
    assert_eq!(preview_dimensions(1920, 1080), (960, 540));
}

#[test]
fn smaller_sources_are_not_upscaled() {
    assert_eq!(preview_dimensions(640, 360), (640, 360));
}

#[test]
fn non_16_by_9_sources_preserve_aspect_ratio() {
    assert_eq!(preview_dimensions(1600, 1200), (720, 540));
}

#[test]
fn zero_geometry_stays_zero() {
    assert_eq!(preview_dimensions(0, 1080), (0, 0));
}
