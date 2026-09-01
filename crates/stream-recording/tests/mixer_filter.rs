use stream_audio::MixerSettings;
use stream_recording::build_mix_filter;

#[test]
fn final_mp4_filter_applies_channel_and_master_gains() {
    let settings = MixerSettings {
        mic_gain: 0.75,
        desktop_gain: 0.25,
        master_gain: 0.80,
        mic_muted: false,
        desktop_muted: false,
    };

    let filter = build_mix_filter(settings);

    assert!(filter.contains("[1:a]volume=0.7500[mic]"));
    assert!(filter.contains("[2:a]volume=0.2500[desktop]"));
    assert!(filter.contains("amix=inputs=2:duration=longest:normalize=0"));
    assert!(filter.contains("volume=0.8000"));
    assert!(filter.contains("alimiter=limit=0.98"));
}

#[test]
fn muted_channel_uses_zero_gain_in_final_mp4() {
    let settings = MixerSettings {
        mic_gain: 1.0,
        desktop_gain: 1.0,
        master_gain: 1.0,
        mic_muted: true,
        desktop_muted: false,
    };

    let filter = build_mix_filter(settings);

    assert!(filter.contains("[1:a]volume=0.0000[mic]"));
    assert!(filter.contains("[2:a]volume=1.0000[desktop]"));
}
