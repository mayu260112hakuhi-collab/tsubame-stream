use std::{
    env, fmt, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread::{self},
    time::{Duration, Instant},
};
use stream_audio::{
    ApplicationRecordingPath, AudioChannel, AudioChannelId, AudioChannelKind, AudioDeviceState,
    AudioRecordingPaths, ChannelMixerControl, MixerControl, MixerSettings, PcmRecordingWorker,
};
use stream_capture::{GpuRecordingConfig, GpuRecordingHandle, GpuRecordingStatus};
use stream_core::EditManifest;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum H264Encoder {
    Nvenc,
    Amf,
    QuickSync,
    Cpu,
}

impl H264Encoder {
    pub fn ffmpeg_name(self) -> &'static str {
        match self {
            Self::Nvenc => "h264_nvenc",
            Self::Amf => "h264_amf",
            Self::QuickSync => "h264_qsv",
            Self::Cpu => "libx264",
        }
    }
}

impl H264Encoder {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Nvenc => "NVIDIA NVENC",
            Self::Amf => "AMD AMF",
            Self::QuickSync => "Intel Quick Sync",
            Self::Cpu => "CPU libx264",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncoderPreference {
    Auto,
    Nvenc,
    Amf,
    QuickSync,
    Cpu,
}

impl Default for EncoderPreference {
    fn default() -> Self {
        Self::Auto
    }
}

#[derive(Debug, Clone)]
pub struct EncoderProbeResult {
    pub encoder: H264Encoder,
    pub available: bool,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub struct EncoderProbeReport {
    pub ffmpeg_path: PathBuf,
    pub results: Vec<EncoderProbeResult>,
}

impl EncoderProbeReport {
    pub fn best(&self) -> Option<H264Encoder> {
        [
            H264Encoder::Nvenc,
            H264Encoder::Amf,
            H264Encoder::QuickSync,
            H264Encoder::Cpu,
        ]
        .into_iter()
        .find(|candidate| {
            self.results
                .iter()
                .any(|r| r.encoder == *candidate && r.available)
        })
    }

    pub fn detail_for(&self, encoder: H264Encoder) -> Option<&str> {
        self.results
            .iter()
            .find(|r| r.encoder == encoder)
            .map(|r| r.detail.as_str())
    }

    pub fn is_available(&self, encoder: H264Encoder) -> bool {
        self.results
            .iter()
            .any(|r| r.encoder == encoder && r.available)
    }
}

pub fn choose_h264_encoder(encoders_text: &str) -> H264Encoder {
    if encoders_text.contains("h264_nvenc") {
        H264Encoder::Nvenc
    } else if encoders_text.contains("h264_amf") {
        H264Encoder::Amf
    } else if encoders_text.contains("h264_qsv") {
        H264Encoder::QuickSync
    } else {
        H264Encoder::Cpu
    }
}

/// Number of output frames that should exist after `elapsed` at a fixed FPS.
/// This is wall-clock based, not capture-callback-count based.
pub fn frames_due(elapsed: Duration, fps: u32) -> u64 {
    if fps == 0 {
        return 0;
    }
    let nanos = elapsed.as_nanos();
    ((nanos * fps as u128) / 1_000_000_000u128) as u64
}

pub fn find_ffmpeg() -> Result<PathBuf, RecordingError> {
    // 1. Trust PATH first.
    if let Ok(output) = Command::new("where.exe")
        .arg("ffmpeg")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
    {
        if output.status.success() {
            if let Some(first) = String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
            {
                let path = PathBuf::from(first);
                if path.is_file() {
                    return Ok(path);
                }
            }
        }
    }

    // 2. Search WinGet package store. This handles the common case where
    // winget installed Gyan.FFmpeg but did not expose its bin directory to PATH.
    if let Some(local_app_data) = env::var_os("LOCALAPPDATA") {
        let packages = PathBuf::from(local_app_data)
            .join("Microsoft")
            .join("WinGet")
            .join("Packages");

        if let Some(found) = find_ffmpeg_recursive(&packages, 6) {
            return Ok(found);
        }
    }

    // 3. Common user/system fallbacks.
    for candidate in [
        PathBuf::from(r"C:\ffmpeg\bin\ffmpeg.exe"),
        PathBuf::from(r"C:\Program Files\ffmpeg\bin\ffmpeg.exe"),
        PathBuf::from(r"C:\Program Files (x86)\ffmpeg\bin\ffmpeg.exe"),
    ] {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    Err(RecordingError::FfmpegMissing)
}

fn find_ffmpeg_recursive(root: &Path, max_depth: usize) -> Option<PathBuf> {
    fn walk(dir: &Path, depth: usize, max_depth: usize) -> Option<PathBuf> {
        if depth > max_depth {
            return None;
        }

        let entries = fs::read_dir(dir).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();

            if path.is_file()
                && path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.eq_ignore_ascii_case("ffmpeg.exe"))
            {
                return Some(path);
            }

            if path.is_dir() {
                if let Some(found) = walk(&path, depth + 1, max_depth) {
                    return Some(found);
                }
            }
        }

        None
    }

