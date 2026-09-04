use crossbeam_channel::{bounded, Receiver, Sender, TryRecvError};
use std::{fmt, path::PathBuf, sync::{Arc, Mutex, atomic::{AtomicU64, Ordering}}};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CaptureBackend {
    #[default]
    WindowsGraphicsCapture,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewMode {
    Fps30,
    Fps15,
    Off,
}

impl Default for PreviewMode {
    fn default() -> Self {
        Self::Fps30
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewPolicy {
    mode: PreviewMode,
}

impl PreviewPolicy {
    pub fn normal() -> Self {
        Self { mode: PreviewMode::Fps30 }
    }

    pub fn for_recording(mode: PreviewMode) -> Self {
        Self { mode }
    }

    pub fn target_fps(self) -> Option<u32> {
        match self.mode {
            PreviewMode::Fps30 => Some(30),
            PreviewMode::Fps15 => Some(15),
            PreviewMode::Off => None,
        }
    }

    pub fn interval(self) -> Option<std::time::Duration> {
        self.target_fps()
            .map(|fps| std::time::Duration::from_secs_f64(1.0 / fps as f64))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowInfo {
    pub hwnd: isize,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureSource {
    Desktop,
    Window(WindowInfo),
}

impl CaptureSource {
    pub fn display_name(&self) -> &str {
        match self {
            Self::Desktop => "デスクトップ全体",
            Self::Window(window) => &window.title,
        }
    }
}


#[derive(Debug, Clone, Copy, Default)]
pub struct CaptureStats {
    pub received_frames: u64,
    pub measured_fps: f32,
    pub gpu_to_cpu_ms_avg: f32,
    pub gpu_to_cpu_ms_max: f32,
    pub preview_worker_frames: u64,
    pub preview_jobs_dropped: u64,
}

#[derive(Debug, Clone)]
pub struct CaptureFrame {
    pub sequence: u64,
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
}

impl CaptureFrame {
    pub fn test(sequence: u64, width: u32, height: u32) -> Self {
        Self {
            sequence,
            width,
            height,
            rgba: Arc::<[u8]>::from(vec![0; (width * height * 4) as usize]),
        }
    }
}

/// Compact a mapped D3D11 texture with RowPitch padding into tightly-packed RGBA8.

/// Preview is capped at 960x540 and never upscaled.
/// Aspect ratio is preserved before GPU readback.

#[cfg(windows)]
fn d3d_none_error(context: &'static str) -> windows::core::Error {
    windows::core::Error::new(
        windows::core::HRESULT(0x80004005u32 as i32),
        context,
    )
}

pub fn preview_dimensions(width: u32, height: u32) -> (u32, u32) {
    if width == 0 || height == 0 {
        return (0, 0);
    }

    const MAX_W: u32 = 960;
    const MAX_H: u32 = 540;

    if width <= MAX_W && height <= MAX_H {
        return (width, height);
    }

    let scale_w = MAX_W as f64 / width as f64;
    let scale_h = MAX_H as f64 / height as f64;
    let scale = scale_w.min(scale_h);

    let w = (width as f64 * scale).round().max(1.0) as u32;
    let h = (height as f64 * scale).round().max(1.0) as u32;
    (w, h)
}

pub fn pack_pitched_rgba(src: &[u8], row_pitch: usize, width: u32, height: u32) -> Vec<u8> {
    if width == 0 || height == 0 {
        return Vec::new();
    }

    let row_bytes = width as usize * 4;
    if row_pitch < row_bytes {
        return Vec::new();
    }

    let needed = row_pitch.saturating_mul(height as usize);
    if src.len() < needed {
        return Vec::new();
    }

    let mut out = Vec::with_capacity(row_bytes * height as usize);
    for y in 0..height as usize {
        let start = y * row_pitch;
        out.extend_from_slice(&src[start..start + row_bytes]);
    }
    out
}

#[derive(Clone, Default)]
pub struct LatestFrameSnapshot {
    latest: Arc<Mutex<Option<CaptureFrame>>>,
}

impl LatestFrameSnapshot {
    fn store(&self, frame: CaptureFrame) {
        if let Ok(mut latest) = self.latest.lock() {
            *latest = Some(frame);
        }
    }

    pub fn latest_frame(&self) -> Option<CaptureFrame> {
        self.latest.lock().ok().and_then(|latest| latest.clone())
    }
}

#[derive(Clone)]
pub struct FrameQueue {
    tx: Sender<CaptureFrame>,
    rx: Receiver<CaptureFrame>,
    latest: LatestFrameSnapshot,
}

impl FrameQueue {
    pub fn new(capacity: usize) -> Self {
        let (tx, rx) = bounded(capacity.max(1));
        Self {
            tx,
            rx,
            latest: LatestFrameSnapshot::default(),
        }
    }

    /// Non-blocking newest-frame queue.
    /// No pixel-buffer clone occurs: CaptureFrame uses Arc<[u8]>.
    pub fn push_latest(&self, frame: CaptureFrame) {
        self.latest.store(frame.clone());

        match self.tx.try_send(frame) {
            Ok(()) => {}
            Err(crossbeam_channel::TrySendError::Full(frame)) => {
                let _ = self.rx.try_recv();
                let _ = self.tx.try_send(frame);
            }
            Err(crossbeam_channel::TrySendError::Disconnected(_)) => {}
        }
    }

    pub fn try_recv(&self) -> Result<CaptureFrame, TryRecvError> {
        self.rx.try_recv()
    }

    pub fn receiver(&self) -> Receiver<CaptureFrame> {
        self.rx.clone()
    }

    /// Return the newest preview frame without consuming the delivery queue.
    pub fn latest_frame(&self) -> Option<CaptureFrame> {
        self.latest.latest_frame()
    }

    /// Cheap cloneable handle for deferred preview windows.
    pub fn latest_handle(&self) -> LatestFrameSnapshot {
        self.latest.clone()
    }
}


#[derive(Debug, Clone)]
pub struct GpuRecordingConfig {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_bps: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GpuRecordingStatus {
    Idle,
    Starting,
    Recording,
    Finishing,
    Finished,
    Failed(String),
}

impl Default for GpuRecordingStatus {
    fn default() -> Self {
        Self::Idle
    }
}

#[derive(Debug)]
enum GpuRecordingCommand {
    Start(GpuRecordingConfig),
    Stop,
    SetPreviewMode(PreviewMode),
}

#[derive(Clone)]
pub struct GpuRecordingHandle {
    tx: Sender<GpuRecordingCommand>,
    status: Arc<Mutex<GpuRecordingStatus>>,
    encoded_frames: Arc<AtomicU64>,
}

impl GpuRecordingHandle {
    pub fn start(&self, config: GpuRecordingConfig) -> Result<(), CaptureError> {
        self.encoded_frames.store(0, Ordering::Relaxed);
        if let Ok(mut status) = self.status.lock() {
            *status = GpuRecordingStatus::Starting;
        }
        self.tx
            .send(GpuRecordingCommand::Start(config))
            .map_err(|e| CaptureError::Backend(format!("GPU録画開始コマンド送信失敗: {e}")))
    }

    pub fn stop(&self) -> Result<(), CaptureError> {
        if let Ok(mut status) = self.status.lock() {
            *status = GpuRecordingStatus::Finishing;
        }
        self.tx
            .send(GpuRecordingCommand::Stop)
            .map_err(|e| CaptureError::Backend(format!("GPU録画停止コマンド送信失敗: {e}")))
    }

    pub fn set_preview_mode(&self, mode: PreviewMode) -> Result<(), CaptureError> {
        self.tx
            .send(GpuRecordingCommand::SetPreviewMode(mode))
            .map_err(|e| CaptureError::Backend(format!("プレビューモード送信失敗: {e}")))
    }

    pub fn status(&self) -> GpuRecordingStatus {
        self.status.lock().map(|s| s.clone()).unwrap_or_else(|_| {
            GpuRecordingStatus::Failed("GPU録画状態ロック失敗".to_owned())
        })
    }

    pub fn encoded_frames(&self) -> u64 {
        self.encoded_frames.load(Ordering::Relaxed)
    }
}

#[derive(Debug, Clone)]
pub enum CaptureError {
    UnsupportedPlatform,
    Backend(String),
    WindowUnavailable,
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform => write!(f, "Windows以外ではWGCキャプチャを開始できません"),
            Self::Backend(message) => write!(f, "WGCエラー: {message}"),
            Self::WindowUnavailable => write!(f, "選択したウィンドウをキャプチャできません"),
        }
    }
}

impl std::error::Error for CaptureError {}

pub fn enumerate_windows() -> Result<Vec<WindowInfo>, CaptureError> {
    #[cfg(windows)]
    {
        wgc::enumerate_windows()
    }

    #[cfg(not(windows))]
    {
        Err(CaptureError::UnsupportedPlatform)
    }
}

#[cfg(windows)]
pub struct CaptureWorker {
    preview_queue: FrameQueue,
    control: Option<windows_capture::capture::CaptureControl<wgc::WgcHandler, wgc::HandlerError>>,
    stats: std::sync::Arc<std::sync::Mutex<CaptureStats>>,
    gpu_recording: GpuRecordingHandle,
}

#[cfg(not(windows))]
pub struct CaptureWorker {
    preview_queue: FrameQueue,
}

impl CaptureWorker {
    pub fn start(source: CaptureSource, target_fps: u32) -> Result<Self, CaptureError> {
        #[cfg(windows)]
        {
            wgc::start_worker(source, target_fps)
        }

        #[cfg(not(windows))]
        {
            let _ = (source, target_fps);
            Err(CaptureError::UnsupportedPlatform)
        }
    }

    pub fn start_desktop(target_fps: u32) -> Result<Self, CaptureError> {
        Self::start(CaptureSource::Desktop, target_fps)
    }

    pub fn try_recv(&self) -> Result<CaptureFrame, TryRecvError> {
        self.preview_queue.try_recv()
    }

    pub fn latest_preview_frame(&self) -> Option<CaptureFrame> {
        self.preview_queue.latest_frame()
    }

    pub fn latest_preview_handle(&self) -> LatestFrameSnapshot {
        self.preview_queue.latest_handle()
    }

    pub fn backend(&self) -> CaptureBackend {
        CaptureBackend::WindowsGraphicsCapture
    }

    #[cfg(windows)]
    pub fn gpu_recording_handle(&self) -> GpuRecordingHandle {
        self.gpu_recording.clone()
    }

    #[cfg(windows)]
    pub fn stats(&self) -> CaptureStats {
        self.stats.lock().map(|s| *s).unwrap_or_default()
    }

    #[cfg(not(windows))]
    pub fn stats(&self) -> CaptureStats {
        CaptureStats::default()
    }
}

#[cfg(windows)]
impl Drop for CaptureWorker {
    fn drop(&mut self) {
        if let Some(control) = self.control.take() {
            let _ = control.stop();
        }
    }
}

#[cfg(windows)]
mod wgc {
    use crate::{pack_pitched_rgba, preview_dimensions, PreviewMode, PreviewPolicy};
    use super::{CaptureError, CaptureFrame, CaptureSource, CaptureWorker, FrameQueue, WindowInfo};
    use std::{
        error::Error,
        ffi::c_void,
        sync::{
            atomic::{AtomicU64, Ordering},
            Arc, Mutex,
        },
        time::{Duration, Instant},
    };
    use windows_capture::{
        capture::{Context, GraphicsCaptureApiHandler},
        d3d11::{SendDirectX, StagingTexture},
        encoder::{
            AudioSettingsBuilder, ContainerSettingsBuilder, VideoEncoder,
            VideoSettingsBuilder, VideoSettingsSubType,
        },
        frame::Frame,
        graphics_capture_api::InternalCaptureControl,
        monitor::Monitor,
        settings::{
            ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
            MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
        },
        window::Window,
    };


    use windows::{
        core::Interface,
        Win32::{
            Foundation::RECT,
            Graphics::{
                Direct3D11::{
                    D3D11_BIND_RENDER_TARGET, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE,
                    D3D11_TEX2D_VPIV, D3D11_TEX2D_VPOV, D3D11_TEXTURE2D_DESC,
                    D3D11_USAGE_DEFAULT, D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
                    D3D11_VIDEO_PROCESSOR_CONTENT_DESC, D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC,
                    D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0,
                    D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC,
                    D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0,
                    D3D11_VIDEO_PROCESSOR_STREAM, D3D11_VIDEO_USAGE_OPTIMAL_SPEED,
                    D3D11_VPIV_DIMENSION_TEXTURE2D, D3D11_VPOV_DIMENSION_TEXTURE2D,
                    ID3D11DeviceContext, ID3D11Multithread, ID3D11Texture2D,
                    ID3D11VideoContext, ID3D11VideoDevice, ID3D11VideoProcessor,
                    ID3D11VideoProcessorEnumerator, ID3D11VideoProcessorOutputView,
                },
                Dxgi::Common::{DXGI_RATIONAL, DXGI_SAMPLE_DESC},
            },
        },
    };


    struct GpuPreviewScaler {
        input_width: u32,
        input_height: u32,
        output_width: u32,
        output_height: u32,
        format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT,
        video_device: ID3D11VideoDevice,
        video_context: ID3D11VideoContext,
        enumerator: ID3D11VideoProcessorEnumerator,
        processor: ID3D11VideoProcessor,
        output_texture: ID3D11Texture2D,
        output_view: ID3D11VideoProcessorOutputView,
        frame_index: u32,
    }

    impl GpuPreviewScaler {
        fn new(frame: &Frame, output_width: u32, output_height: u32) -> windows::core::Result<Self> {
            let input_width = frame.width();
            let input_height = frame.height();
            let format = frame.desc().Format;

            let video_device: ID3D11VideoDevice = frame.device().cast()?;
            let video_context: ID3D11VideoContext = frame.device_context().cast()?;

            let content = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
                InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
                InputFrameRate: DXGI_RATIONAL {
                    Numerator: 60,
                    Denominator: 1,
                },
                InputWidth: input_width,
                InputHeight: input_height,
                OutputFrameRate: DXGI_RATIONAL {
                    Numerator: 60,
                    Denominator: 1,
                },
                OutputWidth: output_width,
                OutputHeight: output_height,
                Usage: D3D11_VIDEO_USAGE_OPTIMAL_SPEED,
            };

            let enumerator =
                unsafe { video_device.CreateVideoProcessorEnumerator(&content)? };
            let processor =
                unsafe { video_device.CreateVideoProcessor(&enumerator, 0)? };

            let output_desc = D3D11_TEXTURE2D_DESC {
                Width: output_width,
                Height: output_height,
                MipLevels: 1,
                ArraySize: 1,
                Format: format,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
                CPUAccessFlags: 0,
                MiscFlags: 0,
            };

            let mut output_texture = None;
            unsafe {
                frame.device().CreateTexture2D(
                    &output_desc,
                    None,
                    Some(&mut output_texture),
                )?;
            }
            let output_texture = output_texture.ok_or_else(|| {
                crate::d3d_none_error("D3D11 object creation returned None")
            })?;

            let output_view_desc = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
                ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
                Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                    Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 },
                },
            };

            let mut output_view = None;
            unsafe {
                video_device.CreateVideoProcessorOutputView(
                    &output_texture,
                    &enumerator,
                    &output_view_desc,
                    Some(&mut output_view),
                )?;
            }
            let output_view = output_view.ok_or_else(|| {
                crate::d3d_none_error("D3D11 object creation returned None")
            })?;

            let source_rect = RECT {
                left: 0,
                top: 0,
                right: input_width as i32,
                bottom: input_height as i32,
            };
            let dest_rect = RECT {
                left: 0,
                top: 0,
                right: output_width as i32,
                bottom: output_height as i32,
            };

            unsafe {
                video_context.VideoProcessorSetStreamSourceRect(
                    &processor,
                    0,
                    true,
                    Some(&source_rect),
                );
                video_context.VideoProcessorSetStreamDestRect(
                    &processor,
                    0,
                    true,
                    Some(&dest_rect),
                );
                video_context.VideoProcessorSetOutputTargetRect(
                    &processor,
                    true,
                    Some(&dest_rect),
                );
            }

            Ok(Self {
                input_width,
                input_height,
                output_width,
                output_height,
                format,
                video_device,
                video_context,
                enumerator,
                processor,
                output_texture,
                output_view,
                frame_index: 0,
            })
        }

        fn matches(&self, frame: &Frame, out_w: u32, out_h: u32) -> bool {
            self.input_width == frame.width()
                && self.input_height == frame.height()
                && self.output_width == out_w
                && self.output_height == out_h
                && self.format == frame.desc().Format
        }

        fn scale(&mut self, frame: &Frame) -> windows::core::Result<&ID3D11Texture2D> {
            use std::mem::ManuallyDrop;

            let input_desc = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
                FourCC: 0,
                ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
                Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
                    Texture2D: D3D11_TEX2D_VPIV {
                        MipSlice: 0,
                        ArraySlice: 0,
                    },
                },
            };

            let mut input_view = None;
            unsafe {
                self.video_device.CreateVideoProcessorInputView(
                    frame.as_raw_texture(),
                    &self.enumerator,
                    &input_desc,
                    Some(&mut input_view),
                )?;
            }
            let input_view = input_view.ok_or_else(|| {
                crate::d3d_none_error("D3D11 object creation returned None")
            })?;

            let mut stream = D3D11_VIDEO_PROCESSOR_STREAM::default();
            stream.Enable = true.into();
            stream.pInputSurface = ManuallyDrop::new(Some(input_view));

            unsafe {
                self.video_context.VideoProcessorBlt(
                    &self.processor,
                    &self.output_view,
                    self.frame_index,
                    &[stream],
                )?;
            }
            self.frame_index = self.frame_index.wrapping_add(1);

            Ok(&self.output_texture)
        }
    }

    struct PreviewReadbackJob {
        texture: SendDirectX<ID3D11Texture2D>,
        context: SendDirectX<ID3D11DeviceContext>,
        width: u32,
        height: u32,
        sequence: u64,
    }

    fn spawn_preview_readback_worker(
        preview_queue: FrameQueue,
        stats: Arc<Mutex<super::CaptureStats>>,
    ) -> (
        crossbeam_channel::Sender<PreviewReadbackJob>,
        crossbeam_channel::Receiver<SendDirectX<ID3D11Texture2D>>,
    ) {
        let (job_tx, job_rx) = crossbeam_channel::bounded::<PreviewReadbackJob>(2);
        let (recycle_tx, recycle_rx) =
            crossbeam_channel::bounded::<SendDirectX<ID3D11Texture2D>>(3);

        std::thread::Builder::new()
            .name("yaoyorozu-preview-readback".to_owned())
            .spawn(move || {
                while let Ok(job) = job_rx.recv() {
                    let started = Instant::now();
                    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();

                    let map_result = unsafe {
                        job.context.0.Map(
                            &job.texture.0,
                            0,
                            D3D11_MAP_READ,
                            0,
                            Some(&mut mapped),
                        )
                    };

                    if map_result.is_ok() && !mapped.pData.is_null() {
                        let row_pitch = mapped.RowPitch as usize;
                        let mapped_len = row_pitch.saturating_mul(job.height as usize);
                        let bytes = unsafe {
                            std::slice::from_raw_parts(
                                mapped.pData.cast::<u8>(),
                                mapped_len,
                            )
                        };

                        let packed = pack_pitched_rgba(
                            bytes,
                            row_pitch,
                            job.width,
                            job.height,
                        );

                        unsafe {
                            job.context.0.Unmap(&job.texture.0, 0);
                        }

                        if !packed.is_empty() {
                            preview_queue.push_latest(CaptureFrame {
                                sequence: job.sequence,
                                width: job.width,
                                height: job.height,
                                rgba: Arc::from(packed),
                            });

                            let elapsed_ms = started.elapsed().as_secs_f32() * 1000.0;
                            if let Ok(mut stats) = stats.lock() {
                                stats.preview_worker_frames =
                                    stats.preview_worker_frames.saturating_add(1);

                                if stats.gpu_to_cpu_ms_avg <= 0.0 {
                                    stats.gpu_to_cpu_ms_avg = elapsed_ms;
                                } else {
                                    stats.gpu_to_cpu_ms_avg =
                                        stats.gpu_to_cpu_ms_avg * 0.9 + elapsed_ms * 0.1;
                                }

                                stats.gpu_to_cpu_ms_max =
                                    stats.gpu_to_cpu_ms_max.max(elapsed_ms);
                            }
                        }
                    }

                    let _ = recycle_tx.try_send(job.texture);
                }
            })
            .expect("preview readback worker spawn failed");

        (job_tx, recycle_rx)
    }

    pub type HandlerError = Box<dyn Error + Send + Sync>;

    #[derive(Clone)]
    pub struct HandlerFlags {
        preview_queue: FrameQueue,
        stats: Arc<Mutex<super::CaptureStats>>,
        gpu_commands: crossbeam_channel::Receiver<super::GpuRecordingCommand>,
        gpu_status: Arc<Mutex<super::GpuRecordingStatus>>,
        gpu_encoded_frames: Arc<AtomicU64>,
    }

    pub struct WgcHandler {
        stats: Arc<Mutex<super::CaptureStats>>,
        sequence: AtomicU64,
        fps_window_started: Instant,
        fps_window_frames: u64,
        last_preview_sent: Instant,
        preview_job_tx: crossbeam_channel::Sender<PreviewReadbackJob>,
        preview_recycle_rx:
            crossbeam_channel::Receiver<SendDirectX<ID3D11Texture2D>>,
        preview_free_textures: Vec<SendDirectX<ID3D11Texture2D>>,
        preview_allocated_textures: usize,
        preview_scaler: Option<GpuPreviewScaler>,
        d3d_multithread_enabled: bool,
        gpu_commands: crossbeam_channel::Receiver<super::GpuRecordingCommand>,
        gpu_status: Arc<Mutex<super::GpuRecordingStatus>>,
        gpu_encoded_frames: Arc<AtomicU64>,
        gpu_encoder: Option<VideoEncoder>,
        preview_mode: PreviewMode,
    }

    impl GraphicsCaptureApiHandler for WgcHandler {
        type Flags = HandlerFlags;
        type Error = HandlerError;

        fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
            let (preview_job_tx, preview_recycle_rx) =
                spawn_preview_readback_worker(
                    ctx.flags.preview_queue.clone(),
                    Arc::clone(&ctx.flags.stats),
                );

            Ok(Self {
                stats: ctx.flags.stats,
                sequence: AtomicU64::new(0),
                fps_window_started: Instant::now(),
                fps_window_frames: 0,
                last_preview_sent: Instant::now() - Duration::from_millis(100),
                preview_job_tx,
                preview_recycle_rx,
                preview_free_textures: Vec::new(),
                preview_allocated_textures: 0,
                preview_scaler: None,
                d3d_multithread_enabled: false,
                gpu_commands: ctx.flags.gpu_commands,
                gpu_status: ctx.flags.gpu_status,
                gpu_encoded_frames: ctx.flags.gpu_encoded_frames,
                gpu_encoder: None,
                preview_mode: PreviewMode::Fps30,
            })
        }

        fn on_frame_arrived(
            &mut self,
            frame: &mut Frame,
            _capture_control: InternalCaptureControl,
        ) -> Result<(), Self::Error> {
            // Handle GPU recorder control first. The recorder consumes the
            // Direct3D frame directly via windows-capture VideoEncoder.
            while let Ok(command) = self.gpu_commands.try_recv() {
                match command {
                    super::GpuRecordingCommand::Start(config) => {
                        let video = VideoSettingsBuilder::new(config.width, config.height)
                            .sub_type(VideoSettingsSubType::H264)
                            .bitrate(config.bitrate_bps)
                            .frame_rate(config.fps);

                        let audio = AudioSettingsBuilder::default().disabled(true);
                        let container = ContainerSettingsBuilder::default();

                        match VideoEncoder::new(video, audio, container, &config.path) {
                            Ok(encoder) => {
                                self.gpu_encoder = Some(encoder);
                                self.gpu_encoded_frames.store(0, Ordering::Relaxed);
                                if let Ok(mut status) = self.gpu_status.lock() {
                                    *status = super::GpuRecordingStatus::Recording;
                                }
                            }
                            Err(err) => {
                                if let Ok(mut status) = self.gpu_status.lock() {
                                    *status = super::GpuRecordingStatus::Failed(err.to_string());
                                }
                            }
                        }
                    }
                    super::GpuRecordingCommand::Stop => {
                        if let Some(encoder) = self.gpu_encoder.take() {
                            if let Ok(mut status) = self.gpu_status.lock() {
                                *status = super::GpuRecordingStatus::Finishing;
                            }
                            match encoder.finish() {
                                Ok(()) => {
                                    if let Ok(mut status) = self.gpu_status.lock() {
                                        *status = super::GpuRecordingStatus::Finished;
                                    }
                                }
                                Err(err) => {
                                    if let Ok(mut status) = self.gpu_status.lock() {
                                        *status = super::GpuRecordingStatus::Failed(err.to_string());
                                    }
                                }
                            }
                        } else if let Ok(mut status) = self.gpu_status.lock() {
                            *status = super::GpuRecordingStatus::Finished;
                        }
                    }
                    super::GpuRecordingCommand::SetPreviewMode(mode) => {
                        self.preview_mode = mode;
                    }
                }
            }

            // GPU-direct recording path: no frame.buffer(), no CPU RGBA readback.
            if let Some(encoder) = self.gpu_encoder.as_mut() {
                match encoder.send_frame(frame) {
                    Ok(()) => {
                        self.gpu_encoded_frames.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(err) => {
                        if let Ok(mut status) = self.gpu_status.lock() {
                            *status = super::GpuRecordingStatus::Failed(err.to_string());
                        }
                        self.gpu_encoder = None;
                    }
                }
            }

            let width = frame.width();
            let height = frame.height();

            let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);

            // CPU readback is only needed for the UI preview. Recording above
            // has already consumed the Direct3D surface directly.
            let preview_policy = PreviewPolicy::for_recording(self.preview_mode);
            let preview_due = preview_policy
                .interval()
                .is_some_and(|interval| self.last_preview_sent.elapsed() >= interval);

            if preview_due {
                // Recycle staging textures returned by the preview worker.
                while let Ok(texture) = self.preview_recycle_rx.try_recv() {
                    self.preview_free_textures.push(texture);
                }

                // Immediate D3D11 context is shared with a worker only after
                // enabling the built-in multithread protection layer.
                if !self.d3d_multithread_enabled {
                    if let Ok(multithread) =
                        frame.device_context().cast::<ID3D11Multithread>()
                    {
                        unsafe {
                            let _ = multithread.SetMultithreadProtected(true);
                        }
                        self.d3d_multithread_enabled = true;
                    }
                }

                let (preview_w, preview_h) = preview_dimensions(width, height);

                if preview_w > 0 && preview_h > 0 {
                    let rebuild_scaler = self
                        .preview_scaler
                        .as_ref()
                        .is_none_or(|scaler| {
                            !scaler.matches(frame, preview_w, preview_h)
                        });

                    if rebuild_scaler {
                        self.preview_scaler =
                            GpuPreviewScaler::new(frame, preview_w, preview_h).ok();

                        // Geometry changed: old staging textures no longer match.
                        self.preview_free_textures.clear();
                        self.preview_allocated_textures = 0;
                    }
                }

                let staging = if preview_w == 0 || preview_h == 0 {
                    None
                } else if let Some(texture) = self.preview_free_textures.pop() {
                    Some(texture)
                } else if self.preview_allocated_textures < 3 {
                    match StagingTexture::new(
                        frame.device(),
                        preview_w,
                        preview_h,
                        frame.desc().Format,
                    ) {
                        Ok(texture) => {
                            self.preview_allocated_textures += 1;
                            Some(SendDirectX::new(texture.texture().clone()))
                        }
                        Err(_) => None,
                    }
                } else {
                    None
                };

                if let Some(staging) = staging {
                    let scaled = self
                        .preview_scaler
                        .as_mut()
                        .and_then(|scaler| scaler.scale(frame).ok());

                    if let Some(scaled_texture) = scaled {
                        // Only the 960x540 (or smaller) GPU result is copied to
                        // CPU-readable staging memory.
                        unsafe {
                            frame.device_context().CopyResource(
                                &staging.0,
                                scaled_texture,
                            );
                        }

                        let job = PreviewReadbackJob {
                            texture: staging,
                            context: SendDirectX::new(frame.device_context().clone()),
                            width: preview_w,
                            height: preview_h,
                            sequence,
                        };

                        match self.preview_job_tx.try_send(job) {
                            Ok(()) => {
                                self.last_preview_sent = Instant::now();
                            }
                            Err(crossbeam_channel::TrySendError::Full(job))
                            | Err(crossbeam_channel::TrySendError::Disconnected(job)) => {
                                self.preview_free_textures.push(job.texture);
                                if let Ok(mut stats) = self.stats.lock() {
                                    stats.preview_jobs_dropped =
                                        stats.preview_jobs_dropped.saturating_add(1);
                                }
                            }
                        }
                    } else {
                        self.preview_free_textures.push(staging);
                        if let Ok(mut stats) = self.stats.lock() {
                            stats.preview_jobs_dropped =
                                stats.preview_jobs_dropped.saturating_add(1);
                        }
                    }
                } else if let Ok(mut stats) = self.stats.lock() {
                    stats.preview_jobs_dropped =
                        stats.preview_jobs_dropped.saturating_add(1);
                }
            }

            self.fps_window_frames += 1;
            let elapsed = self.fps_window_started.elapsed();
            if elapsed >= Duration::from_millis(500) {
                let fps = self.fps_window_frames as f32 / elapsed.as_secs_f32();
                if let Ok(mut stats) = self.stats.lock() {
                    stats.received_frames = sequence + 1;
                    stats.measured_fps = fps;
                    // Preview readback timing is updated asynchronously
                    // by the preview worker.
                }
                self.fps_window_started = Instant::now();
                self.fps_window_frames = 0;

            }

            Ok(())
        }

        fn on_closed(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    pub fn enumerate_windows() -> Result<Vec<WindowInfo>, CaptureError> {
        let windows = Window::enumerate()
            .map_err(|e| CaptureError::Backend(e.to_string()))?;

        let mut result = Vec::new();

        for window in windows {
            if !window.is_valid() {
                continue;
            }

            let Ok(title) = window.title() else {
                continue;
            };

            let title = title.trim().to_owned();
            if title.is_empty() {
                continue;
            }

            result.push(WindowInfo {
                hwnd: window.as_raw_hwnd() as isize,
                title,
            });
        }

        result.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
        result.dedup_by(|a, b| a.hwnd == b.hwnd);
        Ok(result)
    }

    pub fn start_worker(source: CaptureSource, target_fps: u32) -> Result<CaptureWorker, CaptureError> {
        let preview_queue = FrameQueue::new(2);
        let stats = Arc::new(Mutex::new(super::CaptureStats::default()));

        let (gpu_tx, gpu_rx) = crossbeam_channel::unbounded();
        let gpu_status = Arc::new(Mutex::new(super::GpuRecordingStatus::Idle));
        let gpu_encoded_frames = Arc::new(AtomicU64::new(0));

        let flags = HandlerFlags {
            preview_queue: preview_queue.clone(),
            stats: Arc::clone(&stats),
            gpu_commands: gpu_rx,
            gpu_status: Arc::clone(&gpu_status),
            gpu_encoded_frames: Arc::clone(&gpu_encoded_frames),
        };

        let fps = target_fps.clamp(1, 60);
        let interval = Duration::from_secs_f64(1.0 / fps as f64);

        let cursor = CursorCaptureSettings::Default;
        let border = DrawBorderSettings::Default;
        let secondary = SecondaryWindowSettings::Default;
        let minimum_interval = MinimumUpdateIntervalSettings::Custom(interval);
        let dirty = DirtyRegionSettings::Default;

        let control = match source {
            CaptureSource::Desktop => {
                let monitor = Monitor::primary()
                    .map_err(|e| CaptureError::Backend(e.to_string()))?;

                let settings = Settings::new(
                    monitor,
                    cursor,
                    border,
                    secondary,
                    minimum_interval,
                    dirty,
                    ColorFormat::Rgba8,
                    flags,
                );

                WgcHandler::start_free_threaded(settings)
                    .map_err(|e| CaptureError::Backend(e.to_string()))?
            }

            CaptureSource::Window(window_info) => {
                if window_info.hwnd == 0 {
                    return Err(CaptureError::WindowUnavailable);
                }

                let window = Window::from_raw_hwnd(window_info.hwnd as *mut c_void);
                if !window.is_valid() {
                    return Err(CaptureError::WindowUnavailable);
                }

                let settings = Settings::new(
                    window,
                    cursor,
                    border,
                    secondary,
                    minimum_interval,
                    dirty,
                    ColorFormat::Rgba8,
                    flags,
                );

                WgcHandler::start_free_threaded(settings)
                    .map_err(|e| CaptureError::Backend(e.to_string()))?
            }
        };

        Ok(CaptureWorker {
            preview_queue,
            control: Some(control),
            stats,
            gpu_recording: super::GpuRecordingHandle {
                tx: gpu_tx,
                status: gpu_status,
                encoded_frames: gpu_encoded_frames,
            },
        })
    }
}
