use stream_recording::{choose_h264_encoder, EncoderPreference, H264Encoder};

#[test]
fn encoder_prefers_nvenc_then_amf_then_qsv_then_cpu() {
    assert_eq!(
        choose_h264_encoder("h264_qsv h264_amf h264_nvenc libx264"),
        H264Encoder::Nvenc
    );
    assert_eq!(
        choose_h264_encoder("h264_qsv h264_amf libx264"),
        H264Encoder::Amf
    );
    assert_eq!(
        choose_h264_encoder("h264_qsv libx264"),
        H264Encoder::QuickSync
    );
    assert_eq!(choose_h264_encoder("libx264"), H264Encoder::Cpu);
}

#[test]
fn encoder_name_matches_ffmpeg_codec_name() {
    assert_eq!(H264Encoder::Nvenc.ffmpeg_name(), "h264_nvenc");
    assert_eq!(H264Encoder::Amf.ffmpeg_name(), "h264_amf");
    assert_eq!(H264Encoder::QuickSync.ffmpeg_name(), "h264_qsv");
    assert_eq!(H264Encoder::Cpu.ffmpeg_name(), "libx264");
}

#[test]
fn amd_amf_can_be_explicitly_requested() {
    assert_eq!(EncoderPreference::Amf, EncoderPreference::Amf);
    assert_eq!(H264Encoder::Amf.display_name(), "AMD AMF");
}
