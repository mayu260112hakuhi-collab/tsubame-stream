use stream_audio::{
    AudioDeviceConnectionState, AudioDeviceInfo, AudioDeviceKind, AudioDeviceSelection,
    AudioDeviceState,
};

#[test]
fn audio_device_info_keeps_stable_id_and_display_name() {
    let device = AudioDeviceInfo {
        id: "device-123".to_owned(),
        name: "USB Microphone".to_owned(),
        kind: AudioDeviceKind::Input,
        is_default: false,
    };

    assert_eq!(device.id, "device-123");
    assert_eq!(device.name, "USB Microphone");
    assert_eq!(device.kind, AudioDeviceKind::Input);
}

#[test]
fn default_selection_is_explicit() {
    assert_eq!(
        AudioDeviceSelection::default(),
        AudioDeviceSelection::Default
    );
}

#[test]
fn input_and_output_selection_are_independent() {
    let state = AudioDeviceState {
        input: AudioDeviceSelection::DeviceId("mic".into()),
        output: AudioDeviceSelection::DeviceId("speaker".into()),
    };

    assert_ne!(state.input, state.output);
}

#[test]
fn connection_state_distinguishes_disconnect() {
    assert_ne!(
        AudioDeviceConnectionState::Connected,
        AudioDeviceConnectionState::Disconnected
    );
}
