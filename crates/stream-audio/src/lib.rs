pub mod application;
pub mod device;
pub use application::{
    application_source_label, enumerate_application_audio_sources, sort_application_sources,
    ApplicationAudioSource,
};
pub use device::{
    enumerate_input_devices, enumerate_output_devices, selection_label, AudioDeviceConnectionState,
    AudioDeviceInfo, AudioDeviceKind, AudioDeviceSelection, AudioDeviceState,
};

use std::{
    collections::HashMap,
    fmt,
    net::UdpSocket,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioLevels {
    pub mic: f32,
    pub desktop: f32,
    pub mix: f32,
}

impl Default for AudioLevels {
    fn default() -> Self {
        Self {
            mic: 0.0,
            desktop: 0.0,
            mix: 0.0,
        }
    }
}

impl AudioLevels {
    pub fn new(mic: f32, desktop: f32, mix: f32) -> Self {
        Self {
            mic: mic.clamp(0.0, 1.0),
            desktop: desktop.clamp(0.0, 1.0),
            mix: mix.clamp(0.0, 1.0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioChannelKind {
    Desktop,
    Microphone,
    Custom,
    Application,
}

pub type AudioChannelId = u64;

#[derive(Debug, Clone, PartialEq)]
pub struct AudioChannel {
    pub id: AudioChannelId,
    pub name: String,
    pub kind: AudioChannelKind,
    pub gain: f32,
    pub muted: bool,
    pub enabled: bool,
    pub record_individual: bool,
    pub include_in_stream_mix: bool,
    pub current_level: f32,
    pub source_id: Option<String>,
    pub process_id: Option<u32>,
}

impl AudioChannel {
    pub fn custom(name: impl Into<String>) -> Self {
        Self {
            id: 0,
            name: name.into(),
            kind: AudioChannelKind::Custom,
            gain: 1.0,
            muted: false,
            enabled: true,
            record_individual: false,
            include_in_stream_mix: true,
            current_level: 0.0,
            source_id: None,
            process_id: None,
        }
    }

    pub fn application(source: &ApplicationAudioSource) -> Self {
        Self {
            id: 0,
            name: if source.is_discord() {
                "Discord".to_owned()
            } else if source.display_name.trim().is_empty() {
                source.process_name.clone()
            } else {
                source.display_name.clone()
            },
            kind: AudioChannelKind::Application,
            gain: 1.0,
            muted: false,
            enabled: true,
            record_individual: true,
            include_in_stream_mix: true,
            current_level: 0.0,
            source_id: Some(format!("application:pid:{}", source.capture_process_id)),
            process_id: Some(source.capture_process_id),
        }
    }
}

#[derive(Debug)]
struct ChannelMixerState {
    channels: Vec<AudioChannel>,
    next_id: AudioChannelId,
    master_gain: f32,
}

#[derive(Clone)]
pub struct ChannelMixerControl {
    inner: Arc<Mutex<ChannelMixerState>>,
}

impl Default for ChannelMixerControl {
    fn default() -> Self {
        let channels = vec![
            AudioChannel {
                id: Self::DESKTOP_ID,
                name: "PC音声".to_owned(),
                kind: AudioChannelKind::Desktop,
                gain: 1.0,
                muted: false,
                enabled: true,
                record_individual: true,
                include_in_stream_mix: true,
                current_level: 0.0,
                source_id: Some("desktop".to_owned()),
                process_id: None,
            },
            AudioChannel {
                id: Self::MICROPHONE_ID,
                name: "マイク".to_owned(),
                kind: AudioChannelKind::Microphone,
                gain: 1.0,
                muted: false,
                enabled: true,
                record_individual: true,
                include_in_stream_mix: true,
                current_level: 0.0,
                source_id: Some("microphone".to_owned()),
                process_id: None,
            },
        ];
        Self {
            inner: Arc::new(Mutex::new(ChannelMixerState {
                channels,
                next_id: 100,
                master_gain: 1.0,
            })),
        }
    }
}

impl ChannelMixerControl {
    pub const DESKTOP_ID: AudioChannelId = 1;
    pub const MICROPHONE_ID: AudioChannelId = 2;

    pub fn channels(&self) -> Vec<AudioChannel> {
        self.inner
            .lock()
            .map(|s| s.channels.clone())
            .unwrap_or_default()
    }

    pub fn channel(&self, id: AudioChannelId) -> Option<AudioChannel> {
        self.inner
            .lock()
            .ok()?
            .channels
            .iter()
            .find(|c| c.id == id)
            .cloned()
    }

    pub fn add_channel(&self, mut channel: AudioChannel) -> AudioChannelId {
        if let Ok(mut state) = self.inner.lock() {
            let id = state.next_id;
            state.next_id += 1;
            channel.id = id;
            state.channels.push(channel);
            id
        } else {
            0
        }
    }

    pub fn add_application_channel(&self, source: &ApplicationAudioSource) -> AudioChannelId {
        self.add_channel(AudioChannel::application(source))
    }

    pub fn remove_channel(&self, id: AudioChannelId) -> bool {
        if id == Self::DESKTOP_ID || id == Self::MICROPHONE_ID {
            return false;
        }
        if let Ok(mut state) = self.inner.lock() {
            let before = state.channels.len();
            state.channels.retain(|c| c.id != id);
            return state.channels.len() != before;
        }
        false
    }

    fn update(&self, id: AudioChannelId, f: impl FnOnce(&mut AudioChannel)) -> bool {
        if let Ok(mut state) = self.inner.lock() {
            if let Some(channel) = state.channels.iter_mut().find(|c| c.id == id) {
                f(channel);
                return true;
            }
        }
        false
    }

    pub fn set_gain(&self, id: AudioChannelId, gain: f32) -> bool {
        self.update(id, |c| c.gain = gain.clamp(0.0, 1.0))
    }
    pub fn set_muted(&self, id: AudioChannelId, muted: bool) -> bool {
        self.update(id, |c| c.muted = muted)
    }
    pub fn set_include_in_stream_mix(&self, id: AudioChannelId, value: bool) -> bool {
        self.update(id, |c| c.include_in_stream_mix = value)
    }
    pub fn set_record_individual(&self, id: AudioChannelId, value: bool) -> bool {
        self.update(id, |c| c.record_individual = value)
    }
    pub fn set_level(&self, id: AudioChannelId, level: f32) -> bool {
        self.update(id, |c| c.current_level = level.clamp(0.0, 1.0))
    }
    pub fn set_enabled(&self, id: AudioChannelId, enabled: bool) -> bool {
        self.update(id, |c| c.enabled = enabled)
    }
    pub fn master_gain(&self) -> f32 {
        self.inner.lock().map(|s| s.master_gain).unwrap_or(1.0)
    }
    pub fn set_master_gain(&self, gain: f32) {
        if let Ok(mut state) = self.inner.lock() {
            state.master_gain = gain.clamp(0.0, 1.0);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MixerSettings {
    pub mic_gain: f32,
    pub desktop_gain: f32,
    pub master_gain: f32,
    pub mic_muted: bool,
    pub desktop_muted: bool,
}

impl Default for MixerSettings {
    fn default() -> Self {
        Self {
            mic_gain: 1.0,
            desktop_gain: 1.0,
            master_gain: 1.0,
            mic_muted: false,
            desktop_muted: false,
        }
    }
}

impl MixerSettings {
    pub fn set_mic_gain(&mut self, gain: f32) {
        self.mic_gain = gain.clamp(0.0, 1.0);
    }

    pub fn set_desktop_gain(&mut self, gain: f32) {
        self.desktop_gain = gain.clamp(0.0, 1.0);
    }

    pub fn set_master_gain(&mut self, gain: f32) {
        self.master_gain = gain.clamp(0.0, 1.0);
    }

    pub fn effective_mic_gain(self) -> f32 {
        if self.mic_muted {
            0.0
        } else {
            self.mic_gain.clamp(0.0, 1.0)
        }
    }

    pub fn effective_desktop_gain(self) -> f32 {
        if self.desktop_muted {
            0.0
        } else {
            self.desktop_gain.clamp(0.0, 1.0)
        }
    }
}

#[derive(Clone)]
pub struct MixerControl {
    inner: Arc<Mutex<MixerSettings>>,
}

impl Default for MixerControl {
    fn default() -> Self {
        Self::new(MixerSettings::default())
    }
}

impl MixerControl {
    pub fn new(settings: MixerSettings) -> Self {
        Self {
            inner: Arc::new(Mutex::new(settings)),
        }
    }

    pub fn snapshot(&self) -> MixerSettings {
        self.inner.lock().map(|v| *v).unwrap_or_default()
    }

    pub fn set_mic_gain(&self, gain: f32) {
        if let Ok(mut settings) = self.inner.lock() {
            settings.set_mic_gain(gain);
        }
    }

    pub fn set_desktop_gain(&self, gain: f32) {
        if let Ok(mut settings) = self.inner.lock() {
            settings.set_desktop_gain(gain);
        }
    }

    pub fn set_master_gain(&self, gain: f32) {
        if let Ok(mut settings) = self.inner.lock() {
            settings.set_master_gain(gain);
        }
    }

    pub fn set_mic_muted(&self, muted: bool) {
        if let Ok(mut settings) = self.inner.lock() {
            settings.mic_muted = muted;
        }
    }

    pub fn set_desktop_muted(&self, muted: bool) {
        if let Ok(mut settings) = self.inner.lock() {
            settings.desktop_muted = muted;
        }
    }
}

/// UI / streaming preview levels after channel gain, mute and master gain.
/// Editor WAV files do not use this path and remain raw.
pub fn mixed_levels(raw_mic: f32, raw_desktop: f32, settings: MixerSettings) -> AudioLevels {
    let mic = raw_mic.clamp(0.0, 1.0) * settings.effective_mic_gain();
    let desktop = raw_desktop.clamp(0.0, 1.0) * settings.effective_desktop_gain();

    // Real PCM mix is additive. Clamp the meter to full scale after Master.
    let mix = ((mic + desktop) * settings.master_gain.clamp(0.0, 1.0)).clamp(0.0, 1.0);

    AudioLevels::new(mic, desktop, mix)
}

/// Peak-only preview mix used by the UI meter.
/// Real sample mixing is added when the recording/encoder pipeline consumes PCM.
pub fn mix_peak(mic: f32, desktop: f32, mic_enabled: bool, desktop_enabled: bool) -> f32 {
    let mic = if mic_enabled {
        mic.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let desktop = if desktop_enabled {
        desktop.clamp(0.0, 1.0)
    } else {
        0.0
    };

    mic.max(desktop)
}

/// Convert a normalized peak to dBFS for a compact UI label.
pub fn dbfs(peak: f32) -> f32 {
    let peak = peak.clamp(0.0, 1.0);
    if peak <= 0.001 {
        -60.0
    } else {
        (20.0 * peak.log10()).max(-60.0)
    }
}

#[derive(Debug, Clone)]
pub struct AudioDeviceStatus {
    pub microphone: String,
    pub desktop: String,
    pub microphone_connection: AudioDeviceConnectionState,
    pub desktop_connection: AudioDeviceConnectionState,
}

impl Default for AudioDeviceStatus {
    fn default() -> Self {
        Self {
            microphone: "マイク: 再接続中".to_owned(),
            desktop: "PC音声: 再接続中".to_owned(),
            microphone_connection: AudioDeviceConnectionState::Reconnecting,
            desktop_connection: AudioDeviceConnectionState::Reconnecting,
        }
    }
}

#[derive(Debug, Clone)]
pub enum AudioError {
    UnsupportedPlatform,
    Backend(String),
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform => write!(f, "Windows以外ではWASAPIを利用できません"),
            Self::Backend(message) => write!(f, "WASAPIエラー: {message}"),
        }
    }
}

impl std::error::Error for AudioError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioMeterTarget {
    Microphone,
    Desktop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioMeterStrategy {
    ActiveCapture,
    EndpointPeak,
}

pub fn meter_strategy_for(target: AudioMeterTarget) -> AudioMeterStrategy {
    match target {
        AudioMeterTarget::Microphone => AudioMeterStrategy::ActiveCapture,
        AudioMeterTarget::Desktop => AudioMeterStrategy::EndpointPeak,
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct RawAudioLevels {
    mic: f32,
    desktop: f32,
}

struct ApplicationCaptureWorker {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ApplicationCaptureWorker {
    fn stop_and_join(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

#[derive(Debug, Clone)]
pub struct LiveAudioInput {
    pub id: AudioChannelId,
    pub name: String,
    pub port: u16,
}

pub struct LiveAudioBridge {
    pub inputs: Vec<LiveAudioInput>,
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<Result<(), AudioError>>>,
}

impl LiveAudioBridge {
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for handle in self.threads.drain(..) {
            let _ = handle.join();
        }
    }
}

impl Drop for LiveAudioBridge {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for handle in self.threads.drain(..) {
            let _ = handle.join();
        }
    }
}

pub struct AudioWorker {
    raw_levels: Arc<Mutex<RawAudioLevels>>,
    status: Arc<Mutex<AudioDeviceStatus>>,
    mixer: MixerControl,
    channel_mixer: ChannelMixerControl,
    device_state: Arc<Mutex<AudioDeviceState>>,
    input_devices: Arc<Mutex<Vec<AudioDeviceInfo>>>,
    output_devices: Arc<Mutex<Vec<AudioDeviceInfo>>>,
    application_sources: Arc<Mutex<Vec<ApplicationAudioSource>>>,
    application_captures: Mutex<HashMap<AudioChannelId, ApplicationCaptureWorker>>,
    stop: Arc<AtomicBool>,
    mic_thread: Option<JoinHandle<()>>,
    desktop_thread: Option<JoinHandle<()>>,
}

impl AudioWorker {
    pub fn start_default_devices() -> Result<Self, AudioError> {
        #[cfg(windows)]
        {
            let raw_levels = Arc::new(Mutex::new(RawAudioLevels::default()));
            let status = Arc::new(Mutex::new(AudioDeviceStatus::default()));
            let mixer = MixerControl::default();
            let channel_mixer = ChannelMixerControl::default();
            let device_state = Arc::new(Mutex::new(AudioDeviceState::default()));
            let input_devices = Arc::new(Mutex::new(enumerate_input_devices().unwrap_or_default()));
            let output_devices =
                Arc::new(Mutex::new(enumerate_output_devices().unwrap_or_default()));
            let application_sources = Arc::new(Mutex::new(Vec::new()));
            let application_captures = Mutex::new(HashMap::new());
            let stop = Arc::new(AtomicBool::new(false));

            let mic_handle = {
                let raw_levels = Arc::clone(&raw_levels);
                let status = Arc::clone(&status);
                let device_state = Arc::clone(&device_state);
                let stop = Arc::clone(&stop);
                thread::Builder::new()
                    .name("yaoyorozu-audio-meter-mic".to_owned())
                    .spawn(move || {
                        windows_impl::meter_path_loop(
                            windows_impl::MeterPath::Microphone,
                            raw_levels,
                            status,
                            device_state,
                            stop,
                        )
                    })
                    .map_err(|err| AudioError::Backend(err.to_string()))?
            };

            let desktop_handle = {
                let raw_levels = Arc::clone(&raw_levels);
                let status = Arc::clone(&status);
                let device_state = Arc::clone(&device_state);
                let stop = Arc::clone(&stop);
                thread::Builder::new()
                    .name("yaoyorozu-audio-meter-desktop".to_owned())
                    .spawn(move || {
                        windows_impl::meter_path_loop(
                            windows_impl::MeterPath::Desktop,
                            raw_levels,
                            status,
                            device_state,
                            stop,
                        )
                    })
                    .map_err(|err| AudioError::Backend(err.to_string()))?
            };

            Ok(Self {
                raw_levels,
                status,
                mixer,
                channel_mixer,
                device_state,
                input_devices,
                output_devices,
                application_sources,
                application_captures,
                stop,
                mic_thread: Some(mic_handle),
                desktop_thread: Some(desktop_handle),
            })
        }

        #[cfg(not(windows))]
        {
            Err(AudioError::UnsupportedPlatform)
        }
    }

    pub fn levels(&self) -> AudioLevels {
        let raw = self.raw_levels.lock().map(|v| *v).unwrap_or_default();
        let settings = self.mixer.snapshot();
        let base = mixed_levels(raw.mic, raw.desktop, settings);
        self.channel_mixer
            .set_level(ChannelMixerControl::DESKTOP_ID, base.desktop);
        self.channel_mixer
            .set_level(ChannelMixerControl::MICROPHONE_ID, base.mic);

        let application_sum: f32 = self
            .channel_mixer
            .channels()
            .into_iter()
            .filter(|channel| channel.kind == AudioChannelKind::Application)
            .filter(|channel| channel.enabled && channel.include_in_stream_mix && !channel.muted)
            .map(|channel| channel.current_level)
            .sum();
        let mix =
            ((base.mic + base.desktop + application_sum) * settings.master_gain).clamp(0.0, 1.0);

        AudioLevels::new(base.mic, base.desktop, mix)
    }

    pub fn channel_mixer(&self) -> ChannelMixerControl {
        self.channel_mixer.clone()
    }
    pub fn audio_channels(&self) -> Vec<AudioChannel> {
        self.channel_mixer.channels()
    }
    pub fn add_custom_channel(&self, name: impl Into<String>) -> AudioChannelId {
        self.channel_mixer.add_channel(AudioChannel::custom(name))
    }

    pub fn application_sources(&self) -> Vec<ApplicationAudioSource> {
        self.application_sources
            .lock()
            .map(|v| v.clone())
            .unwrap_or_default()
    }

    pub fn refresh_application_sources(&self) -> Result<(), AudioError> {
        let sources = enumerate_application_audio_sources()?;
        if let Ok(mut current) = self.application_sources.lock() {
            *current = sources;
        }
        Ok(())
    }

    pub fn add_application_channel(
        &self,
        source: ApplicationAudioSource,
    ) -> Result<AudioChannelId, AudioError> {
        if let Some(existing) = self.channel_mixer.channels().into_iter().find(|channel| {
            channel.kind == AudioChannelKind::Application
                && channel.process_id == Some(source.capture_process_id)
        }) {
            return Ok(existing.id);
        }

        let id = self.channel_mixer.add_application_channel(&source);
        if id == 0 {
            return Err(AudioError::Backend(
                "アプリ音声チャンネルを作成できませんでした".to_owned(),
            ));
        }

        #[cfg(windows)]
        {
            let stop = Arc::new(AtomicBool::new(false));
            let thread_stop = Arc::clone(&stop);
            let mixer = self.channel_mixer.clone();
            let pid = source.capture_process_id;
            let name = source.display_name.clone();
            let handle = thread::Builder::new()
                .name(format!("yaoyorozu-app-audio-{pid}"))
                .spawn(move || {
                    if windows_application::application_meter_loop(
                        pid,
                        id,
                        mixer.clone(),
                        thread_stop,
                    )
                    .is_err()
                    {
                        mixer.set_level(id, 0.0);
                        mixer.set_enabled(id, false);
                    }
                })
                .map_err(|e| {
                    self.channel_mixer.remove_channel(id);
                    AudioError::Backend(e.to_string())
                })?;

            if let Ok(mut captures) = self.application_captures.lock() {
                captures.insert(
                    id,
                    ApplicationCaptureWorker {
                        stop,
                        thread: Some(handle),
                    },
                );
            } else {
                stop.store(true, Ordering::Relaxed);
                let _ = handle.join();
                self.channel_mixer.remove_channel(id);
                return Err(AudioError::Backend(format!(
                    "{name} のキャプチャ状態を保存できませんでした"
                )));
            }
        }

        #[cfg(not(windows))]
        {
            let _ = source;
            self.channel_mixer.remove_channel(id);
            return Err(AudioError::UnsupportedPlatform);
        }

        Ok(id)
    }

    pub fn remove_audio_channel(&self, id: AudioChannelId) -> bool {
        if let Ok(mut captures) = self.application_captures.lock() {
            if let Some(worker) = captures.remove(&id) {
                drop(captures);
                worker.stop_and_join();
            }
        }
        self.channel_mixer.remove_channel(id)
    }
    pub fn set_channel_gain(&self, id: AudioChannelId, gain: f32) -> bool {
        let changed = self.channel_mixer.set_gain(id, gain);
        if id == ChannelMixerControl::DESKTOP_ID {
            self.mixer.set_desktop_gain(gain);
        }
        if id == ChannelMixerControl::MICROPHONE_ID {
            self.mixer.set_mic_gain(gain);
        }
        changed
    }
    pub fn set_channel_muted(&self, id: AudioChannelId, muted: bool) -> bool {
        let changed = self.channel_mixer.set_muted(id, muted);
        if id == ChannelMixerControl::DESKTOP_ID {
            self.mixer.set_desktop_muted(muted);
        }
        if id == ChannelMixerControl::MICROPHONE_ID {
            self.mixer.set_mic_muted(muted);
        }
        changed
    }
    pub fn set_channel_include_in_stream_mix(&self, id: AudioChannelId, value: bool) -> bool {
        self.channel_mixer.set_include_in_stream_mix(id, value)
    }
    pub fn set_channel_record_individual(&self, id: AudioChannelId, value: bool) -> bool {
        self.channel_mixer.set_record_individual(id, value)
    }

    pub fn start_live_audio_bridge(&self) -> Result<LiveAudioBridge, AudioError> {
        #[cfg(windows)]
        {
            let channels = self.channel_mixer.channels();
            let device_state = self.device_state();
            let stop = Arc::new(AtomicBool::new(false));
            let mut inputs = Vec::new();
            let mut threads: Vec<JoinHandle<Result<(), AudioError>>> = Vec::new();

            let next_port = || -> Result<u16, AudioError> {
                let socket = UdpSocket::bind("127.0.0.1:0").map_err(|e| {
                    AudioError::Backend(format!("ライブ音声UDPポート確保失敗: {e}"))
                })?;
                let port = socket
                    .local_addr()
                    .map_err(|e| AudioError::Backend(format!("ライブ音声UDPポート取得失敗: {e}")))?
                    .port();
                drop(socket);
                Ok(port)
            };

            for channel in channels {
                if !channel.enabled {
                    continue;
                }

                let port = next_port()?;
                let thread_stop = Arc::clone(&stop);
                let name = channel.name.clone();
                let channel_id = channel.id;
                let live_mixer = self.channel_mixer.clone();

                let handle = match channel.kind {
                    AudioChannelKind::Desktop => {
                        let selection = device_state.output.clone();
                        thread::Builder::new()
                            .name("yaoyorozu-live-desktop".to_owned())
                            .spawn(move || {
                                std::thread::sleep(std::time::Duration::from_millis(300));
                                windows_pcm::stream_loopback_udp(
                                    port,
                                    thread_stop,
                                    selection,
                                    live_mixer,
                                    channel_id,
                                )
                            })
                    }
                    AudioChannelKind::Microphone => {
                        let selection = device_state.input.clone();
                        thread::Builder::new()
                            .name("yaoyorozu-live-microphone".to_owned())
                            .spawn(move || {
                                std::thread::sleep(std::time::Duration::from_millis(300));
                                windows_pcm::stream_capture_udp(
                                    port,
                                    thread_stop,
                                    selection,
                                    live_mixer,
                                    channel_id,
                                )
                            })
                    }
                    AudioChannelKind::Application => {
                        let Some(pid) = channel.process_id else {
                            continue;
                        };
                        thread::Builder::new()
                            .name(format!("yaoyorozu-live-app-{pid}"))
                            .spawn(move || {
                                std::thread::sleep(std::time::Duration::from_millis(300));
                                windows_pcm::stream_application_udp(
                                    port,
                                    thread_stop,
                                    pid,
                                    live_mixer,
                                    channel_id,
                                )
                            })
                    }
                    AudioChannelKind::Custom => continue,
                }
                .map_err(|e| AudioError::Backend(format!("{name} ライブ音声開始失敗: {e}")))?;

                inputs.push(LiveAudioInput {
                    id: channel_id,
                    name,
                    port,
                });
                threads.push(handle);
            }

            Ok(LiveAudioBridge {
                inputs,
                stop,
                threads,
            })
        }

        #[cfg(not(windows))]
        {
            Err(AudioError::UnsupportedPlatform)
        }
    }

    pub fn status(&self) -> AudioDeviceStatus {
        self.status.lock().map(|v| v.clone()).unwrap_or_default()
    }

    pub fn mixer_control(&self) -> MixerControl {
        self.mixer.clone()
    }

    pub fn mixer_settings(&self) -> MixerSettings {
        self.mixer.snapshot()
    }

    pub fn device_state(&self) -> AudioDeviceState {
        self.device_state
            .lock()
            .map(|v| v.clone())
            .unwrap_or_default()
    }

    pub fn selected_input_device(&self) -> AudioDeviceSelection {
        self.device_state().input
    }

    pub fn selected_output_device(&self) -> AudioDeviceSelection {
        self.device_state().output
    }

    pub fn input_devices(&self) -> Vec<AudioDeviceInfo> {
        self.input_devices
            .lock()
            .map(|v| v.clone())
            .unwrap_or_default()
    }

    pub fn output_devices(&self) -> Vec<AudioDeviceInfo> {
        self.output_devices
            .lock()
            .map(|v| v.clone())
            .unwrap_or_default()
    }

    pub fn refresh_devices(&self) -> Result<(), AudioError> {
        let inputs = enumerate_input_devices()?;
        let outputs = enumerate_output_devices()?;
        if let Ok(mut current) = self.input_devices.lock() {
            *current = inputs;
        }
        if let Ok(mut current) = self.output_devices.lock() {
            *current = outputs;
        }
        Ok(())
    }

    pub fn set_input_device(&self, selection: AudioDeviceSelection) {
        if let Ok(mut raw) = self.raw_levels.lock() {
            raw.mic = 0.0;
        }
        if let Ok(mut state) = self.device_state.lock() {
            state.input = selection;
        }
        if let Ok(mut status) = self.status.lock() {
            status.microphone_connection = AudioDeviceConnectionState::Reconnecting;
            status.microphone = "マイク: 再接続中".to_owned();
        }
    }

    pub fn set_output_device(&self, selection: AudioDeviceSelection) {
        if let Ok(mut raw) = self.raw_levels.lock() {
            raw.desktop = 0.0;
        }
        if let Ok(mut state) = self.device_state.lock() {
            state.output = selection;
        }
        if let Ok(mut status) = self.status.lock() {
            status.desktop_connection = AudioDeviceConnectionState::Reconnecting;
            status.desktop = "PC音声: 再接続中".to_owned();
        }
    }

    pub fn mic_enabled(&self) -> bool {
        !self.mixer.snapshot().mic_muted
    }
    pub fn desktop_enabled(&self) -> bool {
        !self.mixer.snapshot().desktop_muted
    }
    pub fn set_mic_enabled(&self, enabled: bool) {
        self.mixer.set_mic_muted(!enabled);
    }
    pub fn set_desktop_enabled(&self, enabled: bool) {
        self.mixer.set_desktop_muted(!enabled);
    }
    pub fn set_mic_gain(&self, gain: f32) {
        self.mixer.set_mic_gain(gain);
        self.channel_mixer
            .set_gain(ChannelMixerControl::MICROPHONE_ID, gain);
    }
    pub fn set_desktop_gain(&self, gain: f32) {
        self.mixer.set_desktop_gain(gain);
        self.channel_mixer
            .set_gain(ChannelMixerControl::DESKTOP_ID, gain);
    }
    pub fn set_master_gain(&self, gain: f32) {
        self.mixer.set_master_gain(gain);
        self.channel_mixer.set_master_gain(gain);
    }
    pub fn set_mic_muted(&self, muted: bool) {
        self.mixer.set_mic_muted(muted);
        self.channel_mixer
            .set_muted(ChannelMixerControl::MICROPHONE_ID, muted);
    }
    pub fn set_desktop_muted(&self, muted: bool) {
        self.mixer.set_desktop_muted(muted);
        self.channel_mixer
            .set_muted(ChannelMixerControl::DESKTOP_ID, muted);
    }
}

impl Drop for AudioWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);

        if let Ok(mut captures) = self.application_captures.lock() {
            let workers: Vec<_> = captures.drain().map(|(_, worker)| worker).collect();
            drop(captures);
            for worker in workers {
                worker.stop_and_join();
            }
        }

        if let Some(handle) = self.mic_thread.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.desktop_thread.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(windows)]
mod windows_impl {
    use super::{
        AudioDeviceConnectionState, AudioDeviceSelection, AudioDeviceState, AudioDeviceStatus,
        RawAudioLevels,
    };
    use std::collections::VecDeque;
    use std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, Mutex,
        },
        thread,
        time::Duration,
    };
    use wasapi::{
        deinitialize, initialize_mta, AudioClient, Device, DeviceEnumerator, Direction, Role,
        SampleType, StreamMode, WaveFormat,
    };

    #[derive(Debug, Clone, Copy)]
    pub enum MeterPath {
        Microphone,
        Desktop,
    }

    fn current_selection(
        state: &Arc<Mutex<AudioDeviceState>>,
        path: MeterPath,
    ) -> AudioDeviceSelection {
        state
            .lock()
            .map(|state| match path {
                MeterPath::Microphone => state.input.clone(),
                MeterPath::Desktop => state.output.clone(),
            })
            .unwrap_or_default()
    }

    fn resolve_device(
        enumerator: &DeviceEnumerator,
        path: MeterPath,
        selection: &AudioDeviceSelection,
    ) -> Result<Device, String> {
        let direction = match path {
            MeterPath::Microphone => Direction::Capture,
            MeterPath::Desktop => Direction::Render,
        };

        match selection {
            AudioDeviceSelection::DeviceId(id) => {
                enumerator.get_device(id).map_err(|e| e.to_string())
            }
            AudioDeviceSelection::Default => match path {
                MeterPath::Microphone => enumerator
                    .get_default_device_for_role(&direction, &Role::Communications)
                    .or_else(|_| enumerator.get_default_device(&direction))
                    .map_err(|e| e.to_string()),
                MeterPath::Desktop => enumerator
                    .get_default_device_for_role(&direction, &Role::Multimedia)
                    .or_else(|_| enumerator.get_default_device(&direction))
                    .map_err(|e| e.to_string()),
            },
        }
    }

    fn set_status(
        status: &Arc<Mutex<AudioDeviceStatus>>,
        path: MeterPath,
        connection: AudioDeviceConnectionState,
        message: String,
    ) {
        if let Ok(mut status) = status.lock() {
            match path {
                MeterPath::Microphone => {
                    status.microphone_connection = connection;
                    status.microphone = message;
                }
                MeterPath::Desktop => {
                    status.desktop_connection = connection;
                    status.desktop = message;
                }
            }
        }
    }

    fn setup_microphone_meter(
        device: &Device,
    ) -> Result<
        (
            String,
            AudioClient,
            wasapi::AudioCaptureClient,
            wasapi::Handle,
            wasapi::AudioMeterInformation,
        ),
        String,
    > {
        let name = device
            .get_friendlyname()
            .unwrap_or_else(|_| "マイク".to_owned());
        let meter = device
            .get_audiometerinformation()
            .map_err(|e| e.to_string())?;
        let mut client = device.get_iaudioclient().map_err(|e| e.to_string())?;

        // Some capture endpoints (notably USB audio adapters) do not report
        // meaningful endpoint peak values until a capture stream is active.
        // Start a lightweight shared-mode capture stream for the live meter.
        let desired = WaveFormat::new(32, 32, &SampleType::Float, 48_000, 2, None);
        let (_default_period, min_period) =
            client.get_device_period().map_err(|e| e.to_string())?;
        let mode = StreamMode::EventsShared {
            autoconvert: true,
            buffer_duration_hns: min_period,
        };
        client
            .initialize_client(&desired, &Direction::Capture, &mode)
            .map_err(|e| e.to_string())?;
        let event = client.set_get_eventhandle().map_err(|e| e.to_string())?;
        let capture = client.get_audiocaptureclient().map_err(|e| e.to_string())?;
        client.start_stream().map_err(|e| e.to_string())?;

        Ok((name, client, capture, event, meter))
    }

    fn run_microphone_meter(
        raw_levels: &Arc<Mutex<RawAudioLevels>>,
        status: &Arc<Mutex<AudioDeviceStatus>>,
        device_state: &Arc<Mutex<AudioDeviceState>>,
        stop: &Arc<AtomicBool>,
        selected: &AudioDeviceSelection,
        device: &Device,
    ) -> Result<(), String> {
        let (name, client, capture, event, meter) = setup_microphone_meter(device)?;
        set_status(
            status,
            MeterPath::Microphone,
            AudioDeviceConnectionState::Connected,
            format!("マイク: {name}"),
        );

        let mut discard = VecDeque::<u8>::new();
        while !stop.load(Ordering::Relaxed)
            && current_selection(device_state, MeterPath::Microphone) == *selected
        {
            // Drain the capture buffer so the monitoring stream remains healthy.
            if event.wait_for_event(100).is_ok() {
                loop {
                    let frames = capture
                        .get_next_packet_size()
                        .map_err(|e| e.to_string())?
                        .unwrap_or(0);
                    if frames == 0 {
                        break;
                    }
                    capture
                        .read_from_device_to_deque(&mut discard)
                        .map_err(|e| e.to_string())?;
                    discard.clear();
                }
            }

            let peak = meter.get_peak_value().unwrap_or(0.0);
            if let Ok(mut raw) = raw_levels.lock() {
                raw.mic = peak;
            }
        }

        let _ = client.stop_stream();
        Ok(())
    }

    pub fn meter_path_loop(
        path: MeterPath,
        raw_levels: Arc<Mutex<RawAudioLevels>>,
        status: Arc<Mutex<AudioDeviceStatus>>,
        device_state: Arc<Mutex<AudioDeviceState>>,
        stop: Arc<AtomicBool>,
    ) {
        if initialize_mta().ok().is_err() {
            let prefix = match path {
                MeterPath::Microphone => "マイク",
                MeterPath::Desktop => "PC音声",
            };
            set_status(
                &status,
                path,
                AudioDeviceConnectionState::Disconnected,
                format!("{prefix}: COM初期化失敗"),
            );
            return;
        }

        while !stop.load(Ordering::Relaxed) {
            let selected = current_selection(&device_state, path);
            set_status(
                &status,
                path,
                AudioDeviceConnectionState::Reconnecting,
                match path {
                    MeterPath::Microphone => "マイク: 再接続中".to_owned(),
                    MeterPath::Desktop => "PC音声: 再接続中".to_owned(),
                },
            );

            let enumerator = match DeviceEnumerator::new() {
                Ok(value) => value,
                Err(error) => {
                    set_status(
                        &status,
                        path,
                        AudioDeviceConnectionState::Disconnected,
                        format!(
                            "{}: 切断 ({error})",
                            match path {
                                MeterPath::Microphone => "マイク",
                                MeterPath::Desktop => "PC音声",
                            }
                        ),
                    );
                    thread::sleep(Duration::from_millis(500));
                    continue;
                }
            };
            let device = match resolve_device(&enumerator, path, &selected) {
                Ok(value) => value,
                Err(error) => {
                    set_status(
                        &status,
                        path,
                        AudioDeviceConnectionState::Disconnected,
                        format!(
                            "{}: 切断 ({error})",
                            match path {
                                MeterPath::Microphone => "マイク",
                                MeterPath::Desktop => "PC音声",
                            }
                        ),
                    );
                    thread::sleep(Duration::from_millis(500));
                    continue;
                }
            };

            if matches!(path, MeterPath::Microphone) {
                if let Err(error) = run_microphone_meter(
                    &raw_levels,
                    &status,
                    &device_state,
                    &stop,
                    &selected,
                    &device,
                ) {
                    if let Ok(mut raw) = raw_levels.lock() {
                        raw.mic = 0.0;
                    }
                    set_status(
                        &status,
                        path,
                        AudioDeviceConnectionState::Disconnected,
                        format!("マイク: 切断 ({error})"),
                    );
                    thread::sleep(Duration::from_millis(500));
                }
                continue;
            }

            let name = device
                .get_friendlyname()
                .unwrap_or_else(|_| "出力デバイス".to_owned());
            let meter = match device.get_audiometerinformation() {
                Ok(value) => value,
                Err(error) => {
                    set_status(
                        &status,
                        path,
                        AudioDeviceConnectionState::Disconnected,
                        format!("PC音声: 切断 ({error})"),
                    );
                    thread::sleep(Duration::from_millis(500));
                    continue;
                }
            };

            set_status(
                &status,
                path,
                AudioDeviceConnectionState::Connected,
                format!("PC音声: {name}"),
            );

            while !stop.load(Ordering::Relaxed)
                && current_selection(&device_state, path) == selected
            {
                let peak = meter.get_peak_value().unwrap_or(0.0);
                if let Ok(mut raw) = raw_levels.lock() {
                    raw.desktop = peak;
                }
                thread::sleep(Duration::from_millis(33));
            }
        }

        deinitialize();
    }
}

#[cfg(windows)]
mod windows_application {
    use super::{AudioChannelId, AudioError, ChannelMixerControl};
    use std::{
        collections::VecDeque,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
    };
    use wasapi::{
        deinitialize, initialize_mta, AudioClient, Direction, SampleType, StreamMode, WaveFormat,
    };

    const SAMPLE_RATE: u32 = 48_000;
    const CHANNELS: usize = 2;

    fn peak_from_float32(queue: &mut VecDeque<u8>) -> f32 {
        let mut peak = 0.0_f32;
        while queue.len() >= 4 {
            let raw = [
                queue.pop_front().unwrap(),
                queue.pop_front().unwrap(),
                queue.pop_front().unwrap(),
                queue.pop_front().unwrap(),
            ];
            let sample = f32::from_le_bytes(raw);
            if sample.is_finite() {
                peak = peak.max(sample.abs().clamp(0.0, 1.0));
            }
        }
        peak
    }

    pub fn application_meter_loop(
        process_id: u32,
        channel_id: AudioChannelId,
        mixer: ChannelMixerControl,
        stop: Arc<AtomicBool>,
    ) -> Result<(), AudioError> {
        initialize_mta()
            .ok()
            .map_err(|e| AudioError::Backend(format!("COM初期化失敗: {e:?}")))?;

        let result = (|| {
            // wasapi-rs 0.24 wraps Windows' process-loopback activation API.
            // include_tree=true is important for Discord/Chromium multi-process apps.
            let mut client = AudioClient::new_application_loopback_client(process_id, true)
                .map_err(|e| AudioError::Backend(format!("アプリ音声開始失敗: {e}")))?;
            let desired = WaveFormat::new(
                32,
                32,
                &SampleType::Float,
                SAMPLE_RATE as usize,
                CHANNELS,
                None,
            );
            let mode = StreamMode::EventsShared {
                autoconvert: true,
                buffer_duration_hns: 0,
            };
            client
                .initialize_client(&desired, &Direction::Capture, &mode)
                .map_err(|e| AudioError::Backend(format!("アプリ音声初期化失敗: {e}")))?;
            let event = client
                .set_get_eventhandle()
                .map_err(|e| AudioError::Backend(format!("アプリ音声イベント取得失敗: {e}")))?;
            let capture = client.get_audiocaptureclient().map_err(|e| {
                AudioError::Backend(format!("アプリ音声CaptureClient取得失敗: {e}"))
            })?;
            client
                .start_stream()
                .map_err(|e| AudioError::Backend(format!("アプリ音声Start失敗: {e}")))?;

            let mut queue = VecDeque::<u8>::new();
            while !stop.load(Ordering::Relaxed) {
                let mut raw_peak = 0.0_f32;

                if event.wait_for_event(100).is_ok() {
                    loop {
                        let frames = capture
                            .get_next_packet_size()
                            .map_err(|e| AudioError::Backend(e.to_string()))?
                            .unwrap_or(0);
                        if frames == 0 {
                            break;
                        }

                        capture
                            .read_from_device_to_deque(&mut queue)
                            .map_err(|e| AudioError::Backend(e.to_string()))?;
                        raw_peak = raw_peak.max(peak_from_float32(&mut queue));
                    }
                }

                let adjusted = mixer
                    .channel(channel_id)
                    .map(|channel| {
                        if channel.enabled && !channel.muted {
                            raw_peak * channel.gain.clamp(0.0, 1.0)
                        } else {
                            0.0
                        }
                    })
                    .unwrap_or(0.0);
                mixer.set_level(channel_id, adjusted);
            }

            let _ = client.stop_stream();
            mixer.set_level(channel_id, 0.0);
            Ok(())
        })();

        deinitialize();
        result
    }
}

// -----------------------------------------------------------------------------
// PCM recording for editor-separated microphone / desktop tracks
// -----------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ApplicationRecordingPath {
    pub channel_id: AudioChannelId,
    pub process_id: u32,
    pub path: std::path::PathBuf,
}

#[derive(Debug, Clone)]
pub struct AudioRecordingPaths {
    pub microphone: std::path::PathBuf,
    pub desktop: std::path::PathBuf,
    pub applications: Vec<ApplicationRecordingPath>,
}

pub struct PcmRecordingWorker {
    stop: Arc<AtomicBool>,
    mic_thread: Option<JoinHandle<Result<(), AudioError>>>,
    desktop_thread: Option<JoinHandle<Result<(), AudioError>>>,
    application_threads: Vec<(AudioChannelId, JoinHandle<Result<(), AudioError>>)>,
}

impl PcmRecordingWorker {
    pub fn start(paths: AudioRecordingPaths) -> Result<Self, AudioError> {
        Self::start_with_devices(paths, AudioDeviceState::default())
    }

    pub fn start_with_devices(
        paths: AudioRecordingPaths,
        devices: AudioDeviceState,
    ) -> Result<Self, AudioError> {
        #[cfg(windows)]
        {
            let stop = Arc::new(AtomicBool::new(false));

            let mic_stop = Arc::clone(&stop);
            let mic_path = paths.microphone;
            let mic_selection = devices.input;
            let mic_thread = thread::Builder::new()
                .name("yaoyorozu-mic-recorder".to_owned())
                .spawn(move || windows_pcm::record_capture(&mic_path, mic_stop, mic_selection))
                .map_err(|e| AudioError::Backend(e.to_string()))?;

            let desktop_stop = Arc::clone(&stop);
            let desktop_path = paths.desktop;
            let desktop_selection = devices.output;
            let desktop_thread = thread::Builder::new()
                .name("yaoyorozu-desktop-recorder".to_owned())
                .spawn(move || {
                    windows_pcm::record_loopback(&desktop_path, desktop_stop, desktop_selection)
                })
                .map_err(|e| AudioError::Backend(e.to_string()))?;

            let mut application_threads: Vec<(AudioChannelId, JoinHandle<Result<(), AudioError>>)> =
                Vec::new();
            for application in paths.applications {
                let app_stop = Arc::clone(&stop);
                let channel_id = application.channel_id;
                let process_id = application.process_id;
                let path = application.path;
                let handle = match thread::Builder::new()
                    .name(format!("yaoyorozu-app-recorder-{process_id}"))
                    .spawn(move || {
                        windows_pcm::record_application_loopback(
                            &path, app_stop, process_id, channel_id,
                        )
                    }) {
                    Ok(handle) => handle,
                    Err(err) => {
                        stop.store(true, Ordering::Relaxed);
                        let _ = mic_thread.join();
                        let _ = desktop_thread.join();
                        for (_, handle) in application_threads {
                            let _ = handle.join();
                        }
                        return Err(AudioError::Backend(err.to_string()));
                    }
                };
                application_threads.push((channel_id, handle));
            }

            Ok(Self {
                stop,
                mic_thread: Some(mic_thread),
                desktop_thread: Some(desktop_thread),
                application_threads,
            })
        }

        #[cfg(not(windows))]
        {
            let _ = (paths, devices);
            Err(AudioError::UnsupportedPlatform)
        }
    }

    pub fn stop_and_join(mut self) -> Result<(), AudioError> {
        self.stop.store(true, Ordering::Relaxed);

        if let Some(handle) = self.mic_thread.take() {
            match handle.join() {
                Ok(result) => result?,
                Err(_) => {
                    return Err(AudioError::Backend(
                        "マイク録音スレッドがpanicしました".to_owned(),
                    ))
                }
            }
        }

        if let Some(handle) = self.desktop_thread.take() {
            match handle.join() {
                Ok(result) => result?,
                Err(_) => {
                    return Err(AudioError::Backend(
                        "PC音声録音スレッドがpanicしました".to_owned(),
                    ))
                }
            }
        }

        for (_channel_id, handle) in self.application_threads.drain(..) {
            // Application tracks are isolated: one app exiting or failing must
            // not make the microphone/desktop recording or final MP4 fail.
            let _ = handle.join();
        }

        Ok(())
    }
}

#[cfg(windows)]
mod windows_pcm {
    use super::{AudioChannelId, AudioDeviceSelection, AudioError, ChannelMixerControl};
    use hound::{SampleFormat, WavSpec, WavWriter};
    use std::{
        collections::VecDeque,
        net::UdpSocket,
        path::Path,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
    };
    use wasapi::{
        deinitialize, initialize_mta, AudioClient, DeviceEnumerator, Direction, Role, SampleType,
        StreamMode, WaveFormat,
    };

    const SAMPLE_RATE: u32 = 48_000;
    const CHANNELS: u16 = 2;

    fn wav_spec() -> WavSpec {
        WavSpec {
            channels: CHANNELS,
            sample_rate: SAMPLE_RATE,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        }
    }

    fn float32_queue_to_pcm16_bytes(queue: &mut VecDeque<u8>) -> Vec<u8> {
        let mut out = Vec::with_capacity(queue.len() / 2);
        while queue.len() >= 4 {
            let raw = [
                queue.pop_front().unwrap(),
                queue.pop_front().unwrap(),
                queue.pop_front().unwrap(),
                queue.pop_front().unwrap(),
            ];
            let sample = f32::from_le_bytes(raw);
            if !sample.is_finite() {
                continue;
            }
            let sample = sample.clamp(-1.0, 1.0);
            let pcm = if sample >= 0.0 {
                (sample * i16::MAX as f32).round() as i16
            } else {
                (sample * -(i16::MIN as f32)).round() as i16
            };
            out.extend_from_slice(&pcm.to_le_bytes());
        }
        out
    }

    fn apply_live_channel_control(
        bytes: &mut [u8],
        mixer: &ChannelMixerControl,
        channel_id: AudioChannelId,
    ) {
        let channel = mixer.channel(channel_id);
        let gain = match channel {
            Some(channel) if channel.enabled && channel.include_in_stream_mix && !channel.muted => {
                (channel.gain.clamp(0.0, 1.0) * mixer.master_gain().clamp(0.0, 1.0)).clamp(0.0, 1.0)
            }
            _ => 0.0,
        };

        if gain >= 0.9999 {
            return;
        }

        for sample in bytes.chunks_exact_mut(2) {
            let raw = i16::from_le_bytes([sample[0], sample[1]]);
            let scaled = ((raw as f32) * gain)
                .round()
                .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
            let encoded = scaled.to_le_bytes();
            sample[0] = encoded[0];
            sample[1] = encoded[1];
        }
    }

    fn send_pcm_chunks(socket: &UdpSocket, bytes: &[u8]) -> Result<(), AudioError> {
        const CHUNK: usize = 3840; // 20 ms @ 48 kHz, stereo, PCM16
        for chunk in bytes.chunks(CHUNK) {
            socket
                .send(chunk)
                .map_err(|e| AudioError::Backend(format!("ライブ音声UDP送信失敗: {e}")))?;
        }
        Ok(())
    }

    fn send_silence(socket: &UdpSocket) -> Result<(), AudioError> {
        let silence = [0_u8; 3840];
        socket
            .send(&silence)
            .map_err(|e| AudioError::Backend(format!("ライブ音声UDP無音送信失敗: {e}")))?;
        Ok(())
    }

    /// WASAPI is explicitly asked for 32-bit float samples. Convert those
    /// samples to conventional PCM16 WAV so normal media players/editors
    /// handle the files consistently.
    fn flush_float32_as_pcm16(
        writer: &mut WavWriter<std::io::BufWriter<std::fs::File>>,
        queue: &mut VecDeque<u8>,
    ) -> Result<(u64, f32), AudioError> {
        let mut written = 0_u64;
        let mut peak = 0.0_f32;

        while queue.len() >= 4 {
            let raw = [
                queue.pop_front().unwrap(),
                queue.pop_front().unwrap(),
                queue.pop_front().unwrap(),
                queue.pop_front().unwrap(),
            ];

            let sample = f32::from_le_bytes(raw);
            if !sample.is_finite() {
                continue;
            }

            let sample = sample.clamp(-1.0, 1.0);
            peak = peak.max(sample.abs());

            let pcm = if sample >= 0.0 {
                (sample * i16::MAX as f32).round() as i16
            } else {
                (sample * -(i16::MIN as f32)).round() as i16
            };

            writer
                .write_sample(pcm)
                .map_err(|e| AudioError::Backend(e.to_string()))?;
            written += 1;
        }

        Ok((written, peak))
    }

    pub fn stream_capture_udp(
        port: u16,
        stop: Arc<AtomicBool>,
        selection: AudioDeviceSelection,
        mixer: ChannelMixerControl,
        channel_id: AudioChannelId,
    ) -> Result<(), AudioError> {
        initialize_mta()
            .ok()
            .map_err(|e| AudioError::Backend(format!("COM初期化失敗: {e:?}")))?;
        let result = (|| {
            let enumerator =
                DeviceEnumerator::new().map_err(|e| AudioError::Backend(e.to_string()))?;
            let device = match selection {
                AudioDeviceSelection::Default => enumerator
                    .get_default_device_for_role(&Direction::Capture, &Role::Communications)
                    .or_else(|_| enumerator.get_default_device(&Direction::Capture)),
                AudioDeviceSelection::DeviceId(id) => enumerator.get_device(&id),
            }
            .map_err(|e| AudioError::Backend(e.to_string()))?;
            let mut client = device
                .get_iaudioclient()
                .map_err(|e| AudioError::Backend(e.to_string()))?;
            stream_client_udp(
                &mut client,
                port,
                stop,
                Direction::Capture,
                "microphone-live",
                mixer,
                channel_id,
            )
        })();
        deinitialize();
        result
    }

    pub fn stream_loopback_udp(
        port: u16,
        stop: Arc<AtomicBool>,
        selection: AudioDeviceSelection,
        mixer: ChannelMixerControl,
        channel_id: AudioChannelId,
    ) -> Result<(), AudioError> {
        initialize_mta()
            .ok()
            .map_err(|e| AudioError::Backend(format!("COM初期化失敗: {e:?}")))?;
        let result = (|| {
            let enumerator =
                DeviceEnumerator::new().map_err(|e| AudioError::Backend(e.to_string()))?;
            let device = match selection {
                AudioDeviceSelection::Default => enumerator
                    .get_default_device_for_role(&Direction::Render, &Role::Multimedia)
                    .or_else(|_| enumerator.get_default_device(&Direction::Render)),
                AudioDeviceSelection::DeviceId(id) => enumerator.get_device(&id),
            }
            .map_err(|e| AudioError::Backend(e.to_string()))?;
            let mut client = device
                .get_iaudioclient()
                .map_err(|e| AudioError::Backend(e.to_string()))?;
            stream_client_udp(
                &mut client,
                port,
                stop,
                Direction::Capture,
                "desktop-live",
                mixer,
                channel_id,
            )
        })();
        deinitialize();
        result
    }

    pub fn stream_application_udp(
        port: u16,
        stop: Arc<AtomicBool>,
        process_id: u32,
        mixer: ChannelMixerControl,
        channel_id: AudioChannelId,
    ) -> Result<(), AudioError> {
        initialize_mta()
            .ok()
            .map_err(|e| AudioError::Backend(format!("COM初期化失敗: {e:?}")))?;
        let result = (|| {
            let mut client = AudioClient::new_application_loopback_client(process_id, true)
                .map_err(|e| AudioError::Backend(format!("application-live client: {e}")))?;
            stream_application_client_udp(
                &mut client,
                port,
                stop,
                "application-live",
                mixer,
                channel_id,
            )
        })();
        deinitialize();
        result
    }

    fn stream_application_client_udp(
        client: &mut AudioClient,
        port: u16,
        stop: Arc<AtomicBool>,
        track_name: &str,
        mixer: ChannelMixerControl,
        channel_id: AudioChannelId,
    ) -> Result<(), AudioError> {
        let desired = WaveFormat::new(
            32,
            32,
            &SampleType::Float,
            SAMPLE_RATE as usize,
            CHANNELS as usize,
            None,
        );
        let block_align = desired.get_blockalign() as usize;
        let mode = StreamMode::EventsShared {
            autoconvert: true,
            buffer_duration_hns: 0,
        };
        client
            .initialize_client(&desired, &Direction::Capture, &mode)
            .map_err(|e| AudioError::Backend(format!("{track_name}: initialize: {e}")))?;
        let event = client
            .set_get_eventhandle()
            .map_err(|e| AudioError::Backend(format!("{track_name}: event: {e}")))?;
        let capture = client
            .get_audiocaptureclient()
            .map_err(|e| AudioError::Backend(format!("{track_name}: capture client: {e}")))?;
        let socket = UdpSocket::bind("127.0.0.1:0")
            .map_err(|e| AudioError::Backend(format!("{track_name}: UDP bind: {e}")))?;
        socket
            .connect(("127.0.0.1", port))
            .map_err(|e| AudioError::Backend(format!("{track_name}: UDP connect: {e}")))?;
        let mut queue = VecDeque::<u8>::new();
        client
            .start_stream()
            .map_err(|e| AudioError::Backend(format!("{track_name}: start: {e}")))?;
        while !stop.load(Ordering::Relaxed) {
            if event.wait_for_event(100).is_err() {
                send_silence(&socket)?;
                continue;
            }
            loop {
                let frames = capture
                    .get_next_packet_size()
                    .map_err(|e| AudioError::Backend(format!("{track_name}: packet size: {e}")))?
                    .unwrap_or(0);
                if frames == 0 {
                    break;
                }
                let additional = (frames as usize * block_align)
                    .saturating_sub(queue.capacity().saturating_sub(queue.len()));
                queue.reserve(additional);
                capture
                    .read_from_device_to_deque(&mut queue)
                    .map_err(|e| AudioError::Backend(format!("{track_name}: read: {e}")))?;
            }
            let mut bytes = float32_queue_to_pcm16_bytes(&mut queue);
            if bytes.is_empty() {
                send_silence(&socket)?;
            } else {
                apply_live_channel_control(&mut bytes, &mixer, channel_id);
                send_pcm_chunks(&socket, &bytes)?;
            }
        }
        let _ = client.stop_stream();
        Ok(())
    }

    fn stream_client_udp(
        client: &mut AudioClient,
        port: u16,
        stop: Arc<AtomicBool>,
        direction: Direction,
        track_name: &str,
        mixer: ChannelMixerControl,
        channel_id: AudioChannelId,
    ) -> Result<(), AudioError> {
        let desired = WaveFormat::new(
            32,
            32,
            &SampleType::Float,
            SAMPLE_RATE as usize,
            CHANNELS as usize,
            None,
        );
        let block_align = desired.get_blockalign() as usize;
        let (_default_period, min_period) = client
            .get_device_period()
            .map_err(|e| AudioError::Backend(format!("{track_name}: device period: {e}")))?;
        let mode = StreamMode::EventsShared {
            autoconvert: true,
            buffer_duration_hns: min_period,
        };
        client
            .initialize_client(&desired, &direction, &mode)
            .map_err(|e| AudioError::Backend(format!("{track_name}: initialize: {e}")))?;
        let event = client
            .set_get_eventhandle()
            .map_err(|e| AudioError::Backend(format!("{track_name}: event: {e}")))?;
        let buffer_frames = client
            .get_buffer_size()
            .map_err(|e| AudioError::Backend(format!("{track_name}: buffer size: {e}")))?;
        let capture = client
            .get_audiocaptureclient()
            .map_err(|e| AudioError::Backend(format!("{track_name}: capture client: {e}")))?;
        let mut queue =
            VecDeque::<u8>::with_capacity(100 * block_align * (1024 + 2 * buffer_frames as usize));
        let socket = UdpSocket::bind("127.0.0.1:0")
            .map_err(|e| AudioError::Backend(format!("{track_name}: UDP bind: {e}")))?;
        socket
            .connect(("127.0.0.1", port))
            .map_err(|e| AudioError::Backend(format!("{track_name}: UDP connect: {e}")))?;
        client
            .start_stream()
            .map_err(|e| AudioError::Backend(format!("{track_name}: start: {e}")))?;
        while !stop.load(Ordering::Relaxed) {
            if event.wait_for_event(100).is_err() {
                send_silence(&socket)?;
                continue;
            }
            loop {
                let frames = capture
                    .get_next_packet_size()
                    .map_err(|e| AudioError::Backend(format!("{track_name}: packet size: {e}")))?
                    .unwrap_or(0);
                if frames == 0 {
                    break;
                }
                let additional = (frames as usize * block_align)
                    .saturating_sub(queue.capacity().saturating_sub(queue.len()));
                queue.reserve(additional);
                capture
                    .read_from_device_to_deque(&mut queue)
                    .map_err(|e| AudioError::Backend(format!("{track_name}: read: {e}")))?;
            }
            let mut bytes = float32_queue_to_pcm16_bytes(&mut queue);
            if bytes.is_empty() {
                send_silence(&socket)?;
            } else {
                apply_live_channel_control(&mut bytes, &mixer, channel_id);
                send_pcm_chunks(&socket, &bytes)?;
            }
        }
        let _ = client.stop_stream();
        Ok(())
    }

    pub fn record_capture(
        path: &Path,
        stop: Arc<AtomicBool>,
        selection: AudioDeviceSelection,
    ) -> Result<(), AudioError> {
        initialize_mta()
            .ok()
            .map_err(|e| AudioError::Backend(format!("COM初期化失敗: {e:?}")))?;

        let result = (|| {
            let enumerator =
                DeviceEnumerator::new().map_err(|e| AudioError::Backend(e.to_string()))?;

            let device = match selection {
                AudioDeviceSelection::Default => enumerator
                    .get_default_device_for_role(&Direction::Capture, &Role::Communications)
                    .or_else(|_| enumerator.get_default_device(&Direction::Capture)),
                AudioDeviceSelection::DeviceId(id) => enumerator.get_device(&id),
            }
            .map_err(|e| AudioError::Backend(e.to_string()))?;

            let mut client = device
                .get_iaudioclient()
                .map_err(|e| AudioError::Backend(e.to_string()))?;

            record_client(&mut client, path, stop, Direction::Capture, "microphone")
        })();

        deinitialize();
        result
    }

    pub fn record_loopback(
        path: &Path,
        stop: Arc<AtomicBool>,
        selection: AudioDeviceSelection,
    ) -> Result<(), AudioError> {
        initialize_mta()
            .ok()
            .map_err(|e| AudioError::Backend(format!("COM初期化失敗: {e:?}")))?;

        let result = (|| {
            let enumerator =
                DeviceEnumerator::new().map_err(|e| AudioError::Backend(e.to_string()))?;

            // IMPORTANT:
            // Select the Render endpoint, then initialize that AudioClient with
            // Direction::Capture in shared mode. wasapi-rs detects this pair
            // and applies AUDCLNT_STREAMFLAGS_LOOPBACK.
            let device = match selection {
                AudioDeviceSelection::Default => enumerator
                    .get_default_device_for_role(&Direction::Render, &Role::Multimedia)
                    .or_else(|_| enumerator.get_default_device(&Direction::Render)),
                AudioDeviceSelection::DeviceId(id) => enumerator.get_device(&id),
            }
            .map_err(|e| AudioError::Backend(e.to_string()))?;

            let mut client = device
                .get_iaudioclient()
                .map_err(|e| AudioError::Backend(e.to_string()))?;

            record_client(&mut client, path, stop, Direction::Capture, "desktop")
        })();

        deinitialize();
        result
    }

    pub fn record_application_loopback(
        path: &Path,
        stop: Arc<AtomicBool>,
        process_id: u32,
        channel_id: super::AudioChannelId,
    ) -> Result<(), AudioError> {
        initialize_mta()
            .ok()
            .map_err(|e| AudioError::Backend(format!("COM初期化失敗: {e:?}")))?;

        let result = (|| {
            let mut client = AudioClient::new_application_loopback_client(process_id, true)
                .map_err(|e| {
                    AudioError::Backend(format!("application channel {channel_id}: client: {e}"))
                })?;

            record_application_client(
                &mut client,
                path,
                stop,
                &format!("application-{channel_id}"),
            )
        })();

        deinitialize();
        result
    }

    fn record_application_client(
        client: &mut AudioClient,
        path: &Path,
        stop: Arc<AtomicBool>,
        track_name: &str,
    ) -> Result<(), AudioError> {
        let desired = WaveFormat::new(
            32,
            32,
            &SampleType::Float,
            SAMPLE_RATE as usize,
            CHANNELS as usize,
            None,
        );
        let block_align = desired.get_blockalign() as usize;
        let mode = StreamMode::EventsShared {
            autoconvert: true,
            buffer_duration_hns: 0,
        };

        client
            .initialize_client(&desired, &Direction::Capture, &mode)
            .map_err(|e| AudioError::Backend(format!("{track_name}: initialize: {e}")))?;
        let event = client
            .set_get_eventhandle()
            .map_err(|e| AudioError::Backend(format!("{track_name}: event: {e}")))?;
        let capture = client
            .get_audiocaptureclient()
            .map_err(|e| AudioError::Backend(format!("{track_name}: capture client: {e}")))?;
        let mut queue = VecDeque::<u8>::new();
        let mut writer =
            WavWriter::create(path, wav_spec()).map_err(|e| AudioError::Backend(e.to_string()))?;

        client
            .start_stream()
            .map_err(|e| AudioError::Backend(format!("{track_name}: start: {e}")))?;

        let mut total_samples = 0_u64;
        while !stop.load(Ordering::Relaxed) {
            if event.wait_for_event(100).is_err() {
                continue;
            }
            loop {
                let frames = capture
                    .get_next_packet_size()
                    .map_err(|e| AudioError::Backend(format!("{track_name}: packet size: {e}")))?
                    .unwrap_or(0);
                if frames == 0 {
                    break;
                }
                let additional = (frames as usize * block_align)
                    .saturating_sub(queue.capacity().saturating_sub(queue.len()));
                queue.reserve(additional);
                capture
                    .read_from_device_to_deque(&mut queue)
                    .map_err(|e| AudioError::Backend(format!("{track_name}: read: {e}")))?;
            }
            let (written, _) = flush_float32_as_pcm16(&mut writer, &mut queue)?;
            total_samples += written;
        }

        loop {
            let frames = capture
                .get_next_packet_size()
                .map_err(|e| AudioError::Backend(format!("{track_name}: final packet: {e}")))?
                .unwrap_or(0);
            if frames == 0 {
                break;
            }
            let additional = (frames as usize * block_align)
                .saturating_sub(queue.capacity().saturating_sub(queue.len()));
            queue.reserve(additional);
            capture
                .read_from_device_to_deque(&mut queue)
                .map_err(|e| AudioError::Backend(format!("{track_name}: final read: {e}")))?;
        }
        let (written, _) = flush_float32_as_pcm16(&mut writer, &mut queue)?;
        total_samples += written;
        let _ = client.stop_stream();
        writer
            .finalize()
            .map_err(|e| AudioError::Backend(e.to_string()))?;

        if total_samples == 0 {
            return Err(AudioError::Backend(format!(
                "{track_name}: WASAPIからPCMサンプルを1件も取得できませんでした"
            )));
        }
        Ok(())
    }

    fn record_client(
        client: &mut AudioClient,
        path: &Path,
        stop: Arc<AtomicBool>,
        direction: Direction,
        track_name: &str,
    ) -> Result<(), AudioError> {
        let desired = WaveFormat::new(
            32,
            32,
            &SampleType::Float,
            SAMPLE_RATE as usize,
            CHANNELS as usize,
            None,
        );

        let block_align = desired.get_blockalign() as usize;

        // Use the actual endpoint's minimum period rather than an arbitrary
        // duration. This follows the crate's capture example.
        let (_default_period, min_period) = client
            .get_device_period()
            .map_err(|e| AudioError::Backend(format!("{track_name}: device period: {e}")))?;

        let mode = StreamMode::EventsShared {
            autoconvert: true,
            buffer_duration_hns: min_period,
        };

        client
            .initialize_client(&desired, &direction, &mode)
            .map_err(|e| AudioError::Backend(format!("{track_name}: initialize: {e}")))?;

        let event = client
            .set_get_eventhandle()
            .map_err(|e| AudioError::Backend(format!("{track_name}: event: {e}")))?;

        let buffer_frames = client
            .get_buffer_size()
            .map_err(|e| AudioError::Backend(format!("{track_name}: buffer size: {e}")))?;

        let capture = client
            .get_audiocaptureclient()
            .map_err(|e| AudioError::Backend(format!("{track_name}: capture client: {e}")))?;

        let mut queue =
            VecDeque::<u8>::with_capacity(100 * block_align * (1024 + 2 * buffer_frames as usize));

        let mut writer =
            WavWriter::create(path, wav_spec()).map_err(|e| AudioError::Backend(e.to_string()))?;

        client
            .start_stream()
            .map_err(|e| AudioError::Backend(format!("{track_name}: start: {e}")))?;

        let mut total_samples = 0_u64;
        let mut max_peak = 0.0_f32;

        while !stop.load(Ordering::Relaxed) {
            // Event-driven capture: wait until Windows says a buffer is ready.
            // A short timeout also lets Stop finish promptly.
            if event.wait_for_event(100).is_err() {
                continue;
            }

            loop {
                let frames = capture
                    .get_next_packet_size()
                    .map_err(|e| AudioError::Backend(format!("{track_name}: packet size: {e}")))?
                    .unwrap_or(0);

                if frames == 0 {
                    break;
                }

                let additional = (frames as usize * block_align)
                    .saturating_sub(queue.capacity().saturating_sub(queue.len()));
                queue.reserve(additional);

                capture
                    .read_from_device_to_deque(&mut queue)
                    .map_err(|e| AudioError::Backend(format!("{track_name}: read: {e}")))?;
            }

            let (written, peak) = flush_float32_as_pcm16(&mut writer, &mut queue)?;
            total_samples += written;
            max_peak = max_peak.max(peak);
        }

        // Drain anything already queued by the audio engine before closing.
        loop {
            let frames = capture
                .get_next_packet_size()
                .map_err(|e| AudioError::Backend(format!("{track_name}: final packet: {e}")))?
                .unwrap_or(0);

            if frames == 0 {
                break;
            }

            let additional = (frames as usize * block_align)
                .saturating_sub(queue.capacity().saturating_sub(queue.len()));
            queue.reserve(additional);

            capture
                .read_from_device_to_deque(&mut queue)
                .map_err(|e| AudioError::Backend(format!("{track_name}: final read: {e}")))?;
        }

        let (written, peak) = flush_float32_as_pcm16(&mut writer, &mut queue)?;
        total_samples += written;
        max_peak = max_peak.max(peak);

        let _ = client.stop_stream();

        writer
            .finalize()
            .map_err(|e| AudioError::Backend(e.to_string()))?;

        // A track containing actual samples but zero peak is still a valid
        // silent recording. Zero samples means capture itself failed to
        // deliver data and should be surfaced instead of silently succeeding.
        if total_samples == 0 {
            return Err(AudioError::Backend(format!(
                "{track_name}: WASAPIからPCMサンプルを1件も取得できませんでした"
            )));
        }

        // Write a tiny diagnostics sidecar next to each WAV. This makes future
        // device/driver problems diagnosable without guessing.
        let debug_path = path.with_extension("audio.txt");
        let duration_seconds = total_samples as f64 / CHANNELS as f64 / SAMPLE_RATE as f64;
        let peak_db = if max_peak <= 0.000_001 {
            -120.0
        } else {
            20.0 * (max_peak as f64).log10()
        };

        let debug = format!(
            "track={track_name}\n\
             sample_rate={SAMPLE_RATE}\n\
             channels={CHANNELS}\n\
             format=pcm_s16le\n\
             samples={total_samples}\n\
             duration_seconds={duration_seconds:.3}\n\
             peak={max_peak:.8}\n\
             peak_dbfs={peak_db:.2}\n"
        );

        std::fs::write(debug_path, debug)
            .map_err(|e| AudioError::Backend(format!("{track_name}: debug write: {e}")))?;

        Ok(())
    }
}