    walk(root, 0, max_depth)
}

fn ffmpeg_command() -> Result<Command, RecordingError> {
    let path = find_ffmpeg()?;
    Ok(Command::new(path))
}

pub fn ffmpeg_location_string() -> String {
    match find_ffmpeg() {
        Ok(path) => path.display().to_string(),
        Err(_) => "未検出".to_owned(),
    }
}

fn encoder_specific_probe_args(encoder: H264Encoder) -> Vec<&'static str> {
    match encoder {
        H264Encoder::Amf => vec!["-usage", "transcoding", "-quality", "speed"],
        H264Encoder::Nvenc => vec!["-preset", "p1", "-tune", "ll"],
        H264Encoder::QuickSync => vec!["-preset", "veryfast"],
        H264Encoder::Cpu => vec!["-preset", "veryfast"],
    }
}

pub fn probe_h264_encoder_detailed(encoder: H264Encoder) -> EncoderProbeResult {
    let mut cmd = match ffmpeg_command() {
        Ok(cmd) => cmd,
        Err(err) => {
            return EncoderProbeResult {
                encoder,
                available: false,
                detail: err.to_string(),
            }
        }
    };

    // Probe something much closer to the real workload than the old 64x64
    // single-frame test. Hardware encoders can appear usable in -encoders yet
    // fail during actual initialization.
    cmd.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=1920x1080:rate=60",
        "-frames:v",
        "12",
        "-pix_fmt",
        "yuv420p",
        "-c:v",
        encoder.ffmpeg_name(),
    ]);

    cmd.args(encoder_specific_probe_args(encoder));

    cmd.args(["-b:v", "6000k", "-f", "null", "-"]);

    let output = match cmd.output() {
        Ok(output) => output,
        Err(err) => {
            return EncoderProbeResult {
                encoder,
                available: false,
                detail: err.to_string(),
            }
        }
    };

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();

    EncoderProbeResult {
        encoder,
        available: output.status.success(),
        detail: if stderr.is_empty() {
            if output.status.success() {
                "初期化成功".to_owned()
            } else {
                format!("終了コード: {}", output.status)
            }
        } else {
            stderr
        },
    }
}

pub fn probe_all_h264_encoders() -> Result<EncoderProbeReport, RecordingError> {
    let ffmpeg_path = find_ffmpeg()?;
    let results = [
        H264Encoder::Nvenc,
        H264Encoder::Amf,
        H264Encoder::QuickSync,
        H264Encoder::Cpu,
    ]
    .into_iter()
    .map(probe_h264_encoder_detailed)
    .collect();

    Ok(EncoderProbeReport {
        ffmpeg_path,
        results,
    })
}

pub fn detect_h264_encoder_with_preference(
    preference: EncoderPreference,
) -> Result<(H264Encoder, EncoderProbeReport), RecordingError> {
    let report = probe_all_h264_encoders()?;

    let requested = match preference {
        EncoderPreference::Auto => report.best(),
        EncoderPreference::Nvenc => Some(H264Encoder::Nvenc),
        EncoderPreference::Amf => Some(H264Encoder::Amf),
        EncoderPreference::QuickSync => Some(H264Encoder::QuickSync),
        EncoderPreference::Cpu => Some(H264Encoder::Cpu),
    };

    let Some(encoder) = requested else {
        return Err(RecordingError::Ffmpeg(
            "利用可能なH.264エンコーダーがありません".to_owned(),
        ));
    };

    if !report.is_available(encoder) {
        let detail = report.detail_for(encoder).unwrap_or("初期化失敗");
        return Err(RecordingError::Ffmpeg(format!(
            "{} を初期化できません: {}",
            encoder.display_name(),
            detail
        )));
    }

    Ok((encoder, report))
}

