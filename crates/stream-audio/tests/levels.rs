use stream_audio::{dbfs, mix_peak, AudioLevels};

#[test]
fn mix_peak_uses_louder_enabled_path_without_clipping() {
    assert_eq!(mix_peak(0.25, 0.75, true, true), 0.75);
    assert_eq!(mix_peak(0.25, 0.75, true, false), 0.25);
    assert_eq!(mix_peak(0.25, 0.75, false, true), 0.75);
    assert_eq!(mix_peak(0.25, 0.75, false, false), 0.0);
}

#[test]
fn levels_are_clamped_to_unit_range() {
    let levels = AudioLevels::new(1.5, -0.1, 0.5);
    assert_eq!(levels.mic, 1.0);
    assert_eq!(levels.desktop, 0.0);
    assert_eq!(levels.mix, 0.5);
}

#[test]
fn dbfs_has_sensible_floor_and_full_scale() {
    assert!((dbfs(1.0) - 0.0).abs() < 0.001);
    assert_eq!(dbfs(0.0), -60.0);
}
