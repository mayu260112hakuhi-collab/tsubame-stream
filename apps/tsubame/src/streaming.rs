use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, Command, Stdio},
    sync::{
        mpsc::{self, SyncSender, TrySendError},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use stream_audio::LiveAudioBridge;
use stream_capture::CaptureFrame;
use stream_recording::ffmpeg_location_string;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamingPlatform {
    YouTube,
    Twitch,
}

impl StreamingPlatform {
    pub const ALL: [Self; 2] = [Self::YouTube, Self::Twitch];

    pub const fn label(self) -> &'static str {
        match self {
            Self::YouTube => "YouTube",
            Self::Twitch => "Twitch",
        }
    }

    pub const fn server_hint(self) -> &'static str {
        match self {
            Self::YouTube => "YouTube Live Control Room の RTMPS URL",
            Self::Twitch => "Twitch ingest server の RTMP URL",
        }
    }

    pub const fn default_video_bitrate_kbps(self) -> u32 {
        match self {
            Self::YouTube => 12_000,
            Self::Twitch => 6_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamingTargetConfig {
    pub platform: StreamingPlatform,
    pub server_url: String,
    pub stream_key: String,
    pub video_bitrate_kbps: u32,
}

impl StreamingTargetConfig {
    pub fn new(platform: StreamingPlatform) -> Self {
        Self {
            platform,
            server_url: String::new(),
            stream_key: String::new(),
            video_bitrate_kbps: platform.default_video_bitrate_kbps(),
        }
    }

    pub fn set_platform(&mut self, platform: StreamingPlatform) {
        if self.platform == platform {
            return;
        }
        self.platform = platform;
        self.server_url.clear();
        self.stream_key.clear();
        self.video_bitrate_kbps = platform.default_video_bitrate_kbps();
    }

    pub fn is_ready(&self) -> bool {
        let url = self.server_url.trim();
        let protocol_ok = url.starts_with("rtmp://") || url.starts_with("rtmps://");
        let lower = url.to_ascii_lowercase();
        let provider_ok = match self.platform {
            StreamingPlatform::YouTube => !lower.contains("twitch.tv"),
            StreamingPlatform::Twitch => {
                !lower.contains("youtube.com") && !lower.contains("youtu.be")
            }
        };
        !url.is_empty() && !self.stream_key.trim().is_empty() && protocol_ok && provider_ok
    }

    pub fn readiness_message(&self) -> &'static str {
        let url = self.server_url.trim();
        if self.stream_key.trim().is_empty() {
            return "ストリームキーを入力してください";
        }
        if url.is_empty() {
            return "サーバーURLを入力してください";
        }
        if !(url.starts_with("rtmp://") || url.starts_with("rtmps://")) {
            return "サーバーURLは rtmp:// または rtmps:// で始まる必要があります";
        }

        let lower = url.to_ascii_lowercase();
        match self.platform {
            StreamingPlatform::YouTube if lower.contains("twitch.tv") => {
                "YouTube選択中です。TwitchのURLは使用できません"
            }
            StreamingPlatform::Twitch
                if lower.contains("youtube.com") || lower.contains("youtu.be") =>
            {
                "Twitch選択中です。YouTubeのURLは使用できません"
            }
            StreamingPlatform::YouTube if url.starts_with("rtmp://") => {
                "配信先設定: 準備OK（YouTubeはRTMPSも推奨）"
            }
            _ => "配信先設定: 準備OK",
        }
    }

    pub fn connection_label(&self) -> String {
        if self.is_ready() {
            format!("{}: 設定済み", self.platform.label())
        } else {
            format!("{}: 未設定", self.platform.label())
        }
    }

    pub fn publish_url(&self) -> Result<String, StreamingError> {
        if !self.is_ready() {
            return Err(StreamingError::InvalidTarget);
        }
        let base = self.server_url.trim().trim_end_matches('/');
        let key = self.stream_key.trim().trim_start_matches('/');
        Ok(format!("{base}/{key}"))
    }
}

impl Default for StreamingTargetConfig {
    fn default() -> Self {
        Self::new(StreamingPlatform::YouTube)
    }
}

#[derive(Debug)]
pub enum StreamingError {
    InvalidTarget,
    FfmpegMissing,
    Spawn(String),
}

impl std::fmt::Display for StreamingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidTarget => write!(f, "配信先URLまたはストリームキーが未設定です"),
            Self::FfmpegMissing => write!(f, "FFmpegが見つかりません"),
            Self::Spawn(message) => write!(f, "配信プロセス開始失敗: {message}"),
        }
    }
}