pub fn detect_h264_encoder() -> Result<H264Encoder, RecordingError> {
    detect_h264_encoder_with_preference(EncoderPreference::Auto).map(|(encoder, _)| encoder)
}

#[derive(Debug, Clone)]
pub struct RecordingPaths {
    pub root: PathBuf,
    pub video_only: PathBuf,
    pub final_video: PathBuf,
    pub microphone: PathBuf,
    pub desktop_audio: PathBuf,
    pub edit_json: PathBuf,
}

impl RecordingPaths {
    pub fn for_root(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref().to_path_buf();
        Self {
            video_only: root.join("video_only.mp4"),
            final_video: root.join("recording.mp4"),
            microphone: root.join("microphone.wav"),
            desktop_audio: root.join("desktop.wav"),
            edit_json: root.join("yaoyorozu_edit.json"),
            root,
        }
    }

    pub fn timestamped(base: impl AsRef<Path>) -> Self {
        let stamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
        Self::for_root(base.as_ref().join(stamp))
    }

    pub fn application_audio(&self, channel_id: AudioChannelId, name: &str) -> PathBuf {
        let safe_name = sanitize_track_name(name);
        self.root
            .join(format!("application_{channel_id}_{safe_name}.wav"))
    }
}

fn sanitize_track_name(name: &str) -> String {
    let mut safe = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            safe.push(ch.to_ascii_lowercase());
        } else if ch == '-' || ch == '_' {
            safe.push(ch);
        } else if ch.is_whitespace() {
            safe.push('_');
        }
    }
    while safe.contains("__") {
        safe = safe.replace("__", "_");
    }
    let safe = safe.trim_matches('_');
    if safe.is_empty() {
        "audio".to_owned()
    } else {
        safe.to_owned()
    }
}

#[derive(Debug, Clone)]
struct ApplicationTrack {
    channel_id: AudioChannelId,
    process_id: u32,
    path: PathBuf,
    start_channel: AudioChannel,
}

#[derive(Debug, Clone)]
pub struct FinalMixTrack {
    pub path: PathBuf,
    pub gain: f32,
    pub muted: bool,
    pub enabled: bool,
    pub include_in_stream_mix: bool,
}

#[derive(Debug, Clone)]
pub struct RecordingConfig {
    pub source_width: u32,
    pub source_height: u32,
    pub output_width: u32,
    pub output_height: u32,
    pub fps: u32,
    pub bitrate_kbps: u32,
    pub encoder_preference: EncoderPreference,
}

#[derive(Debug)]
pub enum RecordingError {
    FfmpegMissing,
    Io(String),
    Ffmpeg(String),
    Audio(String),
    InvalidDimensions,
}

impl fmt::Display for RecordingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FfmpegMissing => write!(f, "FFmpegが見つかりません"),
            Self::Io(s) => write!(f, "I/Oエラー: {s}"),
            Self::Ffmpeg(s) => write!(f, "FFmpegエラー: {s}"),
            Self::Audio(s) => write!(f, "音声録音エラー: {s}"),
            Self::InvalidDimensions => write!(f, "プレビュー映像のサイズが未確定です"),
        }
    }
}

impl std::error::Error for RecordingError {}

pub fn build_mix_filter(settings: MixerSettings) -> String {
    let mic_gain = settings.effective_mic_gain();
    let desktop_gain = settings.effective_desktop_gain();
    let master_gain = settings.master_gain.clamp(0.0, 1.0);

    format!(
        "[1:a]volume={mic_gain:.4}[mic];\
[2:a]volume={desktop_gain:.4}[desktop];\
[mic][desktop]amix=inputs=2:duration=longest:normalize=0[mix];\
[mix]volume={master_gain:.4},alimiter=limit=0.98[a]"
    )
}

