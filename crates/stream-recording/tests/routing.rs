use std::path::PathBuf;
use stream_recording::{build_routed_mix_filter, FinalMixTrack};

fn track(name: &str, gain: f32, muted: bool, enabled: bool, in_mix: bool) -> FinalMixTrack {
    FinalMixTrack {
        path: PathBuf::from(name),
        gain,
        muted,
        enabled,
        include_in_stream_mix: in_mix,
    }
}

#[test]
fn routed_filter_skips_channels_not_in_stream_mix() {
    let tracks = vec![
        track("microphone.wav", 0.8, false, true, true),
        track("desktop.wav", 0.4, false, true, false),
        track("application_100_discord.wav", 0.6, false, true, true),
    ];

    let filter = build_routed_mix_filter(&tracks, 0.75).unwrap();
    assert!(filter.contains("[1:a]volume=0.8000[route0]"));
    assert!(!filter.contains("[2:a]volume="));
    assert!(filter.contains("[3:a]volume=0.6000[route1]"));
    assert!(filter.contains("amix=inputs=2:duration=longest:normalize=0"));
    assert!(filter.contains("volume=0.7500"));
}

#[test]
fn routed_filter_turns_muted_channel_into_zero_gain() {
    let tracks = vec![track("discord.wav", 0.9, true, true, true)];
    let filter = build_routed_mix_filter(&tracks, 1.0).unwrap();
    assert!(filter.contains("[1:a]volume=0.0000[route0]"));
    assert!(filter.contains("[route0]volume=1.0000"));
}

#[test]
fn routed_filter_returns_none_when_every_channel_is_excluded() {
    let tracks = vec![
        track("microphone.wav", 1.0, false, true, false),
        track("discord.wav", 1.0, false, false, true),
    ];
    assert!(build_routed_mix_filter(&tracks, 1.0).is_none());
}
