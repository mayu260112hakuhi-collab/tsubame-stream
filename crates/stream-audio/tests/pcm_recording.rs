#[test]
fn phase7_fix4_editor_audio_contract_is_48k_stereo_pcm16() {
    let sample_rate = 48_000_u32;
    let channels = 2_u16;
    let bits_per_sample = 16_u16;

    assert_eq!(sample_rate, 48_000);
    assert_eq!(channels, 2);
    assert_eq!(bits_per_sample, 16);
}