pub fn build_routed_mix_filter(tracks: &[FinalMixTrack], master_gain: f32) -> Option<String> {
    let included: Vec<_> = tracks
        .iter()
        .enumerate()
        .filter(|(_, track)| track.enabled && track.include_in_stream_mix)
        .collect();

    if included.is_empty() {
        return None;
    }

    let mut parts = Vec::new();
    let mut labels = Vec::new();
    for (track_index, (input_index, track)) in included.into_iter().enumerate() {
        let gain = if track.muted {
            0.0
        } else {
            track.gain.clamp(0.0, 1.0)
        };
        let label = format!("route{track_index}");
        parts.push(format!("[{}:a]volume={gain:.4}[{label}]", input_index + 1));
        labels.push(format!("[{label}]"));
    }

    let master_gain = master_gain.clamp(0.0, 1.0);
    if labels.len() == 1 {
        parts.push(format!(
            "{}volume={master_gain:.4},alimiter=limit=0.98[a]",
            labels[0]
        ));
    } else {
        parts.push(format!(
            "{}amix=inputs={}:duration=longest:normalize=0[mix]",
            labels.join(""),
            labels.len()
        ));
        parts.push(format!(
            "[mix]volume={master_gain:.4},alimiter=limit=0.98[a]"
        ));
    }

    Some(parts.join(";"))
}

pub struct RecordingSession {
    pub paths: RecordingPaths,
    pub backend_name: String,
    gpu: GpuRecordingHandle,
    audio: Option<PcmRecordingWorker>,
    mixer: MixerControl,
    channel_mixer: ChannelMixerControl,
    application_tracks: Vec<ApplicationTrack>,
}
impl RecordingSession {
    pub fn start(
        base_dir: impl AsRef<Path>,
        config: RecordingConfig,
        gpu: GpuRecordingHandle,
        mixer: MixerControl,
    ) -> Result<Self, RecordingError> {
        Self::start_with_audio_routing(
            base_dir,
            config,
            gpu,
            mixer,
            ChannelMixerControl::default(),
            AudioDeviceState::default(),
        )
    }

    pub fn start_with_audio_devices(
        base_dir: impl AsRef<Path>,
        config: RecordingConfig,
        gpu: GpuRecordingHandle,
        mixer: MixerControl,
        audio_devices: AudioDeviceState,
    ) -> Result<Self, RecordingError> {
        Self::start_with_audio_routing(
            base_dir,
            config,
            gpu,
            mixer,
            ChannelMixerControl::default(),
            audio_devices,
        )
    }

    pub fn start_with_audio_routing(
        base_dir: impl AsRef<Path>,
        config: RecordingConfig,
        gpu: GpuRecordingHandle,
        mixer: MixerControl,
        channel_mixer: ChannelMixerControl,
        audio_devices: AudioDeviceState,
    ) -> Result<Self, RecordingError> {
        if config.source_width == 0
            || config.source_height == 0
            || config.output_width == 0
            || config.output_height == 0
        {
            return Err(RecordingError::InvalidDimensions);
        }

        // FFmpeg is still required for the final audio/video mux only.
        ffmpeg_command()?
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|_| RecordingError::FfmpegMissing)?;

        let paths = RecordingPaths::timestamped(base_dir);
        fs::create_dir_all(&paths.root).map_err(|e| RecordingError::Io(e.to_string()))?;

        gpu.start(GpuRecordingConfig {
            path: paths.video_only.clone(),
            width: config.output_width,
            height: config.output_height,
            fps: config.fps,
            bitrate_bps: config.bitrate_kbps * 1000,
        })
        .map_err(|e| RecordingError::Io(e.to_string()))?;

        let application_tracks: Vec<ApplicationTrack> = channel_mixer
            .channels()
            .into_iter()
            .filter(|channel| channel.kind == AudioChannelKind::Application)
            .filter_map(|channel| {
                let process_id = channel.process_id?;
                Some(ApplicationTrack {
                    channel_id: channel.id,
                    process_id,
                    path: paths.application_audio(channel.id, &channel.name),
                    start_channel: channel,
                })
            })
            .collect();

