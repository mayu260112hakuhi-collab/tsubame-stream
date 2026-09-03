use yaoyorozu_stream::streaming::{StreamingPlatform, StreamingTargetConfig};

#[test]
fn youtube_accepts_rtmp_and_rtmps() {
    for url in [
        "rtmp://x.rtmp.youtube.com/live2",
        "rtmps://a.rtmps.youtube.com/live2",
    ] {
        let mut target = StreamingTargetConfig::new(StreamingPlatform::YouTube);
        target.server_url = url.to_owned();
        target.stream_key = "test-key".to_owned();
        assert!(target.is_ready(), "url should be accepted: {url}");
    }
}

#[test]
fn provider_mismatch_is_rejected() {
    let mut twitch = StreamingTargetConfig::new(StreamingPlatform::Twitch);
    twitch.server_url = "rtmp://x.rtmp.youtube.com/live2".to_owned();
    twitch.stream_key = "test-key".to_owned();
    assert!(!twitch.is_ready());

    let mut youtube = StreamingTargetConfig::new(StreamingPlatform::YouTube);
    youtube.server_url = "rtmps://live.twitch.tv/app".to_owned();
    youtube.stream_key = "test-key".to_owned();
    assert!(!youtube.is_ready());
}

#[test]
fn switching_platform_resets_service_specific_fields() {
    let mut target = StreamingTargetConfig::new(StreamingPlatform::YouTube);
    target.server_url = "rtmp://x.rtmp.youtube.com/live2".to_owned();
    target.stream_key = "secret".to_owned();
    target.video_bitrate_kbps = 12_000;

    target.set_platform(StreamingPlatform::Twitch);

    assert_eq!(target.platform, StreamingPlatform::Twitch);
    assert!(target.server_url.is_empty());
    assert!(target.stream_key.is_empty());
    assert_eq!(target.video_bitrate_kbps, 6_000);
}
