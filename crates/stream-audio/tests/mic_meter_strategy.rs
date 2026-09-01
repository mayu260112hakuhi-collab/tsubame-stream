use stream_audio::{meter_strategy_for, AudioMeterStrategy, AudioMeterTarget};

#[test]
fn microphone_meter_uses_active_capture_stream() {
    assert_eq!(
        meter_strategy_for(AudioMeterTarget::Microphone),
        AudioMeterStrategy::ActiveCapture,
    );
}

#[test]
fn desktop_meter_keeps_endpoint_peak_strategy() {
    assert_eq!(
        meter_strategy_for(AudioMeterTarget::Desktop),
        AudioMeterStrategy::EndpointPeak,
    );
}
