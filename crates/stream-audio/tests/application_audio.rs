use stream_audio::{
    application_source_label, sort_application_sources, ApplicationAudioSource, AudioChannelKind,
    ChannelMixerControl,
};

#[test]
fn discord_is_sorted_before_other_applications() {
    let mut sources = vec![
        ApplicationAudioSource::new(200, 200, "chrome.exe", "Chrome"),
        ApplicationAudioSource::new(100, 100, "Discord.exe", "Discord"),
    ];

    sort_application_sources(&mut sources);

    assert_eq!(sources[0].process_name.to_ascii_lowercase(), "discord.exe");
}

#[test]
fn application_source_label_marks_discord() {
    let discord = ApplicationAudioSource::new(100, 100, "Discord.exe", "Discord");
    assert!(application_source_label(&discord).contains("Discord"));
}

#[test]
fn application_channel_keeps_capture_process_id() {
    let mixer = ChannelMixerControl::default();
    let source = ApplicationAudioSource::new(42, 40, "Discord.exe", "Discord");
    let id = mixer.add_application_channel(&source);
    let channel = mixer.channel(id).expect("application channel");

    assert_eq!(channel.kind, AudioChannelKind::Application);
    assert_eq!(channel.process_id, Some(40));
    assert!(channel
        .source_id
        .as_deref()
        .unwrap_or_default()
        .contains("pid:40"));
}
