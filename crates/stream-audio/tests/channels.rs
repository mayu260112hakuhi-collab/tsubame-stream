use stream_audio::{AudioChannel, AudioChannelKind, ChannelMixerControl};

#[test]
fn default_channels_are_desktop_and_microphone() {
    let mixer = ChannelMixerControl::default();
    let channels = mixer.channels();
    assert_eq!(channels.len(), 2);
    assert_eq!(channels[0].kind, AudioChannelKind::Desktop);
    assert_eq!(channels[1].kind, AudioChannelKind::Microphone);
}

#[test]
fn custom_channel_can_be_added_removed_and_configured() {
    let mixer = ChannelMixerControl::default();
    let id = mixer.add_channel(AudioChannel::custom("追加音声"));
    assert!(mixer.set_gain(id, 0.42));
    assert!(mixer.set_muted(id, true));
    assert!(mixer.set_include_in_stream_mix(id, false));
    assert!(mixer.set_record_individual(id, true));

    let channel = mixer.channel(id).unwrap();
    assert!((channel.gain - 0.42).abs() < 0.0001);
    assert!(channel.muted);
    assert!(!channel.include_in_stream_mix);
    assert!(channel.record_individual);
    assert!(mixer.remove_channel(id));
    assert!(mixer.channel(id).is_none());
}

#[test]
fn built_in_channels_cannot_be_removed() {
    let mixer = ChannelMixerControl::default();
    assert!(!mixer.remove_channel(ChannelMixerControl::DESKTOP_ID));
    assert!(!mixer.remove_channel(ChannelMixerControl::MICROPHONE_ID));
}

#[test]
fn master_gain_is_clamped() {
    let mixer = ChannelMixerControl::default();
    mixer.set_master_gain(2.0);
    assert_eq!(mixer.master_gain(), 1.0);
    mixer.set_master_gain(-1.0);
    assert_eq!(mixer.master_gain(), 0.0);
}