impl std::error::Error for StreamingError {}

pub struct StreamingSession {
    child: Child,
    started_at: Instant,
    platform: StreamingPlatform,
    diagnostic_log: Arc<Mutex<String>>,
    _audio_bridge: Option<LiveAudioBridge>,
    video_tx: Option<SyncSender<Arc<[u8]>>>,
    source_width: u32,
    source_height: u32,
}

impl StreamingSession {
    pub fn start(
        target: &StreamingTargetConfig,
        source_width: u32,
        source_height: u32,
        width: u32,
        height: u32,
        fps: u32,
        audio_bridge: Option<LiveAudioBridge>,
    ) -> Result<Self, StreamingError> {
        let publish_url = target.publish_url()?;
        let ffmpeg = ffmpeg_location_string();
        if ffmpeg == "未検出" {
            return Err(StreamingError::FfmpegMissing);
        }

        if source_width == 0 || source_height == 0 {
            return Err(StreamingError::Spawn(
                "映像ソースのサイズを取得できていません".to_owned(),
            ));
        }
        let bitrate = target.video_bitrate_kbps.max(500);
        let maxrate = bitrate;
        let bufsize = bitrate.saturating_mul(2);
        let gop = fps.saturating_mul(2).max(30);
        let scale = format!("scale={width}:{height}:flags=bicubic,format=yuv420p");

        let fps_text = fps.to_string();
        let bitrate_text = format!("{bitrate}k");
        let maxrate_text = format!("{maxrate}k");
        let bufsize_text = format!("{bufsize}k");
        let gop_text = gop.to_string();

        let mut args: Vec<String> = vec![
            "-hide_banner".into(),
            "-loglevel".into(),
            "info".into(),
            "-thread_queue_size".into(),
            "4".into(),
            "-f".into(),
            "rawvideo".into(),
            "-pixel_format".into(),
            "rgba".into(),
            "-video_size".into(),
            format!("{source_width}x{source_height}"),
            "-framerate".into(),
            fps_text.clone(),
            "-i".into(),
            "pipe:0".into(),
        ];

        let live_inputs = audio_bridge
            .as_ref()
            .map(|bridge| bridge.inputs.clone())
            .unwrap_or_default();

        if live_inputs.is_empty() {
            args.extend([
                "-f".into(),
                "lavfi".into(),
                "-i".into(),
                "anullsrc=r=44100:cl=stereo".into(),
            ]);
        } else {
            for input in &live_inputs {
                args.extend([
                    "-thread_queue_size".into(),
                    "1024".into(),
                    "-f".into(),
                    "s16le".into(),
                    "-ar".into(),
                    "48000".into(),
                    "-ac".into(),
                    "2".into(),
                    "-i".into(),
                    format!(
                        "udp://127.0.0.1:{}?fifo_size=1048576&overrun_nonfatal=1",
                        input.port
                    ),
                ]);
            }
        }

        args.extend([
            "-map".into(),
            "0:v:0".into(),
            "-vf".into(),
            scale,
            "-c:v".into(),
            "libx264".into(),
            "-preset".into(),
            "veryfast".into(),
            "-tune".into(),
            "zerolatency".into(),
            "-profile:v".into(),
            "high".into(),
            "-pix_fmt".into(),
            "yuv420p".into(),
            "-r".into(),
            fps_text.clone(),
            "-fps_mode".into(),
            "cfr".into(),
            "-b:v".into(),
            bitrate_text,
            "-minrate".into(),
            format!("{bitrate}k"),
            "-maxrate".into(),
            maxrate_text,
            "-bufsize".into(),
            bufsize_text,
            "-g".into(),
            gop_text.clone(),
            "-keyint_min".into(),
            gop_text,
            "-sc_threshold".into(),
            "0".into(),
        ]);

        if live_inputs.is_empty() {
            args.extend(["-map".into(), "1:a:0".into()]);
        } else {
            let mut filters = Vec::new();
            let mut mix_inputs = String::new();
            for (n, _input) in live_inputs.iter().enumerate() {
                let ff_index = n + 1;
                filters.push(format!("[{ff_index}:a]aresample=async=1:first_pts=0[a{n}]"));
                mix_inputs.push_str(&format!("[a{n}]"));
            }
            filters.push(format!(
                "{mix_inputs}amix=inputs={}:normalize=0:dropout_transition=2,alimiter=limit=0.95,aresample=44100[aout]",
                live_inputs.len()
            ));
            args.extend([
                "-filter_complex".into(),
                filters.join(";"),
                "-map".into(),
                "[aout]".into(),
            ]);
        }

        args.extend([
            "-c:a".into(),
            "aac".into(),
            "-b:a".into(),
            "128k".into(),
            "-ar".into(),
            "44100".into(),
            "-ac".into(),
            "2".into(),
            "-flvflags".into(),
            "no_duration_filesize".into(),
            "-f".into(),
            "flv".into(),
            publish_url,
        ]);

        let mut child = Command::new(ffmpeg)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| StreamingError::Spawn(e.to_string()))?;

