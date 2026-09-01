use stream_audio::{mixed_levels, MixerControl, MixerSettings};

#[test]
fn mixer_defaults_to_unity_and_unmuted() {
    let settings = MixerSettings::default();

    assert_eq!(settings.mic_gain, 1.0);
    assert_eq!(settings.desktop_gain, 1.0);
    assert_eq!(settings.master_gain, 1.0);
    assert!(!settings.mic_muted);
    assert!(!settings.desktop_muted);
}

#[test]
fn mixer_gains_are_clamped_to_zero_through_one() {
    let mut settings = MixerSettings::default();
    settings.set_mic_gain(1.5);
    settings.set_desktop_gain(-0.25);
    settings.set_master_gain(2.0);

    assert_eq!(settings.mic_gain, 1.0);
    assert_eq!(settings.desktop_gain, 0.0);
    assert_eq!(settings.master_gain, 1.0);
}

#[test]
fn mute_zeroes_only_the_mix_path() {
    let settings = MixerSettings {
        mic_gain: 1.0,
        desktop_gain: 1.0,
        master_gain: 1.0,
        mic_muted: true,
        desktop_muted: false,
    };

    let levels = mixed_levels(0.8, 0.5, settings);

    assert_eq!(levels.mic, 0.0);
    assert_eq!(levels.desktop, 0.5);
    assert_eq!(levels.mix, 0.5);
}

#[test]
fn master_gain_is_applied_after_channel_mix() {
    let settings = MixerSettings {
        mic_gain: 0.5,
        desktop_gain: 0.5,
        master_gain: 0.5,
        mic_muted: false,
        desktop_muted: false,
    };

    let levels = mixed_levels(0.6, 0.4, settings);

    assert!((levels.mic - 0.3).abs() < 0.0001);
    assert!((levels.desktop - 0.2).abs() < 0.0001);
    assert!((levels.mix - 0.25).abs() < 0.0001);
}

#[test]
fn mixer_control_is_shared_between_clones() {
    let control = MixerControl::default();
    let clone = control.clone();

    control.set_mic_gain(0.42);
    control.set_desktop_muted(true);

    let snapshot = clone.snapshot();
    assert!((snapshot.mic_gain - 0.42).abs() < 0.0001);
    assert!(snapshot.desktop_muted);
}