        let audio = match PcmRecordingWorker::start_with_devices(
            AudioRecordingPaths {
                microphone: paths.microphone.clone(),
                desktop: paths.desktop_audio.clone(),
                applications: application_tracks
                    .iter()
                    .map(|track| ApplicationRecordingPath {
                        channel_id: track.channel_id,
                        process_id: track.process_id,
                        path: track.path.clone(),
                    })
                    .collect(),
            },
            audio_devices,
        ) {
            Ok(audio) => audio,
            Err(err) => {
                let _ = gpu.stop();
                return Err(RecordingError::Audio(err.to_string()));
            }
        };

        Ok(Self {
            paths,
            backend_name: "DirectX GPU / Windows Media Foundation H.264".to_owned(),
            gpu,
            audio: Some(audio),
            mixer,
            channel_mixer,
            application_tracks,
        })
    }

    pub fn encoded_frames(&self) -> u64 {
        self.gpu.encoded_frames()
    }

    pub fn gpu_status(&self) -> GpuRecordingStatus {
        self.gpu.status()
    }

    pub fn stop_async(
        mut self,
        manifest: EditManifest,
    ) -> mpsc::Receiver<Result<RecordingPaths, RecordingError>> {
        let (tx, rx) = mpsc::channel();

        thread::Builder::new()
            .name("yaoyorozu-recording-finalize".to_owned())
            .spawn(move || {
                let result = self.finish(manifest);
                let _ = tx.send(result);
            })
            .expect("finalizer thread spawn failed");

        rx
    }

    fn finish(&mut self, manifest: EditManifest) -> Result<RecordingPaths, RecordingError> {
        self.gpu
            .stop()
            .map_err(|e| RecordingError::Io(e.to_string()))?;

        if let Some(audio) = self.audio.take() {
            audio
                .stop_and_join()
                .map_err(|e| RecordingError::Audio(e.to_string()))?;
        }

        // WGC continues running for preview, so the next callback processes
        // the Stop command and VideoEncoder::finish().
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            match self.gpu.status() {
                GpuRecordingStatus::Finished => break,
                GpuRecordingStatus::Failed(err) => {
                    return Err(RecordingError::Ffmpeg(format!(
                        "GPU DirectX encoder: {err}"
                    )));
                }
                _ if Instant::now() >= deadline => {
                    return Err(RecordingError::Ffmpeg(
                        "GPU録画の終了待ちがタイムアウトしました".to_owned(),
                    ));
                }
                _ => thread::sleep(Duration::from_millis(20)),
            }
        }

        let json = manifest
            .to_json_pretty()
            .map_err(|e| RecordingError::Io(e.to_string()))?;
        fs::write(&self.paths.edit_json, json).map_err(|e| RecordingError::Io(e.to_string()))?;

        let legacy_mixer = self.mixer.snapshot();
        let channels = self.channel_mixer.channels();
        mux_final_mp4_routed(
            &self.paths,
            legacy_mixer,
            &channels,
            &self.application_tracks,
        )?;
        cleanup_unwanted_individual_wavs(&self.paths, &channels, &self.application_tracks);
        Ok(self.paths.clone())
    }
}

fn current_or_start_channel(channels: &[AudioChannel], track: &ApplicationTrack) -> AudioChannel {
    channels
        .iter()
        .find(|channel| channel.id == track.channel_id)
        .cloned()
        .unwrap_or_else(|| track.start_channel.clone())
}

fn remove_temporary_track_files(path: &Path) {
    // Phase 9.2.4 Fix2: each temporary WAV also has a diagnostics sidecar
    // (`*.audio.txt`).  `record_individual = false` means neither artifact
    // should survive finalization.
    let _ = fs::remove_file(path);
    let _ = fs::remove_file(path.with_extension("audio.txt"));
}

fn cleanup_unwanted_individual_wavs(
    paths: &RecordingPaths,
    channels: &[AudioChannel],
    application_tracks: &[ApplicationTrack],
) {
    if let Some(channel) = channels
        .iter()
        .find(|channel| channel.id == ChannelMixerControl::MICROPHONE_ID)
    {
        if !channel.record_individual {
            remove_temporary_track_files(&paths.microphone);
        }
    }
    if let Some(channel) = channels
        .iter()
        .find(|channel| channel.id == ChannelMixerControl::DESKTOP_ID)
    {
        if !channel.record_individual {
            remove_temporary_track_files(&paths.desktop_audio);
        }
    }
    for track in application_tracks {
        let channel = current_or_start_channel(channels, track);
        if !channel.record_individual {
            remove_temporary_track_files(&track.path);
        }
    }
}