        let stdin = child.stdin.take().ok_or_else(|| {
            StreamingError::Spawn("FFmpeg映像入力パイプを取得できません".to_owned())
        })?;
        let (video_tx, video_rx) = mpsc::sync_channel::<Arc<[u8]>>(4);
        let video_frame_interval = Duration::from_secs_f64(1.0 / fps.max(1) as f64);
        thread::spawn(move || {
            let mut stdin = stdin;
            let mut latest_frame: Option<Arc<[u8]>> = None;
            let mut next_tick = Instant::now();

            loop {
                // UI preview readback is intentionally lower rate than the stream
                // target (normally 30 FPS vs 60 FPS). Keep only the newest frame
                // and pace writes to FFmpeg at the requested stream FPS. When WGC
                // has not produced a new CPU frame yet, repeat the last one instead
                // of starving the RTMP connection.
                while let Ok(frame) = video_rx.try_recv() {
                    latest_frame = Some(frame);
                }

                if latest_frame.is_none() {
                    match video_rx.recv() {
                        Ok(frame) => {
                            latest_frame = Some(frame);
                            next_tick = Instant::now();
                        }
                        Err(_) => break,
                    }
                }

                let now = Instant::now();
                if now < next_tick {
                    match video_rx.recv_timeout(next_tick - now) {
                        Ok(frame) => {
                            latest_frame = Some(frame);
                            continue;
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }

                if let Some(frame) = latest_frame.as_ref() {
                    if stdin.write_all(frame).is_err() {
                        break;
                    }
                }

                next_tick += video_frame_interval;
                let now = Instant::now();
                if next_tick + video_frame_interval < now {
                    next_tick = now + video_frame_interval;
                }
            }
        });

        let diagnostic_log = Arc::new(Mutex::new(String::new()));
        if let Some(stderr) = child.stderr.take() {
            let log = Arc::clone(&diagnostic_log);
            thread::spawn(move || {
                let reader = BufReader::new(stderr);
                for line in reader.lines().map_while(Result::ok) {
                    let mut guard = match log.lock() {
                        Ok(guard) => guard,
                        Err(_) => break,
                    };
                    if guard.len() > 12_000 {
                        let keep_from = guard.len().saturating_sub(8_000);
                        *guard = guard[keep_from..].to_owned();
                    }
                    guard.push_str(&line);
                    guard.push('\n');
                }
            });
        }

        Ok(Self {
            child,
            started_at: Instant::now(),
            platform: target.platform,
            diagnostic_log,
            _audio_bridge: audio_bridge,
            video_tx: Some(video_tx),
            source_width,
            source_height,
        })
    }

    pub fn push_video_frame(&self, frame: &CaptureFrame) {
        if frame.width != self.source_width || frame.height != self.source_height {
            return;
        }
        let Some(tx) = &self.video_tx else {
            return;
        };
        match tx.try_send(Arc::clone(&frame.rgba)) {
            Ok(()) | Err(TrySendError::Full(_)) => {}
            Err(TrySendError::Disconnected(_)) => {}
        }
    }

    pub fn platform(&self) -> StreamingPlatform {
        self.platform
    }

    pub fn elapsed_seconds(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }

    pub fn try_exit(&mut self) -> Result<Option<std::process::ExitStatus>, std::io::Error> {
        self.child.try_wait()
    }

    pub fn diagnostic_summary(&self) -> String {
        let Ok(log) = self.diagnostic_log.lock() else {
            return "FFmpeg診断ログを取得できませんでした".to_owned();
        };
        let lines: Vec<&str> = log.lines().filter(|line| !line.trim().is_empty()).collect();
        if lines.is_empty() {
            return "FFmpegから診断メッセージはありませんでした".to_owned();
        }
        lines
            .into_iter()
            .rev()
            .take(4)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join(" | ")
    }

    pub fn stop(mut self) {
        self.video_tx.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for StreamingSession {
    fn drop(&mut self) {
        self.video_tx.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
