use crate::AudioError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioDeviceKind {
    Input,
    Output,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioDeviceInfo {
    pub id: String,
    pub name: String,
    pub kind: AudioDeviceKind,
    pub is_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum AudioDeviceSelection {
    #[default]
    Default,
    DeviceId(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AudioDeviceState {
    pub input: AudioDeviceSelection,
    pub output: AudioDeviceSelection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioDeviceConnectionState {
    Connected,
    Disconnected,
    Reconnecting,
}

impl Default for AudioDeviceConnectionState {
    fn default() -> Self {
        Self::Reconnecting
    }
}

pub fn selection_label(selection: &AudioDeviceSelection, devices: &[AudioDeviceInfo]) -> String {
    match selection {
        AudioDeviceSelection::Default => "Windows既定".to_owned(),
        AudioDeviceSelection::DeviceId(id) => devices
            .iter()
            .find(|device| &device.id == id)
            .map(|device| device.name.clone())
            .unwrap_or_else(|| "切断されたデバイス".to_owned()),
    }
}

#[cfg(windows)]
fn enumerate(
    direction: wasapi::Direction,
    kind: AudioDeviceKind,
) -> Result<Vec<AudioDeviceInfo>, AudioError> {
    use wasapi::{deinitialize, initialize_mta, DeviceEnumerator, Role};

    initialize_mta()
        .ok()
        .map_err(|e| AudioError::Backend(format!("COM初期化失敗: {e:?}")))?;

    let result = (|| {
        let enumerator = DeviceEnumerator::new().map_err(|e| AudioError::Backend(e.to_string()))?;
        let default_id = match kind {
            AudioDeviceKind::Input => enumerator
                .get_default_device_for_role(&direction, &Role::Communications)
                .or_else(|_| enumerator.get_default_device(&direction)),
            AudioDeviceKind::Output => enumerator
                .get_default_device_for_role(&direction, &Role::Multimedia)
                .or_else(|_| enumerator.get_default_device(&direction)),
        }
        .ok()
        .and_then(|device| device.get_id().ok());
        let collection = enumerator
            .get_device_collection(&direction)
            .map_err(|e| AudioError::Backend(e.to_string()))?;

        let mut devices = Vec::new();
        for device_result in &collection {
            let device = device_result.map_err(|e| AudioError::Backend(e.to_string()))?;
            let id = device
                .get_id()
                .map_err(|e| AudioError::Backend(e.to_string()))?;
            let name = device
                .get_friendlyname()
                .unwrap_or_else(|_| "名称不明のオーディオデバイス".to_owned());
            let is_default = default_id.as_deref() == Some(id.as_str());
            devices.push(AudioDeviceInfo {
                id,
                name,
                kind,
                is_default,
            });
        }

        devices.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(devices)
    })();

    deinitialize();
    result
}

pub fn enumerate_input_devices() -> Result<Vec<AudioDeviceInfo>, AudioError> {
    #[cfg(windows)]
    {
        return std::thread::Builder::new()
            .name("yaoyorozu-enumerate-audio-inputs".to_owned())
            .spawn(|| enumerate(wasapi::Direction::Capture, AudioDeviceKind::Input))
            .map_err(|e| AudioError::Backend(e.to_string()))?
            .join()
            .map_err(|_| {
                AudioError::Backend("入力デバイス列挙スレッドがpanicしました".to_owned())
            })?;
    }

    #[cfg(not(windows))]
    {
        Err(AudioError::UnsupportedPlatform)
    }
}

pub fn enumerate_output_devices() -> Result<Vec<AudioDeviceInfo>, AudioError> {
    #[cfg(windows)]
    {
        return std::thread::Builder::new()
            .name("yaoyorozu-enumerate-audio-outputs".to_owned())
            .spawn(|| enumerate(wasapi::Direction::Render, AudioDeviceKind::Output))
            .map_err(|e| AudioError::Backend(e.to_string()))?
            .join()
            .map_err(|_| {
                AudioError::Backend("出力デバイス列挙スレッドがpanicしました".to_owned())
            })?;
    }

    #[cfg(not(windows))]
    {
        Err(AudioError::UnsupportedPlatform)
    }
}