fn mux_final_mp4_routed(
    paths: &RecordingPaths,
    legacy_mixer: MixerSettings,
    channels: &[AudioChannel],
    application_tracks: &[ApplicationTrack],
) -> Result<(), RecordingError> {
    let desktop = channels
        .iter()
        .find(|channel| channel.id == ChannelMixerControl::DESKTOP_ID)
        .cloned()
        .unwrap_or(AudioChannel {
            id: ChannelMixerControl::DESKTOP_ID,
            name: "PC音声".to_owned(),
            kind: AudioChannelKind::Desktop,
            gain: legacy_mixer.desktop_gain,
            muted: legacy_mixer.desktop_muted,
            enabled: true,
            record_individual: true,
            include_in_stream_mix: true,
            current_level: 0.0,
            source_id: Some("desktop".to_owned()),
            process_id: None,
        });
    let microphone = channels
        .iter()
        .find(|channel| channel.id == ChannelMixerControl::MICROPHONE_ID)
        .cloned()
        .unwrap_or(AudioChannel {
            id: ChannelMixerControl::MICROPHONE_ID,
            name: "マイク".to_owned(),
            kind: AudioChannelKind::Microphone,
            gain: legacy_mixer.mic_gain,
            muted: legacy_mixer.mic_muted,
            enabled: true,
            record_individual: true,
            include_in_stream_mix: true,
            current_level: 0.0,
            source_id: Some("microphone".to_owned()),
            process_id: None,
        });

    let mut tracks = vec![
        FinalMixTrack {
            path: paths.microphone.clone(),
            gain: legacy_mixer.mic_gain,
            muted: legacy_mixer.mic_muted,
            enabled: microphone.enabled,
            include_in_stream_mix: microphone.include_in_stream_mix,
        },
        FinalMixTrack {
            path: paths.desktop_audio.clone(),
            gain: legacy_mixer.desktop_gain,
            muted: legacy_mixer.desktop_muted,
            enabled: desktop.enabled,
            include_in_stream_mix: desktop.include_in_stream_mix,
        },
    ];

    for track in application_tracks {
        let channel = current_or_start_channel(channels, track);
        tracks.push(FinalMixTrack {
            path: track.path.clone(),
            gain: channel.gain,
            muted: channel.muted,
            enabled: channel.enabled,
            include_in_stream_mix: channel.include_in_stream_mix,
        });
    }

    let available_tracks: Vec<FinalMixTrack> = tracks
        .into_iter()
        .filter(|track| {
            fs::metadata(&track.path)
                .map(|metadata| metadata.len() > 44)
                .unwrap_or(false)
        })
        .collect();

    let mut cmd = ffmpeg_command()?;
    cmd.args(["-y", "-hide_banner", "-loglevel", "warning", "-i"])
        .arg(&paths.video_only);
    for track in &available_tracks {
        cmd.arg("-i").arg(&track.path);
    }

    if let Some(filter) = build_routed_mix_filter(&available_tracks, legacy_mixer.master_gain) {
        cmd.arg("-filter_complex").arg(filter).args([
            "-map",
            "0:v:0",
            "-map",
            "[a]",
            "-c:v",
            "copy",
            "-c:a",
            "aac",
            "-b:a",
            "192k",
            "-shortest",
            "-movflags",
            "+faststart",
        ]);
    } else {
        cmd.args([
            "-map",
            "0:v:0",
            "-c:v",
            "copy",
            "-an",
            "-movflags",
            "+faststart",
        ]);
    }

    let output = cmd.arg(&paths.final_video).output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            RecordingError::FfmpegMissing
        } else {
            RecordingError::Io(e.to_string())
        }
    })?;

    if !output.status.success() {
        return Err(RecordingError::Ffmpeg(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }
    Ok(())
}
