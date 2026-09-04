use crate::addon::{AddonOrigin, AddonRegistry, ADDON_API_VERSION};
use crate::bgm::{BgmLayerSource, BgmPlayer};
use crate::scene::{ImageOverlaySource, LayerId, LayerKind, OverlaySource, SceneLayer};
use crate::settings::{
    load_settings, sanitize_window_position, sanitize_window_size, save_settings, AppSettings,
};
use crate::streaming::{StreamingPlatform, StreamingSession, StreamingTargetConfig};
use crate::ui_layout::{
    compact_source_name, normal_preview_size, METER_GREEN_END_DB, METER_ORANGE_END_DB,
    METER_YELLOW_END_DB, SCENE_LABELS, SOURCE_SELECTOR_WIDTH,
};
use crate::view_model::StreamViewModel;
use eframe::egui;
use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc, Mutex, RwLock,
    },
    time::{Duration, Instant},
};
use stream_audio::{
    application_source_label, dbfs, selection_label, AudioChannelId, AudioChannelKind,
    AudioDeviceSelection, AudioWorker, ChannelMixerControl, ExternalPcmSender, MixerControl,
};
use stream_capture::{
    enumerate_windows, CaptureSource, CaptureWorker, LatestFrameSnapshot, PreviewMode, WindowInfo,
};
use stream_core::{MarkerKind, StreamPreset};
use stream_recording::{
    ffmpeg_location_string, EncoderPreference, RecordingConfig, RecordingPaths, RecordingSession,
};
use sysinfo::System;

pub fn fit_aspect(source_w: f32, source_h: f32, available_w: f32, available_h: f32) -> (f32, f32) {
    if source_w <= 0.0 || source_h <= 0.0 || available_w <= 0.0 || available_h <= 0.0 {
        return (0.0, 0.0);
    }
    let scale = (available_w / source_w).min(available_h / source_h);
    (source_w * scale, source_h * scale)
}

fn paint_audio_meter_vertical(ui: &mut egui::Ui, level: f32, height: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(18.0, height), egui::Sense::hover());

    let painter = ui.painter();
    painter.rect_filled(rect, 2.0, egui::Color32::from_rgb(35, 38, 43));

    let db = dbfs(level).clamp(-60.0, 0.0);
    let filled = ((db + 60.0) / 60.0).clamp(0.0, 1.0);

    let green_end = ((METER_GREEN_END_DB + 60.0) / 60.0).clamp(0.0, 1.0);
    let yellow_end = ((METER_YELLOW_END_DB + 60.0) / 60.0).clamp(0.0, 1.0);
    let orange_end = ((METER_ORANGE_END_DB + 60.0) / 60.0).clamp(0.0, 1.0);

    let segments = [
        (0.0, green_end, egui::Color32::from_rgb(55, 190, 95)),
        (green_end, yellow_end, egui::Color32::from_rgb(230, 205, 65)),
        (
            yellow_end,
            orange_end,
            egui::Color32::from_rgb(235, 145, 50),
        ),
        (orange_end, 1.0, egui::Color32::from_rgb(220, 65, 65)),
    ];

    for (start, end, color) in segments {
        let visible_end = filled.min(end);
        if visible_end <= start {
            continue;
        }
        let bottom = egui::lerp(rect.bottom()..=rect.top(), start);
        let top = egui::lerp(rect.bottom()..=rect.top(), visible_end);
        let part = egui::Rect::from_min_max(
            egui::pos2(rect.left(), top),
            egui::pos2(rect.right(), bottom),
        );
        painter.rect_filled(part, 1.0, color);
    }

    for marker_db in [-60.0_f32, -36.0, -18.0, -6.0, 0.0] {
        let t = ((marker_db + 60.0) / 60.0).clamp(0.0, 1.0);
        let y = egui::lerp(rect.bottom()..=rect.top(), t);
        painter.line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            egui::Stroke::new(0.5_f32, egui::Color32::from_gray(90)),
        );
    }

    response.on_hover_text(format!("{:.1} dBFS", db))
}

fn apply_accessible_ui_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style
        .text_styles
        .insert(egui::TextStyle::Small, egui::FontId::proportional(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Body, egui::FontId::proportional(15.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, egui::FontId::proportional(15.0));
    style
        .text_styles
        .insert(egui::TextStyle::Monospace, egui::FontId::monospace(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Heading, egui::FontId::proportional(19.0));
    style.spacing.button_padding = egui::vec2(8.0, 6.0);
    style.spacing.interact_size.y = 28.0;
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    ctx.set_style(style);
}

fn install_windows_japanese_font(ctx: &egui::Context) -> Option<PathBuf> {
    let windir = std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));

    let candidates = ["YuGothM.ttc", "YuGothR.ttc", "meiryo.ttc", "msgothic.ttc"];

    for name in candidates {
        let path = windir.join("Fonts").join(name);
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };

        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "yaoyorozu_jp".to_owned(),
            egui::FontData::from_owned(bytes).into(),
        );

        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts
                .families
                .entry(family)
                .or_default()
                .insert(0, "yaoyorozu_jp".to_owned());
        }

        ctx.set_fonts(fonts);
        return Some(path);
    }

    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsPage {
    General,
    OfficialAddons,
    ExternalAddons,
}

#[derive(Debug, Clone)]
struct MixerStripModel {
    title: String,
    source: String,
    level: f32,
    gain_percent: f32,
    mute: Option<bool>,
    mix: Option<bool>,
    wav: Option<bool>,
    removable: bool,
}

#[derive(Debug, Default)]
struct MixerStripResponse {
    gain_percent: f32,
    gain_changed: bool,
    mute: Option<bool>,
    mix: Option<bool>,
    wav: Option<bool>,
    remove_clicked: bool,
}

fn draw_mixer_channel_strip(ui: &mut egui::Ui, model: MixerStripModel) -> MixerStripResponse {
    const STRIP_WIDTH: f32 = 78.0;
    const STRIP_HEIGHT: f32 = 410.0;
    const TITLE_HEIGHT: f32 = 22.0;
    const DB_HEIGHT: f32 = 18.0;
    const SOURCE_HEIGHT: f32 = 38.0;
    const METER_HEIGHT: f32 = 150.0;
    const PERCENT_HEIGHT: f32 = 18.0;
    const FOOTER_HEIGHT: f32 = 104.0;

    let inner_width = STRIP_WIDTH - 10.0;
    let mut gain_percent = model.gain_percent;
    let mut gain_changed = false;
    let mut out_mute = model.mute;
    let mut out_mix = model.mix;
    let mut out_wav = model.wav;
    let mut remove_clicked = false;

    ui.group(|ui| {
        ui.set_min_width(STRIP_WIDTH);
        ui.set_max_width(STRIP_WIDTH);
        ui.set_min_height(STRIP_HEIGHT);
        ui.set_max_height(STRIP_HEIGHT);

        ui.vertical_centered(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(inner_width, TITLE_HEIGHT),
                egui::Layout::top_down(egui::Align::Center),
                |ui| {
                    ui.strong(compact_source_name(&model.title, 10))
                        .on_hover_text(&model.title);
                },
            );
            ui.allocate_ui_with_layout(
                egui::vec2(inner_width, DB_HEIGHT),
                egui::Layout::top_down(egui::Align::Center),
                |ui| {
                    ui.small(format!("{:>5.1} dB", dbfs(model.level)));
                },
            );
            ui.allocate_ui_with_layout(
                egui::vec2(inner_width, SOURCE_HEIGHT),
                egui::Layout::top_down(egui::Align::Center),
                |ui| {
                    ui.small(compact_source_name(&model.source, 10))
                        .on_hover_text(&model.source);
                },
            );

            ui.allocate_ui_with_layout(
                egui::vec2(inner_width, METER_HEIGHT),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    paint_audio_meter_vertical(ui, model.level, METER_HEIGHT);
                    let response = ui.add_sized(
                        egui::vec2(20.0, METER_HEIGHT),
                        egui::Slider::new(&mut gain_percent, 0.0..=100.0)
                            .vertical()
                            .show_value(false)
                            .clamping(egui::SliderClamping::Always),
                    );
                    gain_changed = response.changed();
                },
            );

            ui.allocate_ui_with_layout(
                egui::vec2(inner_width, PERCENT_HEIGHT),
                egui::Layout::top_down(egui::Align::Center),
                |ui| {
                    ui.small(format!("{:.0}%", gain_percent));
                },
            );

            ui.allocate_ui_with_layout(
                egui::vec2(inner_width, FOOTER_HEIGHT),
                egui::Layout::top_down(egui::Align::Center),
                |ui| {
                    let mut muted = out_mute.unwrap_or(false);
                    let mute_response =
                        ui.add_enabled(out_mute.is_some(), egui::Checkbox::new(&mut muted, "Mute"));
                    if mute_response.changed() {
                        out_mute = Some(muted);
                    }

                    let mut in_mix = out_mix.unwrap_or(false);
                    let mix_response =
                        ui.add_enabled(out_mix.is_some(), egui::Checkbox::new(&mut in_mix, "Mix"));
                    if mix_response.changed() {
                        out_mix = Some(in_mix);
                    }

                    let mut wav = out_wav.unwrap_or(false);
                    let wav_response =
                        ui.add_enabled(out_wav.is_some(), egui::Checkbox::new(&mut wav, "WAV"));
                    if wav_response.changed() {
                        out_wav = Some(wav);
                    }

                    if model.removable {
                        remove_clicked = ui.small_button("削除").clicked();
                    } else {
                        ui.add_sized(egui::vec2(inner_width, 20.0), egui::Label::new(""));
                    }
                },
            );
        });
    });

    MixerStripResponse {
        gain_percent,
        gain_changed,
        mute: out_mute,
        mix: out_mix,
        wav: out_wav,
        remove_clicked,
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct RuntimeFrameCountSnapshot {
    preview_ui_frames: u64,
    mixer_ui_frames: u64,
    mixer_meter_updates: u64,
    streaming_output_frames: u64,
}

#[derive(Debug, Default)]
struct RuntimeFrameCounters {
    preview_ui_frames: AtomicU64,
    mixer_ui_frames: AtomicU64,
    mixer_meter_updates: AtomicU64,
    streaming_output_frames: AtomicU64,
}

impl RuntimeFrameCounters {
    fn snapshot(&self) -> RuntimeFrameCountSnapshot {
        RuntimeFrameCountSnapshot {
            preview_ui_frames: self.preview_ui_frames.load(Ordering::Relaxed),
            mixer_ui_frames: self.mixer_ui_frames.load(Ordering::Relaxed),
            mixer_meter_updates: self.mixer_meter_updates.load(Ordering::Relaxed),
            streaming_output_frames: self.streaming_output_frames.load(Ordering::Relaxed),
        }
    }

    fn count_preview_ui_frame(&self) {
        self.preview_ui_frames.fetch_add(1, Ordering::Relaxed);
    }

    fn count_mixer_ui_frame(&self) {
        self.mixer_ui_frames.fetch_add(1, Ordering::Relaxed);
    }

    fn count_mixer_meter_update(&self) {
        self.mixer_meter_updates.fetch_add(1, Ordering::Relaxed);
    }

    fn count_streaming_output_frame(&self) {
        self.streaming_output_frames.fetch_add(1, Ordering::Relaxed);
    }
}

fn measured_fps(current: u64, previous: u64, elapsed: Duration) -> f32 {
    let seconds = elapsed.as_secs_f32();
    if seconds <= f32::EPSILON {
        return 0.0;
    }
    current.saturating_sub(previous) as f32 / seconds
}

fn mixer_meter_refresh_due(last_refresh: Option<Instant>, now: Instant) -> bool {
    last_refresh
        .map(|last| now.saturating_duration_since(last) >= Duration::from_millis(33))
        .unwrap_or(true)
}

fn performance_pipeline_text(
    capture_fps: f32,
    preview_fps: f32,
    mixer_render_fps: f32,
    mixer_meter_fps: f32,
    encode_input_fps: f32,
    target_fps: u32,
    drops: u64,
) -> String {
    format!(
        "Capture {:.1} FPS | Preview {:.1} FPS | Mixer Render {:.1} FPS | Meter {:.1} FPS | Encode In {:.1} FPS | Target {} FPS | Drop {}",
        capture_fps, preview_fps, mixer_render_fps, mixer_meter_fps, encode_input_fps, target_fps, drops
    )
}

#[derive(Debug)]
struct PerformanceMonitor {
    system: System,
    pid: Option<sysinfo::Pid>,
    last_refresh: Instant,
    cpu_percent: f32,
    memory_mb: f64,
    preview_fps: f32,
    mixer_ui_fps: f32,
    mixer_meter_fps: f32,
    encode_input_fps: f32,
    last_runtime_counts: RuntimeFrameCountSnapshot,
    last_recording_encoded_frames: Option<u64>,
}

impl PerformanceMonitor {
    fn new() -> Self {
        Self {
            system: System::new(),
            pid: sysinfo::get_current_pid().ok(),
            last_refresh: Instant::now() - Duration::from_secs(2),
            cpu_percent: 0.0,
            memory_mb: 0.0,
            preview_fps: 0.0,
            mixer_ui_fps: 0.0,
            mixer_meter_fps: 0.0,
            encode_input_fps: 0.0,
            last_runtime_counts: RuntimeFrameCountSnapshot::default(),
            last_recording_encoded_frames: None,
        }
    }

    fn refresh_if_due(
        &mut self,
        counters: &RuntimeFrameCounters,
        recording_encoded_frames: Option<u64>,
        streaming_active: bool,
    ) {
        let elapsed = self.last_refresh.elapsed();
        if elapsed < Duration::from_secs(1) {
            return;
        }

        let current_counts = counters.snapshot();
        self.preview_fps = measured_fps(
            current_counts.preview_ui_frames,
            self.last_runtime_counts.preview_ui_frames,
            elapsed,
        );
        self.mixer_ui_fps = measured_fps(
            current_counts.mixer_ui_frames,
            self.last_runtime_counts.mixer_ui_frames,
            elapsed,
        );
        self.mixer_meter_fps = measured_fps(
            current_counts.mixer_meter_updates,
            self.last_runtime_counts.mixer_meter_updates,
            elapsed,
        );

        let streaming_output_fps = measured_fps(
            current_counts.streaming_output_frames,
            self.last_runtime_counts.streaming_output_frames,
            elapsed,
        );
        self.encode_input_fps = if streaming_active {
            streaming_output_fps
        } else if let Some(current_recording_frames) = recording_encoded_frames {
            self.last_recording_encoded_frames
                .map(|previous| measured_fps(current_recording_frames, previous, elapsed))
                .unwrap_or(0.0)
        } else {
            0.0
        };

        self.last_runtime_counts = current_counts;
        self.last_recording_encoded_frames = recording_encoded_frames;

        // Phase 9.4.4: 1秒周期に制限し、監視処理自身が配信負荷へ
        // 影響しすぎないようにする。
        self.system.refresh_all();
        self.last_refresh = Instant::now();

        let Some(pid) = self.pid else {
            return;
        };
        let Some(process) = self.system.process(pid) else {
            return;
        };

        // sysinfo の process.cpu_usage() は「1論理CPU = 100%」基準。
        // Windows タスク マネージャーの「PC全体 = 100%」表示と比較できるよう、
        // 論理CPU数で正規化する。
        let logical_cpu_count = self.system.cpus().len().max(1) as f32;
        self.cpu_percent = (process.cpu_usage() / logical_cpu_count).clamp(0.0, 100.0);

        // sysinfo の process.memory() はプロセスの常駐メモリ量。
        // Windows の表示方式とは多少差が出る場合があるため、負荷の目安として表示する。
        self.memory_mb = process.memory() as f64 / (1024.0 * 1024.0);
    }
}


fn bundle_subdir(name: &str) -> PathBuf {
    let base = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.to_path_buf()))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    base.join(name)
}

fn ensure_image_library_dir() -> PathBuf {
    let root = bundle_subdir("image");
    let _ = fs::create_dir_all(&root);
    for child in ["background", "overlay", "wipe", "thumbnail"] {
        let _ = fs::create_dir_all(root.join(child));
    }
    root
}

fn is_supported_library_image(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "webp" | "bmp" | "svg"
            )
        })
        .unwrap_or(false)
}

fn scan_image_library(root: &std::path::Path) -> Vec<PathBuf> {
    fn visit(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                visit(&path, out);
            } else if is_supported_library_image(&path) {
                out.push(path);
            }
        }
    }

    let mut files = Vec::new();
    visit(root, &mut files);
    files.sort_by_key(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase()
    });
    files
}

fn ensure_bgm_library_dir() -> PathBuf {
    let root = bundle_subdir("bgm");
    let _ = fs::create_dir_all(&root);
    root
}

fn is_supported_bgm(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| matches!(ext.to_ascii_lowercase().as_str(), "mp3" | "wav" | "ogg" | "flac"))
        .unwrap_or(false)
}

fn scan_bgm_library(root: &std::path::Path) -> Vec<PathBuf> {
    fn visit(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                visit(&path, out);
            } else if is_supported_bgm(&path) {
                out.push(path);
            }
        }
    }

    let mut files = Vec::new();
    visit(root, &mut files);
    files.sort_by_key(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase()
    });
    files
}


#[derive(Clone)]
struct PreviewRenderSnapshot {
    layers: Vec<SceneLayer>,
    overlay_source: OverlaySource,
    image_overlays: Vec<(LayerId, ImageOverlaySource)>,
    selected_layer: Option<LayerId>,
    fish_layer_id: LayerId,
    app_started_at: Instant,
}

impl PreviewRenderSnapshot {
    fn layer_locked(&self, id: LayerId) -> bool {
        self.layers
            .iter()
            .find(|layer| layer.id == id)
            .map(|layer| layer.locked)
            .unwrap_or(false)
    }
}

#[derive(Debug, Clone, Copy)]
enum PreviewViewportCommand {
    Close,
    Geometry {
        position: Option<[f32; 2]>,
        size: Option<[f32; 2]>,
    },
    Select(Option<LayerId>),
    Move {
        layer_id: LayerId,
        delta_x: f32,
        delta_y: f32,
        preview_width: f32,
        preview_height: f32,
        source_width: u32,
        source_height: u32,
    },
    Resize {
        layer_id: LayerId,
        delta_x: f32,
        preview_width: f32,
        source_width: u32,
        source_height: u32,
    },
}

#[derive(Default)]
struct DeferredPreviewUiState {
    texture: Option<egui::TextureHandle>,
    resize_drag_layer: Option<LayerId>,
    last_window_position: Option<[f32; 2]>,
    last_window_size: Option<[f32; 2]>,
}

#[derive(Debug, Clone, Default)]
struct MixerRenderSnapshot {
    bgm_rows: Vec<(LayerId, Option<AudioChannelId>, String, f32, bool)>,
    device_switch_enabled: bool,
    device_switch_reason: Option<String>,
}

#[derive(Debug)]
enum MixerViewportCommand {
    Close,
    Geometry {
        position: Option<[f32; 2]>,
        size: Option<[f32; 2]>,
    },
    BgmGain {
        layer_id: LayerId,
        gain_percent: f32,
    },
    BgmMute {
        layer_id: LayerId,
        muted: bool,
    },
    RemoveBgm {
        layer_id: LayerId,
    },
}

#[derive(Debug, Clone, Default)]
struct MixerMeterSnapshot {
    desktop: f32,
    mic: f32,
    mix: f32,
    application_levels: HashMap<u64, f32>,
}

#[derive(Debug, Clone)]
struct DeferredMixerUiState {
    selected_application_audio_pid: Option<u32>,
    application_audio_message: String,
    audio_message: String,
    meter_snapshot: MixerMeterSnapshot,
    last_meter_refresh: Option<Instant>,
    last_window_position: Option<[f32; 2]>,
    last_window_size: Option<[f32; 2]>,
}

impl Default for DeferredMixerUiState {
    fn default() -> Self {
        Self {
            selected_application_audio_pid: None,
            application_audio_message: "アプリ音声: 一覧から選択してください".to_owned(),
            audio_message: "WASAPI音声メーター: 初期化中".to_owned(),
            meter_snapshot: MixerMeterSnapshot::default(),
            last_meter_refresh: None,
            last_window_position: None,
            last_window_size: None,
        }
    }
}

fn mixer_ui_repaint_ms(open: bool) -> u64 {
    if open {
        33
    } else {
        250
    }
}

fn send_mixer_command(
    ctx: &egui::Context,
    tx: &mpsc::Sender<MixerViewportCommand>,
    command: MixerViewportCommand,
) {
    if tx.send(command).is_ok() {
        ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
}

fn main_ui_repaint_ms(
    streaming: bool,
    _preview_open: bool,
    recording: bool,
    recording_preview_mode: PreviewMode,
) -> u64 {
    if streaming {
        // Streaming still pushes CPU-readable preview frames from the app loop.
        // The streaming path will be detached from the UI in a later perf phase.
        33
    } else if recording {
        match recording_preview_mode {
            PreviewMode::Fps30 => 33,
            PreviewMode::Fps15 => 66,
            PreviewMode::Off => 250,
        }
    } else {
        // Deferred preview viewports repaint themselves. An open preview must
        // not force the main control surface to 30 FPS anymore.
        100
    }
}

fn compose_preview_frame(
    frame: stream_capture::CaptureFrame,
    snapshot: &PreviewRenderSnapshot,
) -> stream_capture::CaptureFrame {
    let overlay_enabled = snapshot.layers.iter().any(|layer| match layer.kind {
        LayerKind::FishOverlay if layer.id == snapshot.fish_layer_id => {
            snapshot.overlay_source.enabled
        }
        LayerKind::Image => snapshot
            .image_overlays
            .iter()
            .find(|(id, _)| *id == layer.id)
            .is_some_and(|(_, overlay)| overlay.enabled),
        _ => false,
    });

    if !overlay_enabled {
        return frame;
    }

    let mut composed_rgba = frame.rgba.to_vec();
    for layer in &snapshot.layers {
        match layer.kind {
            LayerKind::FishOverlay if layer.id == snapshot.fish_layer_id => {
                snapshot.overlay_source.compose_test_overlay(
                    &mut composed_rgba,
                    frame.width,
                    frame.height,
                    snapshot.app_started_at.elapsed().as_secs_f32(),
                );
            }
            LayerKind::Image => {
                if let Some((_, image_overlay)) = snapshot
                    .image_overlays
                    .iter()
                    .find(|(id, _)| *id == layer.id)
                {
                    image_overlay.compose(&mut composed_rgba, frame.width, frame.height);
                }
            }
            _ => {}
        }
    }

    stream_capture::CaptureFrame {
        sequence: frame.sequence,
        width: frame.width,
        height: frame.height,
        rgba: Arc::<[u8]>::from(composed_rgba),
    }
}

fn send_preview_command(
    ctx: &egui::Context,
    tx: &mpsc::Sender<PreviewViewportCommand>,
    command: PreviewViewportCommand,
) {
    if tx.send(command).is_ok() {
        ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
}

fn geometry_changed(a: Option<[f32; 2]>, b: Option<[f32; 2]>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => (a[0] - b[0]).abs() > 0.5 || (a[1] - b[1]).abs() > 0.5,
        (None, None) => false,
        _ => true,
    }
}

fn draw_deferred_preview_canvas(
    preview_ctx: &egui::Context,
    ui: &mut egui::Ui,
    outer_size: egui::Vec2,
    snapshot: &PreviewRenderSnapshot,
    latest_frame: Option<&stream_capture::LatestFrameSnapshot>,
    streamed_frame: Option<&Arc<RwLock<Option<stream_capture::CaptureFrame>>>>,
    ui_state: &Arc<Mutex<DeferredPreviewUiState>>,
    command_tx: &mpsc::Sender<PreviewViewportCommand>,
) {
    let desired = egui::vec2(outer_size.x.max(120.0), outer_size.y.max(120.0));
    let (outer, outer_response) = ui.allocate_exact_size(desired, egui::Sense::click());
    ui.painter()
        .rect_filled(outer, 4.0, egui::Color32::from_rgb(18, 20, 24));

    // While streaming, reuse the exact composited frame already sent to the
    // streamer. That keeps deferred preview from compositing overlays a second
    // time at 30 FPS. Outside streaming, compose from the non-consuming latest
    // capture snapshot so the main UI can remain asleep.
    let streamed = streamed_frame.and_then(|shared| {
        shared
            .read()
            .ok()
            .and_then(|frame| frame.as_ref().cloned())
    });
    let composed_frame = if let Some(frame) = streamed {
        frame
    } else {
        let Some(frame) = latest_frame.and_then(|latest| latest.latest_frame()) else {
            ui.painter().text(
                outer.center(),
                egui::Align2::CENTER_CENTER,
                "キャプチャ待機中…",
                egui::FontId::proportional(16.0),
                egui::Color32::LIGHT_GRAY,
            );
            return;
        };
        compose_preview_frame(frame, snapshot)
    };

    let source_size = [composed_frame.width, composed_frame.height];
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [composed_frame.width as usize, composed_frame.height as usize],
        &composed_frame.rgba,
    );

    let texture_id = {
        let Ok(mut state) = ui_state.lock() else {
            return;
        };
        if let Some(texture) = &mut state.texture {
            texture.set(image, egui::TextureOptions::LINEAR);
        } else {
            state.texture = Some(preview_ctx.load_texture(
                "capture-preview-deferred",
                image,
                egui::TextureOptions::LINEAR,
            ));
        }
        state.texture.as_ref().map(|texture| texture.id())
    };

    let Some(texture_id) = texture_id else {
        return;
    };

    let (draw_w, draw_h) = fit_aspect(
        source_size[0] as f32,
        source_size[1] as f32,
        outer.width(),
        outer.height(),
    );
    let image_rect = egui::Rect::from_center_size(outer.center(), egui::vec2(draw_w, draw_h));
    ui.painter().image(
        texture_id,
        image_rect,
        egui::Rect::from_min_max(egui::Pos2::new(0.0, 0.0), egui::Pos2::new(1.0, 1.0)),
        egui::Color32::WHITE,
    );

    let mut overlay_interacted = false;

    if snapshot.overlay_source.enabled && source_size[0] > 0 && source_size[1] > 0 {
        let fish_locked = snapshot.layer_locked(snapshot.fish_layer_id);
        let fish_w = image_rect.width() * snapshot.overlay_source.width_percent / 100.0;
        let fish_h = fish_w * 0.42;
        let bounce = if snapshot.overlay_source.bounce {
            (snapshot.app_started_at.elapsed().as_secs_f32() * 4.2)
                .sin()
                .abs()
                * image_rect.height()
                * 0.055
        } else {
            0.0
        };
        let center = egui::pos2(
            image_rect.left() + image_rect.width() * snapshot.overlay_source.x_percent / 100.0,
            image_rect.top() + image_rect.height() * snapshot.overlay_source.y_percent / 100.0
                - bounce,
        );
        let fish_rect = egui::Rect::from_center_size(center, egui::vec2(fish_w, fish_h));
        let hit_rect = fish_rect.expand(5.0).intersect(image_rect);
        let fish_response = ui.interact(
            hit_rect,
            ui.id().with("overlay_fish_drag_deferred"),
            egui::Sense::click_and_drag(),
        );
        let handle_size = 12.0;
        let handle_rect = egui::Rect::from_center_size(
            fish_rect.right_bottom(),
            egui::vec2(handle_size, handle_size),
        );

        if fish_response.clicked() {
            overlay_interacted = true;
            send_preview_command(
                preview_ctx,
                command_tx,
                PreviewViewportCommand::Select(Some(snapshot.fish_layer_id)),
            );
        }
        if fish_response.drag_started() {
            overlay_interacted = true;
            let resize = !fish_locked
                && preview_ctx
                    .pointer_interact_pos()
                    .map(|p| handle_rect.expand(5.0).contains(p))
                    .unwrap_or(false);
            if let Ok(mut state) = ui_state.lock() {
                state.resize_drag_layer = resize.then_some(snapshot.fish_layer_id);
            }
            send_preview_command(
                preview_ctx,
                command_tx,
                PreviewViewportCommand::Select(Some(snapshot.fish_layer_id)),
            );
        }
        if fish_response.dragged() {
            overlay_interacted = true;
            if !fish_locked {
                let delta = preview_ctx.input(|i| i.pointer.delta());
                let resize = ui_state
                    .lock()
                    .ok()
                    .and_then(|state| state.resize_drag_layer)
                    == Some(snapshot.fish_layer_id);
                let command = if resize {
                    PreviewViewportCommand::Resize {
                        layer_id: snapshot.fish_layer_id,
                        delta_x: delta.x,
                        preview_width: image_rect.width(),
                        source_width: source_size[0],
                        source_height: source_size[1],
                    }
                } else {
                    PreviewViewportCommand::Move {
                        layer_id: snapshot.fish_layer_id,
                        delta_x: delta.x,
                        delta_y: delta.y,
                        preview_width: image_rect.width(),
                        preview_height: image_rect.height(),
                        source_width: source_size[0],
                        source_height: source_size[1],
                    }
                };
                send_preview_command(preview_ctx, command_tx, command);
            }
        }
        if fish_response.drag_stopped() {
            if let Ok(mut state) = ui_state.lock() {
                state.resize_drag_layer = None;
            }
        }

        if snapshot.selected_layer == Some(snapshot.fish_layer_id) {
            let stroke = egui::Stroke::new(1.5_f32, egui::Color32::from_rgb(100, 190, 255));
            ui.painter()
                .rect_stroke(fish_rect, 2.0, stroke, egui::StrokeKind::Outside);
            ui.painter().rect_filled(
                handle_rect,
                1.0,
                egui::Color32::from_rgb(100, 190, 255),
            );
        }
    }

    let image_layer_ids: Vec<LayerId> = snapshot
        .layers
        .iter()
        .rev()
        .filter(|layer| layer.kind == LayerKind::Image)
        .map(|layer| layer.id)
        .collect();

    for image_layer_id in image_layer_ids {
        let image_locked = snapshot.layer_locked(image_layer_id);
        let Some((_, image_overlay)) = snapshot
            .image_overlays
            .iter()
            .find(|(id, _)| *id == image_layer_id)
        else {
            continue;
        };
        if !image_overlay.enabled || source_size[0] == 0 || source_size[1] == 0 {
            continue;
        }

        let overlay_w = image_rect.width() * image_overlay.width_percent / 100.0;
        let overlay_h = overlay_w * image_overlay.aspect();
        let center = egui::pos2(
            image_rect.left() + image_rect.width() * image_overlay.x_percent / 100.0,
            image_rect.top() + image_rect.height() * image_overlay.y_percent / 100.0,
        );
        let overlay_rect = egui::Rect::from_center_size(center, egui::vec2(overlay_w, overlay_h));
        let hit_rect = overlay_rect.expand(5.0).intersect(image_rect);
        let image_response = ui.interact(
            hit_rect,
            ui.id().with(("overlay_image_drag_deferred", image_layer_id)),
            egui::Sense::click_and_drag(),
        );
        let handle_size = 12.0;
        let handle_rect = egui::Rect::from_center_size(
            overlay_rect.right_bottom(),
            egui::vec2(handle_size, handle_size),
        );

        if image_response.clicked() {
            overlay_interacted = true;
            send_preview_command(
                preview_ctx,
                command_tx,
                PreviewViewportCommand::Select(Some(image_layer_id)),
            );
        }
        if image_response.drag_started() {
            overlay_interacted = true;
            let resize = !image_locked
                && preview_ctx
                    .pointer_interact_pos()
                    .map(|p| handle_rect.expand(5.0).contains(p))
                    .unwrap_or(false);
            if let Ok(mut state) = ui_state.lock() {
                state.resize_drag_layer = resize.then_some(image_layer_id);
            }
            send_preview_command(
                preview_ctx,
                command_tx,
                PreviewViewportCommand::Select(Some(image_layer_id)),
            );
        }
        if image_response.dragged() {
            overlay_interacted = true;
            if !image_locked {
                let delta = preview_ctx.input(|i| i.pointer.delta());
                let resize = ui_state
                    .lock()
                    .ok()
                    .and_then(|state| state.resize_drag_layer)
                    == Some(image_layer_id);
                let command = if resize {
                    PreviewViewportCommand::Resize {
                        layer_id: image_layer_id,
                        delta_x: delta.x,
                        preview_width: image_rect.width(),
                        source_width: source_size[0],
                        source_height: source_size[1],
                    }
                } else {
                    PreviewViewportCommand::Move {
                        layer_id: image_layer_id,
                        delta_x: delta.x,
                        delta_y: delta.y,
                        preview_width: image_rect.width(),
                        preview_height: image_rect.height(),
                        source_width: source_size[0],
                        source_height: source_size[1],
                    }
                };
                send_preview_command(preview_ctx, command_tx, command);
            }
        }
        if image_response.drag_stopped() {
            if let Ok(mut state) = ui_state.lock() {
                state.resize_drag_layer = None;
            }
        }

        if snapshot.selected_layer == Some(image_layer_id) {
            let stroke = egui::Stroke::new(1.5_f32, egui::Color32::from_rgb(130, 220, 140));
            ui.painter()
                .rect_stroke(overlay_rect, 2.0, stroke, egui::StrokeKind::Outside);
            ui.painter().rect_filled(
                handle_rect,
                1.0,
                egui::Color32::from_rgb(130, 220, 140),
            );
        }
    }

    if outer_response.clicked() && !overlay_interacted {
        if let Some(pointer) = outer_response.interact_pointer_pos() {
            if image_rect.contains(pointer) {
                send_preview_command(
                    preview_ctx,
                    command_tx,
                    PreviewViewportCommand::Select(None),
                );
            }
        }
    }

    ui.small(format!(
        "プレビュー別窓 / 入力 {}×{} / 画像・ししゃものドラッグ編集対応",
        source_size[0], source_size[1]
    ));
}


fn draw_deferred_audio_mixer(
    mixer_ctx: &egui::Context,
    ui: &mut egui::Ui,
    audio: Option<&Arc<AudioWorker>>,
    meter_snapshot: &MixerMeterSnapshot,
    snapshot: &MixerRenderSnapshot,
    ui_state: &Arc<Mutex<DeferredMixerUiState>>,
    command_tx: &mpsc::Sender<MixerViewportCommand>,
) {
    let desktop_level = meter_snapshot.desktop;
    let mic_level = meter_snapshot.mic;
    let mix_level = meter_snapshot.mix;
    // BGM PCMはまだPhase 8B-Audio前なので実メーターは0.0のまま。
    // UI状態は親からの軽量スナップショットで受け、操作はコマンドで返す。
    let bgm_mixer_snapshot = &snapshot.bgm_rows;
    let mut bgm_mixer_gain_changes: Vec<(LayerId, f32)> = Vec::new();
    let mut bgm_mixer_mute_changes: Vec<(LayerId, bool)> = Vec::new();
    let mut bgm_mixer_remove: Option<LayerId> = None;

    if let Some(audio) = audio {
            let settings = audio.mixer_settings();
            let output_devices = audio.output_devices();
            let input_devices = audio.input_devices();
            let device_switch_enabled = snapshot.device_switch_enabled;
            let all_channels = audio.audio_channels();
            let desktop_channel = all_channels
                .iter()
                .find(|channel| channel.id == ChannelMixerControl::DESKTOP_ID)
                .cloned();
            let microphone_channel = all_channels
                .iter()
                .find(|channel| channel.id == ChannelMixerControl::MICROPHONE_ID)
                .cloned();
            let application_channels: Vec<_> = all_channels
                .iter()
                .filter(|channel| channel.kind == AudioChannelKind::Application)
                .cloned()
                .collect();

            // Mixer stripには長いWASAPIステータス文字列をそのまま出さない。
            // Windows既定を使っている場合は短い「Win規定」に統一して、
            // PC音声 / マイク / Master の3段目の高さを揃える。
            let selected_output = audio.selected_output_device();
            let desktop_source_label = match &selected_output {
                AudioDeviceSelection::Default => "Win規定".to_owned(),
                AudioDeviceSelection::DeviceId(_) => compact_source_name(
                    &selection_label(&selected_output, &output_devices),
                    10,
                ),
            };
            let selected_input = audio.selected_input_device();
            let microphone_source_label = match &selected_input {
                AudioDeviceSelection::Default => "Win規定".to_owned(),
                AudioDeviceSelection::DeviceId(_) => compact_source_name(
                    &selection_label(&selected_input, &input_devices),
                    10,
                ),
            };

            ui.small("同一サイズのチャンネルストリップ / PC・マイク・BGM・アプリ音声・Masterを同じ基準で表示します");
            ui.add_space(4.0);

            let mut remove_channel = None;
            egui::ScrollArea::horizontal()
                .id_salt("vertical_audio_mixer_channels")
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;

                        let desktop_strip = draw_mixer_channel_strip(
                            ui,
                            MixerStripModel {
                                title: "PC音声".to_owned(),
                                source: desktop_source_label,
                                level: desktop_level,
                                gain_percent: settings.desktop_gain * 100.0,
                                mute: Some(settings.desktop_muted),
                                mix: desktop_channel.as_ref().map(|c| c.include_in_stream_mix),
                                wav: desktop_channel.as_ref().map(|c| c.record_individual),
                                removable: false,
                            },
                        );
                        if desktop_strip.gain_changed {
                            audio.set_desktop_gain(desktop_strip.gain_percent / 100.0);
                        }
                        if desktop_strip.mute != Some(settings.desktop_muted) {
                            if let Some(muted) = desktop_strip.mute {
                                audio.set_desktop_muted(muted);
                            }
                        }
                        if let Some(channel) = &desktop_channel {
                            if desktop_strip.mix != Some(channel.include_in_stream_mix) {
                                if let Some(in_mix) = desktop_strip.mix {
                                    audio.set_channel_include_in_stream_mix(channel.id, in_mix);
                                }
                            }
                            if desktop_strip.wav != Some(channel.record_individual) {
                                if let Some(record) = desktop_strip.wav {
                                    audio.set_channel_record_individual(channel.id, record);
                                }
                            }
                        }

                        let microphone_strip = draw_mixer_channel_strip(
                            ui,
                            MixerStripModel {
                                title: "マイク".to_owned(),
                                source: microphone_source_label,
                                level: mic_level,
                                gain_percent: settings.mic_gain * 100.0,
                                mute: Some(settings.mic_muted),
                                mix: microphone_channel.as_ref().map(|c| c.include_in_stream_mix),
                                wav: microphone_channel.as_ref().map(|c| c.record_individual),
                                removable: false,
                            },
                        );
                        if microphone_strip.gain_changed {
                            audio.set_mic_gain(microphone_strip.gain_percent / 100.0);
                        }
                        if microphone_strip.mute != Some(settings.mic_muted) {
                            if let Some(muted) = microphone_strip.mute {
                                audio.set_mic_muted(muted);
                            }
                        }
                        if let Some(channel) = &microphone_channel {
                            if microphone_strip.mix != Some(channel.include_in_stream_mix) {
                                if let Some(in_mix) = microphone_strip.mix {
                                    audio.set_channel_include_in_stream_mix(channel.id, in_mix);
                                }
                            }
                            if microphone_strip.wav != Some(channel.record_individual) {
                                if let Some(record) = microphone_strip.wav {
                                    audio.set_channel_record_individual(channel.id, record);
                                }
                            }
                        }

                        for (bgm_id, audio_channel_id, bgm_name, volume_percent, muted)
                            in bgm_mixer_snapshot
                        {
                            let bgm_channel = audio_channel_id.and_then(|channel_id| {
                                all_channels.iter().find(|channel| channel.id == channel_id)
                            });
                            let bgm_strip = draw_mixer_channel_strip(
                                ui,
                                MixerStripModel {
                                    title: "BGM".to_owned(),
                                    source: bgm_name.clone(),
                                    level: audio_channel_id
                                        .and_then(|channel_id| {
                                            meter_snapshot
                                                .application_levels
                                                .get(&channel_id)
                                                .copied()
                                        })
                                        .unwrap_or(0.0),
                                    gain_percent: *volume_percent,
                                    mute: Some(*muted),
                                    mix: bgm_channel.map(|channel| channel.include_in_stream_mix),
                                    wav: bgm_channel.map(|channel| channel.record_individual),
                                    removable: true,
                                },
                            );
                            if bgm_strip.gain_changed {
                                bgm_mixer_gain_changes.push((*bgm_id, bgm_strip.gain_percent));
                            }
                            if bgm_strip.mute != Some(*muted) {
                                if let Some(new_muted) = bgm_strip.mute {
                                    bgm_mixer_mute_changes.push((*bgm_id, new_muted));
                                }
                            }
                            if let Some(channel) = bgm_channel {
                                if bgm_strip.mix != Some(channel.include_in_stream_mix) {
                                    if let Some(in_mix) = bgm_strip.mix {
                                        audio.set_channel_include_in_stream_mix(channel.id, in_mix);
                                    }
                                }
                                if bgm_strip.wav != Some(channel.record_individual) {
                                    if let Some(record) = bgm_strip.wav {
                                        audio.set_channel_record_individual(channel.id, record);
                                    }
                                }
                            }
                            if bgm_strip.remove_clicked {
                                bgm_mixer_remove = Some(*bgm_id);
                            }
                        }

                        for channel in &application_channels {
                            let source = channel
                                .process_id
                                .map(|pid| format!("PID {pid}"))
                                .unwrap_or_else(|| "アプリ音声".to_owned());
                            let app_strip = draw_mixer_channel_strip(
                                ui,
                                MixerStripModel {
                                    title: channel.name.clone(),
                                    source,
                                    level: meter_snapshot
                                        .application_levels
                                        .get(&channel.id)
                                        .copied()
                                        .unwrap_or(0.0),
                                    gain_percent: channel.gain * 100.0,
                                    mute: Some(channel.muted),
                                    mix: Some(channel.include_in_stream_mix),
                                    wav: Some(channel.record_individual),
                                    removable: true,
                                },
                            );
                            if app_strip.gain_changed {
                                audio.set_channel_gain(channel.id, app_strip.gain_percent / 100.0);
                            }
                            if app_strip.mute != Some(channel.muted) {
                                if let Some(muted) = app_strip.mute {
                                    audio.set_channel_muted(channel.id, muted);
                                }
                            }
                            if app_strip.mix != Some(channel.include_in_stream_mix) {
                                if let Some(in_mix) = app_strip.mix {
                                    audio.set_channel_include_in_stream_mix(channel.id, in_mix);
                                }
                            }
                            if app_strip.wav != Some(channel.record_individual) {
                                if let Some(record) = app_strip.wav {
                                    audio.set_channel_record_individual(channel.id, record);
                                }
                            }
                            if app_strip.remove_clicked {
                                remove_channel = Some(channel.id);
                            }
                        }

                        let master_strip = draw_mixer_channel_strip(
                            ui,
                            MixerStripModel {
                                title: "Master".to_owned(),
                                source: "配信Mix".to_owned(),
                                level: mix_level,
                                gain_percent: settings.master_gain * 100.0,
                                mute: None,
                                mix: None,
                                wav: None,
                                removable: false,
                            },
                        );
                        if master_strip.gain_changed {
                            audio.set_master_gain(master_strip.gain_percent / 100.0);
                        }
                    });
                });

            if let Some(id) = remove_channel {
                audio.remove_audio_channel(id);
            }

            ui.add_space(6.0);
            egui::CollapsingHeader::new("音声入出力・アプリ追加")
                .id_salt("audio_io_controls")
                .default_open(false)
                .show(ui, |ui| {
                    ui.columns(2, |columns| {
                        columns[0].label("PC音声デバイス");
                        let selected_output = audio.selected_output_device();
                        columns[0].add_enabled_ui(device_switch_enabled, |ui| {
                            egui::ComboBox::from_id_salt("phase9_desktop_audio_device")
                                .width(220.0)
                                .selected_text(selection_label(&selected_output, &output_devices))
                                .show_ui(ui, |ui| {
                                    if ui.selectable_label(
                                        selected_output == AudioDeviceSelection::Default,
                                        "Windows既定",
                                    ).clicked() {
                                        audio.set_output_device(AudioDeviceSelection::Default);
                                    }
                                    for device in &output_devices {
                                        let selection = AudioDeviceSelection::DeviceId(device.id.clone());
                                        if ui.selectable_label(selected_output == selection, &device.name).clicked() {
                                            audio.set_output_device(selection);
                                        }
                                    }
                                });
                        });

                        columns[1].label("マイクデバイス");
                        let selected_input = audio.selected_input_device();
                        columns[1].add_enabled_ui(device_switch_enabled, |ui| {
                            egui::ComboBox::from_id_salt("phase9_microphone_audio_device")
                                .width(220.0)
                                .selected_text(selection_label(&selected_input, &input_devices))
                                .show_ui(ui, |ui| {
                                    if ui.selectable_label(
                                        selected_input == AudioDeviceSelection::Default,
                                        "Windows既定",
                                    ).clicked() {
                                        audio.set_input_device(AudioDeviceSelection::Default);
                                    }
                                    for device in &input_devices {
                                        let selection = AudioDeviceSelection::DeviceId(device.id.clone());
                                        if ui.selectable_label(selected_input == selection, &device.name).clicked() {
                                            audio.set_input_device(selection);
                                        }
                                    }
                                });
                        });
                    });

                    ui.separator();
                    let mut ui_state_guard = match ui_state.lock() {
                        Ok(guard) => guard,
                        Err(_) => {
                            ui.label("ミキサーUI状態を取得できません");
                            return;
                        }
                    };
                    let app_sources = audio.application_sources();
                    let selected_app_label = ui_state_guard
                        .selected_application_audio_pid
                        .and_then(|pid| app_sources.iter().find(|source| source.capture_process_id == pid))
                        .map(application_source_label)
                        .unwrap_or_else(|| "アプリ音声を選択".to_owned());

                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt("phase9_application_audio_source")
                            .width(220.0)
                            .selected_text(compact_source_name(&selected_app_label, 28))
                            .show_ui(ui, |ui| {
                                if app_sources.is_empty() {
                                    ui.label("音声を使用中のアプリがありません");
                                }
                                for source in &app_sources {
                                    let label = application_source_label(source);
                                    if ui.selectable_label(
                                        ui_state_guard.selected_application_audio_pid == Some(source.capture_process_id),
                                        &label,
                                    ).on_hover_text(&label).clicked() {
                                        ui_state_guard.selected_application_audio_pid = Some(source.capture_process_id);
                                    }
                                }
                            });

                        let selected_source = ui_state_guard
                            .selected_application_audio_pid
                            .and_then(|pid| app_sources.iter().find(|source| source.capture_process_id == pid).cloned());
                        if ui.add_enabled(
                            selected_source.is_some() && device_switch_enabled,
                            egui::Button::new("＋ アプリ音声"),
                        ).clicked() {
                            if let Some(source) = selected_source {
                                match audio.add_application_channel(source) {
                                    Ok(_) => {
                                        ui_state_guard.application_audio_message =
                                            "アプリ音声チャンネルを追加しました".to_owned();
                                    }
                                    Err(err) => {
                                        ui_state_guard.application_audio_message =
                                            format!("アプリ音声追加失敗: {err}");
                                    }
                                }
                            }
                        }
                    });

                    ui.horizontal(|ui| {
                        if ui.add_enabled(device_switch_enabled, egui::Button::new("アプリ音声一覧更新")).clicked() {
                            match audio.refresh_application_sources() {
                                Ok(()) => {
                                    ui_state_guard.application_audio_message =
                                        "アプリ音声一覧を更新しました".to_owned();
                                }
                                Err(err) => {
                                    ui_state_guard.application_audio_message =
                                        format!("アプリ音声一覧更新失敗: {err}");
                                }
                            }
                        }
                        if ui.add_enabled(device_switch_enabled, egui::Button::new("音声デバイス更新")).clicked() {
                            match audio.refresh_devices() {
                                Ok(()) => {
                                    ui_state_guard.audio_message = "音声デバイス一覧を更新しました".to_owned();
                                }
                                Err(err) => {
                                    ui_state_guard.audio_message = format!("デバイス更新失敗: {err}");
                                }
                            }
                        }
                    });
                    ui.small(&ui_state_guard.application_audio_message);
                    ui.small(&ui_state_guard.audio_message);
                    if !device_switch_enabled {
                        if let Some(reason) = &snapshot.device_switch_reason {
                            ui.small(reason);
                        }
                    }
                });

            egui::CollapsingHeader::new("音声ルーティング説明")
                .id_salt("audio_routing_help")
                .default_open(false)
                .show(ui, |ui| {
                    ui.small("個別WAVは原音のまま保存（Gain/Muteは非破壊）");
                    ui.small("Mix ONのチャンネルだけ完成MP4・配信Mixへ合流");
                    ui.small("個別WAV保存 OFFでもMix用の一時原音は録画後に自動削除");
                });
        } else {
            ui.label("音声デバイスを取得できていません");
        }

    // BGMプレイヤー自体はメインAppが所有するため、操作だけコマンドで返す。
    for (id, gain_percent) in bgm_mixer_gain_changes {
        send_mixer_command(
            mixer_ctx,
            command_tx,
            MixerViewportCommand::BgmGain {
                layer_id: id,
                gain_percent,
            },
        );
    }
    for (id, muted) in bgm_mixer_mute_changes {
        send_mixer_command(
            mixer_ctx,
            command_tx,
            MixerViewportCommand::BgmMute { layer_id: id, muted },
        );
    }
    if let Some(id) = bgm_mixer_remove {
        send_mixer_command(
            mixer_ctx,
            command_tx,
            MixerViewportCommand::RemoveBgm { layer_id: id },
        );
    }

    ui.small("BGMは48kHz stereo PCMで配信Mixへ直接接続 / 個別WAVはAudio.2で追加します");
    if let Ok(state) = ui_state.lock() {
        ui.small(&state.audio_message);
    }
}



fn prefer_selected_or_active_bgm<T: Copy>(
    selected_bgm: Option<T>,
    active_bgm: Option<T>,
) -> Option<T> {
    selected_bgm.or(active_bgm)
}

pub struct YaoyorozuApp {
    vm: StreamViewModel,
    elapsed_ms: u64,
    stream_title: String,

    windows: Vec<WindowInfo>,
    selected_source: CaptureSource,
    capture: Option<CaptureWorker>,
    capture_target_fps: u32,

    preview_size: [u32; 2],
    preview_snapshot: Arc<RwLock<PreviewRenderSnapshot>>,
    streamed_preview_frame: Arc<RwLock<Option<stream_capture::CaptureFrame>>>,
    preview_ui_state: Arc<Mutex<DeferredPreviewUiState>>,
    preview_command_tx: mpsc::Sender<PreviewViewportCommand>,
    preview_command_rx: mpsc::Receiver<PreviewViewportCommand>,
    preview_window_open: bool,
    preview_window_position: Option<[f32; 2]>,
    preview_window_size: Option<[f32; 2]>,
    mixer_window_open: bool,
    mixer_window_position: Option<[f32; 2]>,
    mixer_window_size: Option<[f32; 2]>,
    mixer_snapshot: Arc<RwLock<MixerRenderSnapshot>>,
    mixer_ui_state: Arc<Mutex<DeferredMixerUiState>>,
    mixer_command_tx: mpsc::Sender<MixerViewportCommand>,
    mixer_command_rx: mpsc::Receiver<MixerViewportCommand>,

    capture_message: String,
    font_message: String,
    audio: Option<Arc<AudioWorker>>,
    recording: Option<RecordingSession>,
    finalize_rx: Option<mpsc::Receiver<Result<RecordingPaths, stream_recording::RecordingError>>>,
    recording_message: String,
    ffmpeg_location: String,
    encoder_preference: EncoderPreference,
    recording_preview_mode: PreviewMode,
    capture_preview_mode: PreviewMode,
    streaming_target: StreamingTargetConfig,
    show_stream_key: bool,
    overlay_source: OverlaySource,
    layers: Vec<SceneLayer>,
    selected_layer: Option<LayerId>,
    next_layer_id: LayerId,
    fish_layer_id: LayerId,
    preview_resize_drag: bool,
    image_overlays: Vec<(LayerId, ImageOverlaySource)>,
    image_overlay_message: String,
    image_library_dir: PathBuf,
    image_library_files: Vec<PathBuf>,
    image_library_message: String,
    bgm_layers: Vec<(LayerId, BgmLayerSource)>,
    bgm_players: HashMap<LayerId, BgmPlayer>,
    bgm_pcm_senders: HashMap<LayerId, ExternalPcmSender>,
    bgm_audio_channels: HashMap<LayerId, AudioChannelId>,
    bgm_library_dir: PathBuf,
    bgm_library_files: Vec<PathBuf>,
    bgm_library_message: String,
    bgm_message: String,
    app_started_at: std::time::Instant,
    streaming: Option<StreamingSession>,
    streaming_message: String,
    performance: PerformanceMonitor,
    runtime_frame_counters: Arc<RuntimeFrameCounters>,
    settings_open: bool,
    settings_page: SettingsPage,
    addon_registry: AddonRegistry,
    external_addon_message: String,
    persisted_settings: AppSettings,
    last_saved_settings: AppSettings,
    settings_save_message: String,
    last_settings_save_at: Instant,
    first_run_open: bool,
}

impl YaoyorozuApp {
    fn allocate_layer_id(&mut self) -> LayerId {
        let id = self.next_layer_id;
        self.next_layer_id = self.next_layer_id.saturating_add(1);
        id
    }

    fn layer_locked(&self, id: LayerId) -> bool {
        self.layers
            .iter()
            .find(|layer| layer.id == id)
            .map(|layer| layer.locked)
            .unwrap_or(false)
    }

    fn move_layer_toward_front(&mut self, id: LayerId) {
        let Some(index) = self.layers.iter().position(|layer| layer.id == id) else {
            return;
        };
        if index + 1 < self.layers.len() {
            self.layers.swap(index, index + 1);
        }
    }

    fn move_layer_toward_back(&mut self, id: LayerId) {
        let Some(index) = self.layers.iter().position(|layer| layer.id == id) else {
            return;
        };
        if index > 0 {
            self.layers.swap(index, index - 1);
        }
    }

    fn add_image_layer(&mut self, source: ImageOverlaySource) -> LayerId {
        let id = self.allocate_layer_id();
        self.layers
            .push(SceneLayer::new(id, LayerKind::Image, source.name.clone()));
        self.image_overlays.push((id, source));
        id
    }

    fn remove_image_layer(&mut self, id: LayerId) {
        self.layers.retain(|layer| layer.id != id);
        self.image_overlays.retain(|(layer_id, _)| *layer_id != id);
        if self.selected_layer == Some(id) {
            self.selected_layer = None;
        }
    }

    fn add_bgm_layer(&mut self, source: BgmLayerSource) -> LayerId {
        let id = self.allocate_layer_id();
        self.layers
            .push(SceneLayer::new(id, LayerKind::Audio, source.name.clone()));

        if let Some(audio) = self.audio.as_ref() {
            match audio.add_external_pcm_channel(format!("BGM: {}", source.name)) {
                Ok(sender) => {
                    let channel_id = sender.channel_id();
                    audio.set_channel_gain(channel_id, source.volume_linear());
                    audio.set_channel_muted(channel_id, source.muted);
                    audio.set_channel_include_in_stream_mix(channel_id, true);
                    self.bgm_audio_channels.insert(id, channel_id);
                    self.bgm_pcm_senders.insert(id, sender);
                }
                Err(err) => {
                    self.bgm_message = format!("BGM PCMチャンネル作成失敗: {err}");
                }
            }
        }

        self.bgm_layers.push((id, source));
        id
    }

    fn remove_bgm_layer(&mut self, id: LayerId) {
        if let Some(player) = self.bgm_players.remove(&id) {
            player.stop();
        }
        if let Some(sender) = self.bgm_pcm_senders.remove(&id) {
            sender.clear_level();
        }
        if let Some(channel_id) = self.bgm_audio_channels.remove(&id) {
            if let Some(audio) = self.audio.as_ref() {
                audio.remove_audio_channel(channel_id);
            }
        }
        self.layers.retain(|layer| layer.id != id);
        self.bgm_layers.retain(|(layer_id, _)| *layer_id != id);
        if self.selected_layer == Some(id) {
            self.selected_layer = None;
        }
    }

    fn refresh_bgm_library(&mut self) {
        self.bgm_library_files = scan_bgm_library(&self.bgm_library_dir);
        self.bgm_library_message = format!(
            "BGMライブラリ: {} 曲 / {}",
            self.bgm_library_files.len(),
            self.bgm_library_dir.display()
        );
    }

    fn add_bgm_from_path(&mut self, path: &std::path::Path) {
        if !is_supported_bgm(path) {
            self.bgm_message = "BGM追加失敗: 未対応の音声形式です".to_owned();
            return;
        }
        let source = BgmLayerSource::from_path(path);
        let name = source.name.clone();
        let id = self.add_bgm_layer(source);
        self.selected_layer = Some(id);
        self.bgm_message = format!("BGM追加: {name} / 合計 {} 曲", self.bgm_layers.len());
    }

    fn play_bgm_layer(&mut self, id: LayerId) {
        let Some((_, source)) = self.bgm_layers.iter().find(|(layer_id, _)| *layer_id == id) else {
            return;
        };
        if !source.enabled {
            self.bgm_message = "BGM再生: レイヤーが無効です".to_owned();
            return;
        }
        let path = source.path.clone();
        let loop_enabled = source.loop_enabled;
        let volume = source.effective_volume();
        let name = source.name.clone();
        let pcm_sender = self.bgm_pcm_senders.get(&id).cloned();
        match BgmPlayer::play_file(&path, loop_enabled, volume, pcm_sender) {
            Ok(player) => {
                if let Some(old) = self.bgm_players.insert(id, player) {
                    old.stop();
                }
                self.bgm_message = format!("BGM再生中: {name}");
            }
            Err(err) => {
                self.bgm_message = format!("BGM再生失敗: {err}");
            }
        }
    }

    fn refresh_image_library(&mut self) {
        self.image_library_files = scan_image_library(&self.image_library_dir);
        self.image_library_message = format!(
            "画像ライブラリ: {} 素材 / {}",
            self.image_library_files.len(),
            self.image_library_dir.display()
        );
    }

    fn add_image_from_path(&mut self, path: &std::path::Path) {
        match ImageOverlaySource::load(path) {
            Ok(mut source) => {
                source.clamp_to_frame(self.preview_size[0], self.preview_size[1]);
                let source_name = source.name.clone();
                let pixel_width = source.pixel_width;
                let pixel_height = source.pixel_height;
                let layer_id = self.add_image_layer(source);
                self.selected_layer = Some(layer_id);
                self.preview_resize_drag = false;
                self.image_overlay_message = format!(
                    "画像追加: {} ({}×{}) / 合計 {} 枚",
                    source_name,
                    pixel_width,
                    pixel_height,
                    self.image_overlays.len()
                );
            }
            Err(err) => {
                self.image_overlay_message = format!("画像追加失敗: {err}");
            }
        }
    }

    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        apply_accessible_ui_style(&cc.egui_ctx);
        let font_message = match install_windows_japanese_font(&cc.egui_ctx) {
            Some(path) => format!("日本語フォント: {}", path.display()),
            None => "Windows日本語フォントが見つかりません".to_owned(),
        };

        let (persisted_settings, settings_save_message) = match load_settings() {
            Ok(settings) => (settings, "設定: 読み込み済み".to_owned()),
            Err(err) => (AppSettings::default(), format!("設定読み込み警告: {err}")),
        };

        let preset = match persisted_settings.preset.as_str() {
            "work" => StreamPreset::Work,
            "light" => StreamPreset::Light,
            _ => StreamPreset::Game,
        };
        let encoder_preference = match persisted_settings.encoder_preference.as_str() {
            "auto" => EncoderPreference::Auto,
            "nvenc" => EncoderPreference::Nvenc,
            "qsv" => EncoderPreference::QuickSync,
            "cpu" => EncoderPreference::Cpu,
            _ => EncoderPreference::Amf,
        };
        let recording_preview_mode = match persisted_settings.recording_preview_mode.as_str() {
            "fps30" => PreviewMode::Fps30,
            "off" => PreviewMode::Off,
            _ => PreviewMode::Fps15,
        };
        let streaming_platform = match persisted_settings.streaming_platform.as_str() {
            "twitch" => StreamingPlatform::Twitch,
            _ => StreamingPlatform::YouTube,
        };

        let windows = enumerate_windows().unwrap_or_default();
        let selected_source = CaptureSource::Desktop;
        let capture_target_fps = preset.dimensions().2;

        let (capture, capture_message) =
            match CaptureWorker::start(selected_source.clone(), capture_target_fps) {
                Ok(worker) => (
                    Some(worker),
                    format!("WGC: デスクトップ全体 / target {capture_target_fps} FPS"),
                ),
                Err(err) => (None, format!("キャプチャ開始失敗: {err}")),
            };

        let (audio, audio_message) = match AudioWorker::start_default_devices() {
            Ok(worker) => {
                if let Some(id) = persisted_settings.output_device_id.clone() {
                    worker.set_output_device(AudioDeviceSelection::DeviceId(id));
                }
                if let Some(id) = persisted_settings.input_device_id.clone() {
                    worker.set_input_device(AudioDeviceSelection::DeviceId(id));
                }
                (Some(Arc::new(worker)), "WASAPI音声メーター: 稼働中".to_owned())
            }
            Err(err) => (None, format!("音声開始失敗: {err}")),
        };

        let mut vm = StreamViewModel::default();
        vm.set_preset(preset);
        let mut streaming_target = StreamingTargetConfig::new(streaming_platform);
        streaming_target.server_url = persisted_settings.streaming_server_url.clone();
        streaming_target.video_bitrate_kbps =
            persisted_settings.streaming_video_bitrate_kbps.max(500);
        // ストリームキーは平文保存しない。起動ごとに入力する。
        streaming_target.stream_key.clear();
        let first_run_open = !persisted_settings.first_run_complete;
        let last_saved_settings = persisted_settings.clone();

        // Layer foundation: keep the proven source implementations for now,
        // but give every editable preview item a stable layer identity.
        let fish_layer_id: LayerId = 1;
        let layers = vec![SceneLayer::new(
            fish_layer_id,
            LayerKind::FishOverlay,
            "ししゃも（テスト）",
        )];

        let image_library_dir = ensure_image_library_dir();
        let image_library_files = scan_image_library(&image_library_dir);
        let image_library_message = format!(
            "画像ライブラリ: {} 枚 / {}",
            image_library_files.len(),
            image_library_dir.display()
        );
        let bgm_library_dir = ensure_bgm_library_dir();
        let bgm_library_files = scan_bgm_library(&bgm_library_dir);
        let bgm_library_message = format!(
            "BGMライブラリ: {} 曲 / {}",
            bgm_library_files.len(),
            bgm_library_dir.display()
        );

        let overlay_source = OverlaySource::default();
        let image_overlays = Vec::new();
        let app_started_at = Instant::now();
        let preview_snapshot = Arc::new(RwLock::new(PreviewRenderSnapshot {
            layers: layers.clone(),
            overlay_source: overlay_source.clone(),
            image_overlays: image_overlays.clone(),
            selected_layer: None,
            fish_layer_id,
            app_started_at,
        }));
        let streamed_preview_frame = Arc::new(RwLock::new(None));
        let preview_ui_state = Arc::new(Mutex::new(DeferredPreviewUiState::default()));
        let (preview_command_tx, preview_command_rx) = mpsc::channel();

        let mixer_snapshot = Arc::new(RwLock::new(MixerRenderSnapshot {
            bgm_rows: Vec::new(),
            device_switch_enabled: true,
            device_switch_reason: None,
        }));
        let mixer_ui_state = Arc::new(Mutex::new(DeferredMixerUiState {
            audio_message: audio_message.clone(),
            ..DeferredMixerUiState::default()
        }));
        let (mixer_command_tx, mixer_command_rx) = mpsc::channel();
        let runtime_frame_counters = Arc::new(RuntimeFrameCounters::default());

        Self {
            vm,
            elapsed_ms: 0,
            stream_title: "燕 / Tsubame".to_owned(),
            windows,
            selected_source,
            capture,
            capture_target_fps,
            preview_size: [0, 0],
            preview_snapshot,
            streamed_preview_frame,
            preview_ui_state,
            preview_command_tx,
            preview_command_rx,
            preview_window_open: persisted_settings.preview_window_open,
            preview_window_position: sanitize_window_position(
                persisted_settings.preview_window_position,
            ),
            preview_window_size: sanitize_window_size(persisted_settings.preview_window_size),
            mixer_window_open: persisted_settings.mixer_window.open,
            mixer_window_position: sanitize_window_position(
                persisted_settings.mixer_window.position,
            ),
            mixer_window_size: sanitize_window_size(persisted_settings.mixer_window.size),
            mixer_snapshot,
            mixer_ui_state,
            mixer_command_tx,
            mixer_command_rx,
            capture_message,
            font_message,
            audio,
            recording: None,
            finalize_rx: None,
            recording_message: "録画: 待機中".to_owned(),
            ffmpeg_location: ffmpeg_location_string(),
            encoder_preference,
            recording_preview_mode,
            capture_preview_mode: PreviewMode::Fps30,
            streaming_target,
            show_stream_key: false,
            overlay_source,
            layers,
            selected_layer: None,
            next_layer_id: fish_layer_id + 1,
            fish_layer_id,
            preview_resize_drag: false,
            image_overlays,
            image_overlay_message: "画像オーバーレイ: 未追加".to_owned(),
            image_library_dir,
            image_library_files,
            image_library_message,
            bgm_layers: Vec::new(),
            bgm_players: HashMap::new(),
            bgm_pcm_senders: HashMap::new(),
            bgm_audio_channels: HashMap::new(),
            bgm_library_dir,
            bgm_library_files,
            bgm_library_message,
            bgm_message: "BGM: 未追加".to_owned(),
            app_started_at,
            streaming: None,
            streaming_message: "配信: 待機中".to_owned(),
            performance: PerformanceMonitor::new(),
            runtime_frame_counters,
            settings_open: false,
            settings_page: SettingsPage::General,
            addon_registry: AddonRegistry::new(),
            external_addon_message: "外部アドオン: 未追加".to_owned(),
            persisted_settings,
            last_saved_settings,
            settings_save_message,
            last_settings_save_at: Instant::now(),
            first_run_open,
        }
    }

    fn collect_persisted_settings(&self) -> AppSettings {
        let mut settings = self.persisted_settings.clone();
        settings.preset = match self.vm.session.preset {
            StreamPreset::Game => "game",
            StreamPreset::Work => "work",
            StreamPreset::Light => "light",
        }
        .to_owned();
        settings.encoder_preference = match self.encoder_preference {
            EncoderPreference::Auto => "auto",
            EncoderPreference::Nvenc => "nvenc",
            EncoderPreference::Amf => "amf",
            EncoderPreference::QuickSync => "qsv",
            EncoderPreference::Cpu => "cpu",
        }
        .to_owned();
        settings.recording_preview_mode = match self.recording_preview_mode {
            PreviewMode::Fps30 => "fps30",
            PreviewMode::Fps15 => "fps15",
            PreviewMode::Off => "off",
        }
        .to_owned();
        settings.streaming_platform = match self.streaming_target.platform {
            StreamingPlatform::YouTube => "youtube",
            StreamingPlatform::Twitch => "twitch",
        }
        .to_owned();
        settings.streaming_server_url = self.streaming_target.server_url.clone();
        settings.streaming_video_bitrate_kbps = self.streaming_target.video_bitrate_kbps;
        settings.preview_window_open = self.preview_window_open;
        settings.preview_window_position =
            sanitize_window_position(self.preview_window_position);
        settings.preview_window_size = sanitize_window_size(self.preview_window_size);
        settings.mixer_window.open = self.mixer_window_open;
        settings.mixer_window.position = sanitize_window_position(self.mixer_window_position);
        settings.mixer_window.size = sanitize_window_size(self.mixer_window_size);

        if let Some(audio) = &self.audio {
            settings.input_device_id = match audio.selected_input_device() {
                AudioDeviceSelection::Default => None,
                AudioDeviceSelection::DeviceId(id) => Some(id),
            };
            settings.output_device_id = match audio.selected_output_device() {
                AudioDeviceSelection::Default => None,
                AudioDeviceSelection::DeviceId(id) => Some(id),
            };
        }
        settings
    }

    fn save_settings_if_due(&mut self) {
        if self.last_settings_save_at.elapsed() < Duration::from_secs(1) {
            return;
        }
        self.last_settings_save_at = Instant::now();
        let current = self.collect_persisted_settings();
        if current == self.last_saved_settings {
            return;
        }
        match save_settings(&current) {
            Ok(path) => {
                self.settings_save_message = format!("設定保存: {}", path.display());
                self.persisted_settings = current.clone();
                self.last_saved_settings = current;
            }
            Err(err) => {
                self.settings_save_message = format!("設定保存失敗: {err}");
            }
        }
    }

    fn show_first_run_window(&mut self, ctx: &egui::Context) {
        if !self.first_run_open {
            return;
        }
        egui::Window::new("燕へようこそ")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.heading("燕 / Tsubame");
                ui.label("軽量・安定を優先した配信／録画ソフトです。");
                ui.add_space(6.0);
                ui.label("設定は自動保存され、次回起動時に復元されます。");
                ui.label("安全のためストリームキーだけは平文保存しません。");
                ui.label("配信前に設定画面で音声デバイスと配信先を確認してください。");
                ui.add_space(10.0);
                if ui.button("はじめる").clicked() {
                    self.first_run_open = false;
                    self.persisted_settings.first_run_complete = true;
                    self.last_settings_save_at = Instant::now() - Duration::from_secs(2);
                }
            });
    }

    fn set_stream_preset(&mut self, preset: StreamPreset) {
        let new_fps = preset.dimensions().2;
        self.vm.set_preset(preset);

        if new_fps != self.capture_target_fps
            && self.recording.is_none()
            && self.finalize_rx.is_none()
        {
            self.capture_target_fps = new_fps;
            let source = self.selected_source.clone();
            self.switch_source(source);
        }
    }

    fn refresh_windows(&mut self) {
        match enumerate_windows() {
            Ok(windows) => {
                self.windows = windows;
                self.capture_message = format!("ウィンドウ一覧を更新: {} 件", self.windows.len());
            }
            Err(err) => {
                self.capture_message = format!("ウィンドウ一覧取得失敗: {err}");
            }
        }
    }

    fn switch_source(&mut self, source: CaptureSource) {
        let name = source.display_name().to_owned();

        // Drop previous worker first. Worker owns its capture thread and joins
        // during Drop, keeping capture lifetime localized.
        self.capture = None;
        self.preview_size = [0, 0];
        if let Ok(mut state) = self.preview_ui_state.lock() {
            state.texture = None;
            state.resize_drag_layer = None;
        }
        if let Ok(mut frame) = self.streamed_preview_frame.write() {
            *frame = None;
        }

        match CaptureWorker::start(source.clone(), self.capture_target_fps) {
            Ok(worker) => {
                self.selected_source = source;
                self.capture = Some(worker);
                self.capture_preview_mode = PreviewMode::Fps30;
                self.capture_message =
                    format!("WGC: {name} / target {} FPS", self.capture_target_fps);
            }
            Err(err) => {
                self.capture_message = format!("キャプチャ切替失敗: {err}");
            }
        }
    }

    fn desired_capture_preview_mode(&self) -> PreviewMode {
        // Live streaming still consumes the CPU-readable preview frame path.
        // Keep 30 FPS for the streamer, even when the preview window is hidden.
        if self.streaming.is_some() {
            return PreviewMode::Fps30;
        }

        // A visible edit preview should stay responsive, but 30 FPS is enough
        // for composition/dragging and avoids wasting CPU on 60 FPS UI work.
        if self.preview_window_open {
            return PreviewMode::Fps30;
        }

        // Recording is GPU-direct. Only keep the user's requested recording
        // preview cadence when needed; otherwise the CPU readback can stop.
        if self.recording.is_some() {
            return self.recording_preview_mode;
        }

        // After a source switch we need one preview frame to learn its size.
        if self.preview_size[0] == 0 || self.preview_size[1] == 0 {
            return PreviewMode::Fps30;
        }

        PreviewMode::Off
    }

    fn sync_capture_preview_mode(&mut self) {
        let desired = self.desired_capture_preview_mode();
        if desired == self.capture_preview_mode {
            return;
        }

        if let Some(capture) = &self.capture {
            if capture
                .gpu_recording_handle()
                .set_preview_mode(desired)
                .is_ok()
            {
                self.capture_preview_mode = desired;
            }
        }
    }

    fn make_preview_snapshot(&self) -> PreviewRenderSnapshot {
        PreviewRenderSnapshot {
            layers: self.layers.clone(),
            overlay_source: self.overlay_source.clone(),
            image_overlays: self.image_overlays.clone(),
            selected_layer: self.selected_layer,
            fish_layer_id: self.fish_layer_id,
            app_started_at: self.app_started_at.clone(),
        }
    }

    fn sync_preview_snapshot(&self) {
        if let Ok(mut snapshot) = self.preview_snapshot.write() {
            *snapshot = self.make_preview_snapshot();
        }
    }

    fn sync_preview_dimensions_from_latest(&mut self) {
        let Some(capture) = &self.capture else {
            return;
        };
        if let Some(frame) = capture.latest_preview_frame() {
            self.preview_size = [frame.width, frame.height];
        }
    }

    fn drain_preview_commands(&mut self) {
        while let Ok(command) = self.preview_command_rx.try_recv() {
            match command {
                PreviewViewportCommand::Close => {
                    self.preview_window_open = false;
                }
                PreviewViewportCommand::Geometry { position, size } => {
                    self.preview_window_position = sanitize_window_position(position);
                    self.preview_window_size = sanitize_window_size(size);
                }
                PreviewViewportCommand::Select(layer_id) => {
                    self.selected_layer = layer_id;
                    self.preview_resize_drag = false;
                }
                PreviewViewportCommand::Move {
                    layer_id,
                    delta_x,
                    delta_y,
                    preview_width,
                    preview_height,
                    source_width,
                    source_height,
                } => {
                    if self.layer_locked(layer_id) {
                        continue;
                    }
                    self.selected_layer = Some(layer_id);
                    if layer_id == self.fish_layer_id {
                        self.overlay_source.move_by_preview_delta(
                            delta_x,
                            delta_y,
                            preview_width,
                            preview_height,
                            source_width,
                            source_height,
                        );
                    } else if let Some((_, overlay)) = self
                        .image_overlays
                        .iter_mut()
                        .find(|(id, _)| *id == layer_id)
                    {
                        overlay.move_by_preview_delta(
                            delta_x,
                            delta_y,
                            preview_width,
                            preview_height,
                            source_width,
                            source_height,
                        );
                    }
                }
                PreviewViewportCommand::Resize {
                    layer_id,
                    delta_x,
                    preview_width,
                    source_width,
                    source_height,
                } => {
                    if self.layer_locked(layer_id) {
                        continue;
                    }
                    self.selected_layer = Some(layer_id);
                    if layer_id == self.fish_layer_id {
                        self.overlay_source.resize_by_preview_delta(
                            delta_x,
                            preview_width,
                            source_width,
                            source_height,
                        );
                    } else if let Some((_, overlay)) = self
                        .image_overlays
                        .iter_mut()
                        .find(|(id, _)| *id == layer_id)
                    {
                        overlay.resize_by_preview_delta(
                            delta_x,
                            preview_width,
                            source_width,
                            source_height,
                        );
                    }
                }
            }
        }
    }

    fn sync_mixer_snapshot(&self) {
        if let Ok(mut snapshot) = self.mixer_snapshot.write() {
            snapshot.bgm_rows = self
                .bgm_layers
                .iter()
                .map(|(id, source)| {
                    (
                        *id,
                        self.bgm_audio_channels.get(id).copied(),
                        source.name.clone(),
                        source.volume_percent,
                        source.muted,
                    )
                })
                .collect();
            snapshot.device_switch_enabled = self.recording.is_none()
                && self.finalize_rx.is_none()
                && self.streaming.is_none();
            snapshot.device_switch_reason = if self.streaming.is_some() {
                Some("配信中は音声デバイスとアプリ構成を固定します".to_owned())
            } else if self.recording.is_some() || self.finalize_rx.is_some() {
                Some("録画中は音声デバイスとアプリ構成を固定します".to_owned())
            } else {
                None
            };
        }
    }

    fn drain_mixer_commands(&mut self) {
        while let Ok(command) = self.mixer_command_rx.try_recv() {
            match command {
                MixerViewportCommand::Close => {
                    self.mixer_window_open = false;
                }
                MixerViewportCommand::Geometry { position, size } => {
                    self.mixer_window_position = sanitize_window_position(position);
                    self.mixer_window_size = sanitize_window_size(size);
                }
                MixerViewportCommand::BgmGain {
                    layer_id,
                    gain_percent,
                } => {
                    if let Some((_, source)) = self
                        .bgm_layers
                        .iter_mut()
                        .find(|(id, _)| *id == layer_id)
                    {
                        source.volume_percent = gain_percent.clamp(0.0, 100.0);
                        if let Some(player) = self.bgm_players.get(&layer_id) {
                            player.set_volume(source.effective_volume());
                        }
                        if let Some(channel_id) = self.bgm_audio_channels.get(&layer_id).copied() {
                            if let Some(audio) = self.audio.as_ref() {
                                audio.set_channel_gain(channel_id, source.volume_linear());
                            }
                        }
                    }
                }
                MixerViewportCommand::BgmMute { layer_id, muted } => {
                    if let Some((_, source)) = self
                        .bgm_layers
                        .iter_mut()
                        .find(|(id, _)| *id == layer_id)
                    {
                        source.muted = muted;
                        if let Some(player) = self.bgm_players.get(&layer_id) {
                            player.set_volume(source.effective_volume());
                        }
                        if let Some(channel_id) = self.bgm_audio_channels.get(&layer_id).copied() {
                            if let Some(audio) = self.audio.as_ref() {
                                audio.set_channel_muted(channel_id, muted);
                            }
                        }
                    }
                }
                MixerViewportCommand::RemoveBgm { layer_id } => {
                    self.remove_bgm_layer(layer_id);
                    self.bgm_message = if self.bgm_layers.is_empty() {
                        "BGM: 未追加".to_owned()
                    } else {
                        format!("BGM削除 / 残り {} 曲", self.bgm_layers.len())
                    };
                }
            }
        }
    }

    fn push_streaming_frame_if_available(&mut self) {
        if self.streaming.is_none() {
            return;
        }

        let Some(capture) = &self.capture else {
            return;
        };
        let mut newest = None;
        while let Ok(frame) = capture.try_recv() {
            newest = Some(frame);
        }
        let Some(frame) = newest else {
            return;
        };

        self.preview_size = [frame.width, frame.height];
        let snapshot = self.make_preview_snapshot();
        let composed_frame = compose_preview_frame(frame, &snapshot);
        if let Ok(mut preview_frame) = self.streamed_preview_frame.write() {
            *preview_frame = Some(composed_frame.clone());
        }
        if let Some(streaming) = &self.streaming {
            streaming.push_video_frame(&composed_frame);
            self.runtime_frame_counters.count_streaming_output_frame();
        }
    }

    fn start_recording(&mut self) {
        if self.recording.is_some() || self.finalize_rx.is_some() {
            return;
        }

        let (out_w, out_h, fps) = self.vm.session.preset.dimensions();

        let config = RecordingConfig {
            source_width: self.preview_size[0],
            source_height: self.preview_size[1],
            output_width: out_w,
            output_height: out_h,
            fps,
            bitrate_kbps: 6000,
            encoder_preference: self.encoder_preference,
        };

        let Some(capture) = &self.capture else {
            self.recording_message = "録画開始失敗: キャプチャがありません".to_owned();
            return;
        };
        let gpu = capture.gpu_recording_handle();

        let mixer = self
            .audio
            .as_ref()
            .map(|audio| audio.mixer_control())
            .unwrap_or_else(MixerControl::default);

        let audio_devices = self
            .audio
            .as_ref()
            .map(|audio| audio.device_state())
            .unwrap_or_default();

        let (channel_mixer, external_pcm_sources) = self
            .audio
            .as_ref()
            .map(|audio| {
                (
                    audio.channel_mixer(),
                    audio.external_pcm_recording_sources(),
                )
            })
            .unwrap_or_else(|| (ChannelMixerControl::default(), Vec::new()));

        match RecordingSession::start_with_audio_routing_and_external(
            "recordings",
            config,
            gpu,
            mixer,
            channel_mixer,
            audio_devices,
            external_pcm_sources,
        ) {
            Ok(session) => {
                self.recording_message = format!(
                    "録画中: {} / {}",
                    session.backend_name,
                    session.paths.root.display()
                );
                self.recording = Some(session);
            }
            Err(err) => {
                self.recording_message = format!("録画開始失敗: {err}");
            }
        }
    }

    fn stop_recording(&mut self) {
        let Some(session) = self.recording.take() else {
            return;
        };

        let mut manifest = stream_core::EditManifest::new(self.vm.session.clone());
        manifest.markers = self.vm.markers.clone();

        self.recording_message = "録画停止処理中… MP4を仕上げています".to_owned();
        self.finalize_rx = Some(session.stop_async(manifest));
    }

    fn poll_recording_finalize(&mut self) {
        let Some(rx) = &self.finalize_rx else {
            return;
        };

        match rx.try_recv() {
            Ok(Ok(paths)) => {
                self.recording_message = format!("録画完了: {}", paths.final_video.display());
                self.finalize_rx = None;
            }
            Ok(Err(err)) => {
                self.recording_message = format!("録画終了エラー: {err}");
                self.finalize_rx = None;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.recording_message = "録画終了処理が予期せず終了しました".to_owned();
                self.finalize_rx = None;
            }
        }
    }

    fn start_streaming(&mut self) {
        if self.streaming.is_some() {
            return;
        }
        let (w, h, fps) = self.vm.session.preset.dimensions();
        let live_audio = match self.audio.as_ref() {
            Some(audio) => match audio.start_live_audio_bridge() {
                Ok(bridge) => Some(bridge),
                Err(err) => {
                    self.streaming_message = format!("ライブ音声開始失敗: {err}");
                    return;
                }
            },
            None => None,
        };
        let live_audio_count = live_audio
            .as_ref()
            .map(|bridge| bridge.inputs.len())
            .unwrap_or(0);
        match StreamingSession::start(
            &self.streaming_target,
            self.preview_size[0],
            self.preview_size[1],
            w,
            h,
            fps,
            live_audio,
        ) {
            Ok(session) => {
                self.streaming_message = format!(
                    "送信中: {} / {}×{} {} FPS / 映像: {} / ライブ音声 {}ch（受信確認は配信サービス側）",
                    session.platform().label(),
                    w,
                    h,
                    fps,
                    self.selected_source.display_name(),
                    live_audio_count
                );
                self.streaming = Some(session);
                self.vm.is_live = true;
            }
            Err(err) => {
                self.streaming_message = format!("配信開始失敗: {err}");
                self.vm.is_live = false;
            }
        }
    }

    fn stop_streaming(&mut self) {
        if let Some(session) = self.streaming.take() {
            session.stop();
        }
        self.streaming_message = "配信: 停止しました".to_owned();
        self.vm.is_live = false;
    }

    fn poll_streaming(&mut self) {
        let Some(streaming) = &mut self.streaming else {
            return;
        };
        match streaming.try_exit() {
            Ok(Some(status)) => {
                let diagnostic = streaming.diagnostic_summary();
                self.streaming_message =
                    format!("配信プロセス終了: {status} / FFmpeg: {diagnostic}");
                self.streaming = None;
                self.vm.is_live = false;
            }
            Ok(None) => {}
            Err(err) => {
                self.streaming_message = format!("配信状態確認失敗: {err}");
                self.streaming = None;
                self.vm.is_live = false;
            }
        }
    }

    fn source_selector(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("キャプチャ対象");
            if self.recording.is_none()
                && self.finalize_rx.is_none()
                && ui.button("一覧更新").clicked()
            {
                self.refresh_windows();
            }
        });

        let selected_name = self.selected_source.display_name().to_owned();

        // フルのウィンドウ名は固定幅の折り返し領域へ表示する。
        // タイトル文字数でSidePanel自体が広がらないようにする。
        ui.scope(|ui| {
            ui.set_min_width(SOURCE_SELECTOR_WIDTH);
            ui.set_max_width(SOURCE_SELECTOR_WIDTH);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(&selected_name)
                        .small()
                        .color(egui::Color32::LIGHT_GRAY),
                )
                .wrap(),
            );
        });

        if self.recording.is_some() || self.finalize_rx.is_some() || self.streaming.is_some() {
            ui.small(if self.streaming.is_some() {
                "配信中はソース切替を固定しています"
            } else {
                "録画中はソース切替を固定しています"
            });
            return;
        }

        let compact_selected = compact_source_name(&selected_name, 28);
        let mut requested: Option<CaptureSource> = None;

        egui::ComboBox::from_id_salt("capture-source")
            .selected_text(compact_selected)
            .width(SOURCE_SELECTOR_WIDTH)
            .show_ui(ui, |ui| {
                ui.set_min_width(SOURCE_SELECTOR_WIDTH);
                ui.set_max_width(SOURCE_SELECTOR_WIDTH);

                if ui
                    .selectable_label(
                        matches!(self.selected_source, CaptureSource::Desktop),
                        "デスクトップ全体",
                    )
                    .clicked()
                {
                    requested = Some(CaptureSource::Desktop);
                }

                ui.separator();

                for window in &self.windows {
                    let selected = match &self.selected_source {
                        CaptureSource::Window(current) => current.hwnd == window.hwnd,
                        CaptureSource::Desktop => false,
                    };

                    let compact_title = compact_source_name(&window.title, 36);
                    let response = ui.selectable_label(selected, compact_title);
                    if response.hovered() {
                        response.clone().on_hover_text(&window.title);
                    }
                    if response.clicked() {
                        requested = Some(CaptureSource::Window(window.clone()));
                    }
                }
            });

        if let Some(source) = requested {
            if source != self.selected_source {
                self.switch_source(source);
            }
        }
    }

    fn capture_source_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("映像ソース");
        self.source_selector(ui);
        ui.small(&self.capture_message);

        egui::CollapsingHeader::new("詳細情報")
            .id_salt("capture_details")
            .default_open(false)
            .show(ui, |ui| {
                if let Some(capture) = &self.capture {
                    let stats = capture.stats();
                    ui.small(format!(
                        "WGC callback: {:.1} FPS / frames {}",
                        stats.measured_fps, stats.received_frames
                    ));
                    ui.small(format!(
                        "540p async readback: avg {:.2} ms / max {:.2} ms",
                        stats.gpu_to_cpu_ms_avg, stats.gpu_to_cpu_ms_max
                    ));
                    ui.small(format!(
                        "Preview worker frames: {} / jobs dropped: {}",
                        stats.preview_worker_frames, stats.preview_jobs_dropped
                    ));
                }
                ui.separator();
                ui.small(format!("FFmpeg: {}", self.ffmpeg_location));
                ui.small("録画Encoder: DirectX GPU / Windows Media Foundation H.264");
                ui.small("配信Encoder: FFmpeg h264_mf / RTMP・RTMPS");

                if self.recording.is_none() && self.finalize_rx.is_none() {
                    ui.horizontal(|ui| {
                        ui.label("録画中プレビュー");
                        egui::ComboBox::from_id_salt("recording-preview-mode")
                            .selected_text(match self.recording_preview_mode {
                                PreviewMode::Fps30 => "30 FPS",
                                PreviewMode::Fps15 => "15 FPS",
                                PreviewMode::Off => "OFF",
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut self.recording_preview_mode,
                                    PreviewMode::Fps15,
                                    "15 FPS（推奨）",
                                );
                                ui.selectable_value(
                                    &mut self.recording_preview_mode,
                                    PreviewMode::Off,
                                    "OFF（最軽量）",
                                );
                                ui.selectable_value(
                                    &mut self.recording_preview_mode,
                                    PreviewMode::Fps30,
                                    "30 FPS",
                                );
                            });
                    });
                }

                if let Some(recording) = &self.recording {
                    ui.small(format!(
                        "GPU encoded frames: {} / status: {:?}",
                        recording.encoded_frames(),
                        recording.gpu_status()
                    ));
                }
            });
    }

    fn show_mixer_viewport(&mut self, ctx: &egui::Context) {
        if !self.mixer_window_open {
            return;
        }

        self.mixer_window_position = sanitize_window_position(self.mixer_window_position);
        self.mixer_window_size = sanitize_window_size(self.mixer_window_size);

        if let Ok(mut state) = self.mixer_ui_state.lock() {
            if state.last_window_position.is_none() {
                state.last_window_position = self.mixer_window_position;
            }
            if state.last_window_size.is_none() {
                state.last_window_size = self.mixer_window_size;
            }
        }

        let viewport_id = egui::ViewportId::from_hash_of("tsubame_audio_mixer_window");
        let mut builder = egui::ViewportBuilder::default()
            .with_title("燕 - 音声ミキサー")
            .with_inner_size(self.mixer_window_size.unwrap_or([900.0, 560.0]))
            .with_min_inner_size([520.0, 360.0]);
        if let Some([x, y]) = self.mixer_window_position {
            builder = builder.with_position([x, y]);
        }

        let audio = self.audio.as_ref().map(Arc::clone);
        let snapshot = Arc::clone(&self.mixer_snapshot);
        let ui_state = Arc::clone(&self.mixer_ui_state);
        let command_tx = self.mixer_command_tx.clone();
        let runtime_frame_counters = Arc::clone(&self.runtime_frame_counters);

        ctx.show_viewport_deferred(viewport_id, builder, move |mixer_ctx, _class| {
            runtime_frame_counters.count_mixer_ui_frame();
            let viewport_info = mixer_ctx.input(|input| input.viewport().clone());
            if viewport_info.close_requested() {
                send_mixer_command(mixer_ctx, &command_tx, MixerViewportCommand::Close);
                return;
            }

            let position = viewport_info
                .outer_rect
                .map(|rect| [rect.min.x, rect.min.y]);
            let size = viewport_info
                .inner_rect
                .map(|rect| [rect.width(), rect.height()]);

            let mut geometry_command = None;
            if let Ok(mut state) = ui_state.lock() {
                let observed_position = position.or(state.last_window_position);
                let observed_size = size.or(state.last_window_size);
                let changed = geometry_changed(state.last_window_position, observed_position)
                    || geometry_changed(state.last_window_size, observed_size);
                if changed {
                    state.last_window_position = observed_position;
                    state.last_window_size = observed_size;
                    geometry_command = Some(MixerViewportCommand::Geometry {
                        position: observed_position,
                        size: observed_size,
                    });
                }
            }
            if let Some(command) = geometry_command {
                send_mixer_command(mixer_ctx, &command_tx, command);
            }

            let snapshot_value = snapshot.read().map(|value| value.clone()).unwrap_or_default();
            let now = Instant::now();
            let meter_snapshot = if let Ok(mut state) = ui_state.lock() {
                if mixer_meter_refresh_due(state.last_meter_refresh, now) {
                    if let Some(audio) = audio.as_ref() {
                        let levels = audio.levels();
                        state.meter_snapshot.desktop = levels.desktop;
                        state.meter_snapshot.mic = levels.mic;
                        state.meter_snapshot.mix = levels.mix;
                        state.meter_snapshot.application_levels = audio
                            .audio_channels()
                            .into_iter()
                            .map(|channel| (channel.id, channel.current_level))
                            .collect();
                    } else {
                        state.meter_snapshot = MixerMeterSnapshot::default();
                    }
                    state.last_meter_refresh = Some(now);
                    runtime_frame_counters.count_mixer_meter_update();
                }
                state.meter_snapshot.clone()
            } else {
                MixerMeterSnapshot::default()
            };

            egui::CentralPanel::default().show(mixer_ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("音声ミキサー");
                    ui.separator();
                    ui.small("閉じても配信・録画・BGM音声は継続");
                    if ui.button("閉じる").clicked() {
                        send_mixer_command(mixer_ctx, &command_tx, MixerViewportCommand::Close);
                    }
                });
                ui.separator();
                draw_deferred_audio_mixer(
                    mixer_ctx,
                    ui,
                    audio.as_ref(),
                    &meter_snapshot,
                    &snapshot_value,
                    &ui_state,
                    &command_tx,
                );
            });

            mixer_ctx.request_repaint_after(Duration::from_millis(mixer_ui_repaint_ms(true)));
        });
    }

    fn show_preview_viewport(&mut self, ctx: &egui::Context) {
        if !self.preview_window_open {
            return;
        }

        self.preview_window_position = sanitize_window_position(self.preview_window_position);
        self.preview_window_size = sanitize_window_size(self.preview_window_size);

        let [default_w, default_h] = normal_preview_size();
        let viewport_id = egui::ViewportId::from_hash_of("yaoyorozu_preview_window");
        let mut builder = egui::ViewportBuilder::default()
            .with_title("燕 / Tsubame Preview")
            .with_inner_size(
                self.preview_window_size
                    .unwrap_or([default_w + 40.0, default_h + 80.0]),
            )
            .with_min_inner_size([360.0, 240.0]);
        if let Some([x, y]) = self.preview_window_position {
            builder = builder.with_position([x, y]);
        }

        let latest_frame: Option<LatestFrameSnapshot> =
            self.capture.as_ref().map(|capture| capture.latest_preview_handle());
        let streamed_frame = self
            .streaming
            .as_ref()
            .map(|_| Arc::clone(&self.streamed_preview_frame));
        let snapshot = Arc::clone(&self.preview_snapshot);
        let ui_state = Arc::clone(&self.preview_ui_state);
        let command_tx = self.preview_command_tx.clone();
        let runtime_frame_counters = Arc::clone(&self.runtime_frame_counters);

        ctx.show_viewport_deferred(viewport_id, builder, move |preview_ctx, _class| {
            runtime_frame_counters.count_preview_ui_frame();
            let viewport_info = preview_ctx.input(|input| input.viewport().clone());
            if viewport_info.close_requested() {
                send_preview_command(preview_ctx, &command_tx, PreviewViewportCommand::Close);
                return;
            }

            let position = viewport_info
                .outer_rect
                .map(|rect| [rect.min.x, rect.min.y]);
            let size = viewport_info
                .inner_rect
                .map(|rect| [rect.width(), rect.height()]);

            let mut geometry_command = None;
            if let Ok(mut state) = ui_state.lock() {
                // Some platforms briefly report one of these rectangles as None while
                // a native viewport is being created. Keep the last known value so we
                // never erase a valid sub-monitor restore position with a transient None.
                let observed_position = position.or(state.last_window_position);
                let observed_size = size.or(state.last_window_size);
                let changed = geometry_changed(state.last_window_position, observed_position)
                    || geometry_changed(state.last_window_size, observed_size);
                if changed {
                    state.last_window_position = observed_position;
                    state.last_window_size = observed_size;
                    geometry_command = Some(PreviewViewportCommand::Geometry {
                        position: observed_position,
                        size: observed_size,
                    });
                }
            }
            if let Some(command) = geometry_command {
                send_preview_command(preview_ctx, &command_tx, command);
            }

            let snapshot_value = match snapshot.read() {
                Ok(snapshot) => snapshot.clone(),
                Err(_) => {
                    preview_ctx.request_repaint_after(Duration::from_millis(33));
                    return;
                }
            };

            egui::CentralPanel::default().show(preview_ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("プレビュー");
                    ui.separator();
                    ui.small("閉じても配信・録画は続行");
                    if ui.button("閉じる").clicked() {
                        send_preview_command(
                            preview_ctx,
                            &command_tx,
                            PreviewViewportCommand::Close,
                        );
                    }
                });
                ui.separator();
                let available = ui.available_size();
                draw_deferred_preview_canvas(
                    preview_ctx,
                    ui,
                    available,
                    &snapshot_value,
                    latest_frame.as_ref(),
                    streamed_frame.as_ref(),
                    &ui_state,
                    &command_tx,
                );
            });

            // Deferred viewport repaints independently from the main control UI.
            preview_ctx.request_repaint_after(Duration::from_millis(33));
        });
    }

    fn show_settings_window(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }

        let mut open = self.settings_open;
        egui::Window::new("燕 設定")
            .open(&mut open)
            .default_width(720.0)
            .default_height(520.0)
            .min_width(560.0)
            .min_height(420.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut self.settings_page, SettingsPage::General, "通常設定");
                    ui.selectable_value(
                        &mut self.settings_page,
                        SettingsPage::OfficialAddons,
                        "公式アドオン",
                    );
                    ui.selectable_value(
                        &mut self.settings_page,
                        SettingsPage::ExternalAddons,
                        "外部アドオン",
                    );
                });
                ui.separator();

                egui::ScrollArea::vertical()
                    .id_salt("tsubame_settings_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| match self.settings_page {
                        SettingsPage::General => {
                            ui.heading("通常設定");
                            ui.small("Phase 10.0.0では設定画面の基盤を先に用意します。");
                            ui.add_space(8.0);
                            ui.group(|ui| {
                                ui.label("アプリケーション");
                                ui.small("表示名: 燕 / Tsubame");
                                ui.small(format!("Addon API: v{}", ADDON_API_VERSION));
                                ui.small("コア機能: キャプチャ / 配信 / 録画 / 音声");
                            });
                            ui.add_space(8.0);
                            ui.group(|ui| {
                                ui.label("プレビュー");
                                ui.checkbox(&mut self.preview_window_open, "起動中にプレビュー窓を表示");
                                ui.small("プレビューを閉じても配信・録画は継続します。");
                            });
                            ui.add_space(8.0);
                            ui.group(|ui| {
                                ui.label("設定保存");
                                ui.small("主要設定は自動保存し、次回起動時に復元します。");
                                ui.small("ストリームキーは安全のため平文保存しません。");
                                ui.small(&self.settings_save_message);
                            });
                            ui.add_space(8.0);
                            ui.group(|ui| {
                                ui.label("設計方針");
                                ui.small("コアと公開Addon APIを分離し、内部更新でアドオンが壊れにくい構成を目指します。");
                            });
                        }
                        SettingsPage::OfficialAddons => {
                            ui.heading("公式アドオン");
                            ui.small(format!(
                                "登録 {} 件 / Addon API v{}",
                                self.addon_registry.official_count(),
                                ADDON_API_VERSION
                            ));
                            ui.add_space(8.0);

                            for addon in self.addon_registry.addons_mut() {
                                if addon.origin != AddonOrigin::Official {
                                    continue;
                                }
                                ui.group(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.checkbox(&mut addon.enabled, "有効");
                                        ui.strong(&addon.name);
                                    });
                                    ui.small(format!(
                                        "{} / v{} / API v{} / {}",
                                        addon.id,
                                        addon.version,
                                        addon.required_api,
                                        addon.compatibility_label()
                                    ));
                                    ui.small("※ Phase 10.0.0では管理基盤のみ。機能接続は後続Phaseで行います。");
                                });
                                ui.add_space(6.0);
                            }
                        }
                        SettingsPage::ExternalAddons => {
                            ui.heading("外部アドオン");
                            ui.small("Blenderのように、外部アドオンを追加するための入口です。");
                            ui.small("安全のためPhase 10.0.0では実行せず、登録と互換性確認の枠だけを作ります。");
                            ui.add_space(8.0);

                            if ui.button("＋ 外部アドオンを追加").clicked() {
                                if let Some(path) = rfd::FileDialog::new()
                                    .add_filter("燕アドオン候補", &["zip", "json", "toml"])
                                    .pick_file()
                                {
                                    self.external_addon_message = format!(
                                        "外部アドオン候補を登録: {}",
                                        path.display()
                                    );
                                    self.addon_registry.register_external_placeholder(path);
                                }
                            }
                            ui.small(&self.external_addon_message);
                            ui.separator();
                            ui.small(format!(
                                "登録済み外部アドオン: {} 件",
                                self.addon_registry.external_count()
                            ));

                            for addon in self.addon_registry.addons_mut() {
                                if addon.origin != AddonOrigin::External {
                                    continue;
                                }
                                ui.group(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.add_enabled_ui(addon.is_compatible(), |ui| {
                                            ui.checkbox(&mut addon.enabled, "有効");
                                        });
                                        ui.strong(&addon.name);
                                    });
                                    if let Some(path) = &addon.path {
                                        ui.small(path.display().to_string());
                                    }
                                    ui.small(format!(
                                        "API v{} / {}",
                                        addon.required_api,
                                        addon.compatibility_label()
                                    ));
                                });
                                ui.add_space(6.0);
                            }
                        }
                    });
            });

        self.settings_open = open;
    }
}

fn toggle_window_open(open: &mut bool) {
    *open = !*open;
}

impl eframe::App for YaoyorozuApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_preview_commands();
        self.drain_mixer_commands();
        self.bgm_players.retain(|_, player| !player.is_finished());
        self.poll_recording_finalize();
        self.poll_streaming();
        let recording_encoded_frames = self
            .recording
            .as_ref()
            .map(|recording| recording.encoded_frames());
        let streaming_active = self.streaming.is_some();
        let runtime_frame_counters = Arc::clone(&self.runtime_frame_counters);
        self.performance.refresh_if_due(
            runtime_frame_counters.as_ref(),
            recording_encoded_frames,
            streaming_active,
        );
        self.sync_capture_preview_mode();
        self.sync_preview_dimensions_from_latest();
        self.push_streaming_frame_if_available();
        self.sync_preview_snapshot();
        self.sync_mixer_snapshot();
        self.show_preview_viewport(ctx);
        self.show_mixer_viewport(ctx);
        self.show_settings_window(ctx);
        self.show_first_run_window(ctx);
        self.save_settings_if_due();

        egui::TopBottomPanel::top("tsubame_top_toolbar")
            .exact_height(38.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .selectable_label(self.preview_window_open, "▣ プレビュー")
                        .on_hover_text("プレビューウィンドウを開く / 閉じる")
                        .clicked()
                    {
                        toggle_window_open(&mut self.preview_window_open);
                    }
                    if ui
                        .selectable_label(self.mixer_window_open, "≡ ミキサー")
                        .on_hover_text("音声ミキサーを開く / 閉じる")
                        .clicked()
                    {
                        toggle_window_open(&mut self.mixer_window_open);
                    }
                    if ui
                        .selectable_label(self.settings_open, "⚙ 設定")
                        .on_hover_text("設定ウィンドウを開く / 閉じる")
                        .clicked()
                    {
                        toggle_window_open(&mut self.settings_open);
                    }
                    ui.separator();
                    ui.strong("燕 / Tsubame");
                    ui.separator();
                    ui.small("Core");
                    ui.separator();
                    ui.small(format!("Addon API v{}", ADDON_API_VERSION));
                });
            });

        egui::TopBottomPanel::bottom("post_recording_controls").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.strong("録画後");
                ui.separator();
                if self.vm.can_send_to_aviutl2() {
                    let _ = ui.button("AviUtl2へ送る");
                } else {
                    ui.add_enabled(false, egui::Button::new("AviUtl2へ送る"));
                }

                ui.separator();
                for (label, kind) in [
                    ("CUT", MarkerKind::Cut),
                    ("SHORT", MarkerKind::Short),
                    ("CHAPTER", MarkerKind::Chapter),
                    ("NOTE", MarkerKind::Note),
                ] {
                    if ui.button(label).clicked() {
                        self.vm.add_marker(self.elapsed_ms, kind, label);
                    }
                }
                ui.label(format!("Markers: {}", self.vm.markers.len()));
                ui.separator();
                ui.small(&self.recording_message);

                // Phase 9.3.1.8: 上部ステータスバーを下部の空き領域へ統合。
                // プレビュー・映像ソース・音声ミキサーが使える縦領域を増やす。
                ui.separator();
                ui.strong("燕 / Tsubame");
                ui.separator();
                let (capture_fps, preview_drops) = self
                    .capture
                    .as_ref()
                    .map(|capture| {
                        let stats = capture.stats();
                        (stats.measured_fps, stats.preview_jobs_dropped)
                    })
                    .unwrap_or((0.0, 0));
                ui.label(format!(
                    "CPU {:.1}% | RAM {:.0} MB | {}",
                    self.performance.cpu_percent,
                    self.performance.memory_mb,
                    performance_pipeline_text(
                        capture_fps,
                        self.performance.preview_fps,
                        self.performance.mixer_ui_fps,
                        self.performance.mixer_meter_fps,
                        self.performance.encode_input_fps,
                        self.capture_target_fps,
                        preview_drops,
                    )
                ));
                ui.separator();
                ui.label(self.streaming_target.connection_label());
                ui.separator();
                ui.label(if self.streaming.is_some() {
                    "● 送信中"
                } else {
                    "○ OFFLINE"
                });
            });
        });

        egui::TopBottomPanel::bottom("phase9_scene_panel")
            .exact_height(120.0)
            .show(ctx, |ui| {
                ui.heading("シーン");
                ui.horizontal_wrapped(|ui| {
                    for scene in SCENE_LABELS {
                        let _ = ui.selectable_label(scene == "ゲーム", scene);
                    }
                });
                ui.small("追加・保存は Phase 9.4");
            });

        egui::SidePanel::right("phase9_settings")
            .default_width(350.0)
            .resizable(true)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("phase9_settings_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                ui.heading("配信・録画設定");
                ui.label("配信タイトル");
                ui.text_edit_singleline(&mut self.stream_title);

                ui.separator();
                ui.heading("配信先");
                let mut selected_platform = self.streaming_target.platform;
                egui::ComboBox::from_id_salt("streaming-platform")
                    .selected_text(selected_platform.label())
                    .width(SOURCE_SELECTOR_WIDTH)
                    .show_ui(ui, |ui| {
                        for platform in StreamingPlatform::ALL {
                            ui.selectable_value(
                                &mut selected_platform,
                                platform,
                                platform.label(),
                            );
                        }
                    });
                if selected_platform != self.streaming_target.platform {
                    self.streaming_target.set_platform(selected_platform);
                    self.show_stream_key = false;
                    self.streaming_message = format!(
                        "配信先を{}へ切り替えました。URLとキーを設定してください",
                        selected_platform.label()
                    );
                }

                ui.label("サーバーURL");
                ui.add(
                    egui::TextEdit::singleline(&mut self.streaming_target.server_url)
                        .desired_width(SOURCE_SELECTOR_WIDTH)
                        .hint_text(self.streaming_target.platform.server_hint()),
                );

                ui.horizontal(|ui| {
                    ui.label("ストリームキー");
                    ui.checkbox(&mut self.show_stream_key, "表示");
                });
                let mut key_edit = egui::TextEdit::singleline(&mut self.streaming_target.stream_key)
                    .desired_width(SOURCE_SELECTOR_WIDTH)
                    .hint_text("配信サービスから取得したキー");
                if !self.show_stream_key {
                    key_edit = key_edit.password(true);
                }
                ui.add(key_edit);

                ui.horizontal(|ui| {
                    ui.label("映像bitrate");
                    ui.add(
                        egui::DragValue::new(&mut self.streaming_target.video_bitrate_kbps)
                            .range(500..=50_000)
                            .speed(100.0)
                            .suffix(" kbps"),
                    );
                });
                if self.streaming_target.is_ready() {
                    ui.small("配信先設定: 準備OK");
                } else {
                    ui.small(self.streaming_target.readiness_message());
                }

                ui.label("プリセット");
                ui.horizontal_wrapped(|ui| {
                    if ui.button("ゲーム 1080p60").clicked() {
                        self.set_stream_preset(StreamPreset::Game);
                    }
                    if ui.button("作業 1080p30").clicked() {
                        self.set_stream_preset(StreamPreset::Work);
                    }
                    if ui.button("軽量 720p30").clicked() {
                        self.set_stream_preset(StreamPreset::Light);
                    }
                });

                let (w, h, fps) = self.vm.session.preset.dimensions();
                ui.label(format!("録画出力: {w} × {h} / {fps} FPS"));
                ui.small("※ 300pxプレビューとは独立");

                ui.add_space(10.0);
                ui.separator();
                ui.heading("実行");

                let recording_busy = self.recording.is_some() || self.finalize_rx.is_some();
                let streaming_active = self.streaming.is_some();
                let target_ready = self.streaming_target.is_ready();

                if !recording_busy {
                    if ui.add_sized([SOURCE_SELECTOR_WIDTH, 34.0], egui::Button::new("● 録画開始")).clicked() {
                        self.start_recording();
                    }
                } else if self.recording.is_some() {
                    if ui.add_sized([SOURCE_SELECTOR_WIDTH, 38.0], egui::Button::new("■ 録画停止")).clicked() {
                        self.stop_recording();
                    }
                } else {
                    ui.add_enabled(false, egui::Button::new("MP4仕上げ中…"));
                }

                if !streaming_active {
                    if ui.add_enabled(target_ready, egui::Button::new("● 配信開始")).clicked() {
                        self.start_streaming();
                    }
                } else if ui.button("■ 配信停止").clicked() {
                    self.stop_streaming();
                }

                if !recording_busy && !streaming_active {
                    if ui.add_enabled(target_ready, egui::Button::new("● 録画＋配信")).clicked() {
                        self.start_recording();
                        self.start_streaming();
                    }
                }
                ui.small(&self.streaming_message);
                if let Some(streaming) = &self.streaming {
                    ui.small(format!("配信経過: {} 秒", streaming.elapsed_seconds()));
                }
                ui.small("Phase 9.3.4: 配信中もGain / Mute / 配信Mix / Masterをリアルタイム反映。音声デバイスやアプリ追加・削除は配信停止中に変更します。");
                ui.separator();
                ui.small(&self.font_message);
                    });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            let content_height = ui.available_height();

            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(310.0, content_height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("preview_source_scroll")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.heading("プレビュー");
                        ui.small(format!("{}×{}", self.preview_size[0], self.preview_size[1]));
                    });
                    ui.small(if self.preview_window_open {
                        "別窓表示中・ドラッグ編集対応"
                    } else {
                        "別窓は閉じています"
                    });
                    ui.add_space(6.0);
                    ui.separator();
                    self.capture_source_panel(ui);
                    ui.add_space(8.0);
                    ui.separator();
                    ui.heading("レイヤー");
                    ui.small("上に表示される項目ほど手前。順番はプレビュー・配信映像の合成順にも反映されます。");
                    let selected_layer = self.selected_layer;
                    let mut select_layer = None;
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        for layer in self.layers.iter_mut().rev() {
                            ui.horizontal(|ui| {
                                let selected = selected_layer == Some(layer.id);
                                let label = format!("{}  {}", layer.kind.label(), layer.name);
                                if ui.selectable_label(selected, label).clicked() {
                                    select_layer = Some(layer.id);
                                }
                                ui.checkbox(&mut layer.locked, "ロック");
                            });
                        }
                    });
                    if let Some(id) = select_layer {
                        self.selected_layer = Some(id);
                        self.preview_resize_drag = false;
                    }

                    let selected_index = self
                        .selected_layer
                        .and_then(|id| self.layers.iter().position(|layer| layer.id == id));
                    let can_move_front = selected_index
                        .is_some_and(|index| index + 1 < self.layers.len());
                    let can_move_back = selected_index.is_some_and(|index| index > 0);
                    let mut move_front = false;
                    let mut move_back = false;
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(can_move_front, egui::Button::new("↑ 手前へ"))
                            .clicked()
                        {
                            move_front = true;
                        }
                        if ui
                            .add_enabled(can_move_back, egui::Button::new("↓ 奥へ"))
                            .clicked()
                        {
                            move_back = true;
                        }
                    });
                    if let Some(id) = self.selected_layer {
                        if move_front {
                            self.move_layer_toward_front(id);
                        } else if move_back {
                            self.move_layer_toward_back(id);
                        }
                    }

                    ui.add_space(6.0);
                    ui.heading("ソース / オーバーレイ");
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("＋ 画像").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("画像 / SVG", &["png", "jpg", "jpeg", "webp", "bmp", "svg"])
                                .pick_file()
                            {
                                self.add_image_from_path(&path);
                            }
                        }
                        ui.small(&self.image_overlay_message);
                    });

                    egui::CollapsingHeader::new("画像ライブラリ")
                        .id_salt("image_library")
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                if ui.button("更新").clicked() {
                                    self.refresh_image_library();
                                }
                                #[cfg(target_os = "windows")]
                                if ui.button("フォルダを開く").clicked() {
                                    let _ = std::process::Command::new("explorer")
                                        .arg(&self.image_library_dir)
                                        .spawn();
                                }
                            });
                            ui.small(&self.image_library_message);
                            ui.small("image/ に入れた画像・SVGを自動認識します。background / overlay / wipe / thumbnail の各フォルダも使用できます。");

                            let mut add_from_library: Option<PathBuf> = None;
                            egui::ScrollArea::vertical()
                                .id_salt("image_library_files")
                                .max_height(150.0)
                                .show(ui, |ui| {
                                    if self.image_library_files.is_empty() {
                                        ui.small("まだ素材がありません。image/ に画像またはSVGを入れて「更新」を押してください。");
                                    } else {
                                        for path in &self.image_library_files {
                                            let relative = path
                                                .strip_prefix(&self.image_library_dir)
                                                .unwrap_or(path.as_path());
                                            ui.horizontal(|ui| {
                                                if ui.small_button("追加").clicked() {
                                                    add_from_library = Some(path.clone());
                                                }
                                                ui.label(relative.display().to_string());
                                            });
                                        }
                                    }
                                });
                            if let Some(path) = add_from_library {
                                self.add_image_from_path(&path);
                            }
                        });

                    ui.add_space(6.0);
                    egui::CollapsingHeader::new("BGMライブラリ")
                        .id_salt("bgm_library")
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                if ui.button("更新").clicked() {
                                    self.refresh_bgm_library();
                                }
                                if ui.button("＋ BGM").clicked() {
                                    if let Some(path) = rfd::FileDialog::new()
                                        .add_filter("BGM", &["mp3", "wav", "ogg", "flac"])
                                        .pick_file()
                                    {
                                        self.add_bgm_from_path(&path);
                                    }
                                }
                                #[cfg(target_os = "windows")]
                                if ui.button("フォルダを開く").clicked() {
                                    let _ = std::process::Command::new("explorer")
                                        .arg(&self.bgm_library_dir)
                                        .spawn();
                                }
                            });
                            ui.small(&self.bgm_library_message);
                            ui.small("bgm/ に MP3 / WAV / OGG / FLAC を入れて「更新」。追加すると音声レイヤーになります。");

                            let mut add_bgm_from_library: Option<PathBuf> = None;
                            egui::ScrollArea::vertical()
                                .id_salt("bgm_library_files")
                                .max_height(130.0)
                                .show(ui, |ui| {
                                    if self.bgm_library_files.is_empty() {
                                        ui.small("まだBGMがありません。bgm/ に音源を入れて「更新」を押してください。");
                                    } else {
                                        for path in &self.bgm_library_files {
                                            let relative = path
                                                .strip_prefix(&self.bgm_library_dir)
                                                .unwrap_or(path.as_path());
                                            ui.horizontal(|ui| {
                                                if ui.small_button("追加").clicked() {
                                                    add_bgm_from_library = Some(path.clone());
                                                }
                                                ui.label(relative.display().to_string());
                                            });
                                        }
                                    }
                                });
                            if let Some(path) = add_bgm_from_library {
                                self.add_bgm_from_path(&path);
                            }
                        });

                    let selected_bgm_id = self.selected_layer.and_then(|selected_id| {
                        self.layers
                            .iter()
                            .find(|layer| layer.id == selected_id && layer.kind == LayerKind::Audio)
                            .map(|layer| layer.id)
                    });
                    let active_bgm_id = self
                        .bgm_layers
                        .iter()
                        .find(|(id, _)| self.bgm_players.contains_key(id))
                        .map(|(id, _)| *id);
                    let bgm_control_layer_id =
                        prefer_selected_or_active_bgm(selected_bgm_id, active_bgm_id);
                    let mut bgm_play = false;
                    let mut bgm_pause_resume = false;
                    let mut bgm_stop = false;
                    let mut bgm_remove = false;
                    let mut bgm_restart_for_loop = false;
                    let mut bgm_volume_change: Option<f32> = None;
                    let mut bgm_mute_change: Option<bool> = None;
                    if let Some(bgm_layer_id) = bgm_control_layer_id {
                        if let Some(bgm_index) = self
                            .bgm_layers
                            .iter()
                            .position(|(id, _)| *id == bgm_layer_id)
                        {
                            let is_active = self.bgm_players.contains_key(&bgm_layer_id);
                            let is_paused = self
                                .bgm_players
                                .get(&bgm_layer_id)
                                .map(|player| player.is_paused())
                                .unwrap_or(false);
                            egui::CollapsingHeader::new("選択BGM")
                                .id_salt(("bgm_controls", bgm_layer_id))
                                .default_open(true)
                                .show(ui, |ui| {
                                    let source = &mut self.bgm_layers[bgm_index].1;
                                    ui.horizontal_wrapped(|ui| {
                                        if ui.checkbox(&mut source.enabled, "有効").changed() && !source.enabled {
                                            bgm_stop = true;
                                        }
                                        ui.label(&source.name);
                                        if ui.button("削除").clicked() {
                                            bgm_remove = true;
                                        }
                                    });
                                    ui.small(source.path.display().to_string());
                                    ui.horizontal_wrapped(|ui| {
                                        if ui.button("▶ 再生").clicked() {
                                            bgm_play = true;
                                        }
                                        if ui
                                            .add_enabled(
                                                is_active,
                                                egui::Button::new(if is_paused { "▶ 再開" } else { "⏸ 一時停止" }),
                                            )
                                            .clicked()
                                        {
                                            bgm_pause_resume = true;
                                        }
                                        if ui.add_enabled(is_active, egui::Button::new("■ 停止")).clicked() {
                                            bgm_stop = true;
                                        }
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("音量");
                                        if ui
                                            .add(egui::Slider::new(&mut source.volume_percent, 0.0..=100.0).suffix("%"))
                                            .changed()
                                        {
                                            bgm_volume_change = Some(source.effective_volume());
                                        }
                                        if ui.checkbox(&mut source.muted, "Mute").changed() {
                                            bgm_mute_change = Some(source.muted);
                                            bgm_volume_change = Some(source.effective_volume());
                                        }
                                    });
                                    if ui.checkbox(&mut source.loop_enabled, "ループ再生").changed() && is_active {
                                        bgm_restart_for_loop = true;
                                    }
                                });
                        }
                    }
                    if let Some(bgm_layer_id) = bgm_control_layer_id {
                        if let Some(volume) = bgm_volume_change {
                            if let Some(player) = self.bgm_players.get(&bgm_layer_id) {
                                player.set_volume(volume);
                            }
                            if let Some(channel_id) = self.bgm_audio_channels.get(&bgm_layer_id).copied() {
                                if let Some((_, source)) = self
                                    .bgm_layers
                                    .iter()
                                    .find(|(id, _)| *id == bgm_layer_id)
                                {
                                    if let Some(audio) = self.audio.as_ref() {
                                        audio.set_channel_gain(channel_id, source.volume_linear());
                                    }
                                }
                            }
                        }
                        if let Some(muted) = bgm_mute_change {
                            if let Some(channel_id) = self.bgm_audio_channels.get(&bgm_layer_id).copied() {
                                if let Some(audio) = self.audio.as_ref() {
                                    audio.set_channel_muted(channel_id, muted);
                                }
                            }
                            self.bgm_message = if muted {
                                "BGM: Mute ON".to_owned()
                            } else {
                                "BGM: Mute OFF".to_owned()
                            };
                        }
                        if bgm_remove {
                            self.remove_bgm_layer(bgm_layer_id);
                            self.bgm_message = if self.bgm_layers.is_empty() {
                                "BGM: 未追加".to_owned()
                            } else {
                                format!("BGM削除 / 残り {} 曲", self.bgm_layers.len())
                            };
                        } else {
                            if bgm_stop {
                                if let Some(player) = self.bgm_players.remove(&bgm_layer_id) {
                                    player.stop();
                                }
                                self.bgm_message = "BGM: 停止".to_owned();
                            }
                            if bgm_pause_resume {
                                if let Some(player) = self.bgm_players.get(&bgm_layer_id) {
                                    if player.is_paused() {
                                        player.resume();
                                        self.bgm_message = "BGM: 再開".to_owned();
                                    } else {
                                        player.pause();
                                        self.bgm_message = "BGM: 一時停止".to_owned();
                                    }
                                }
                            }
                            if bgm_play || bgm_restart_for_loop {
                                self.play_bgm_layer(bgm_layer_id);
                            }
                        }
                    }
                    ui.small(&self.bgm_message);

                    let selected_image_id = self.selected_layer.and_then(|selected_id| {
                        self.layers
                            .iter()
                            .find(|layer| layer.id == selected_id && layer.kind == LayerKind::Image)
                            .map(|layer| layer.id)
                    });
                    let mut remove_selected_image = false;
                    if let Some(image_layer_id) = selected_image_id {
                        if let Some(image_index) = self
                            .image_overlays
                            .iter()
                            .position(|(id, _)| *id == image_layer_id)
                        {
                            egui::CollapsingHeader::new("選択画像")
                                .id_salt(("image_overlay_controls", image_layer_id))
                                .default_open(true)
                                .show(ui, |ui| {
                                    let image_overlay = &mut self.image_overlays[image_index].1;
                                    ui.horizontal(|ui| {
                                        ui.checkbox(&mut image_overlay.enabled, "表示");
                                        ui.label(&image_overlay.name);
                                        if ui.button("削除").clicked() {
                                            remove_selected_image = true;
                                        }
                                    });
                                    ui.small(format!(
                                        "元サイズ {}×{}",
                                        image_overlay.pixel_width, image_overlay.pixel_height
                                    ));
                                    ui.horizontal(|ui| {
                                        ui.label("X");
                                        let changed = ui
                                            .add(egui::Slider::new(&mut image_overlay.x_percent, 0.0..=100.0).suffix("%"))
                                            .changed();
                                        if changed {
                                            image_overlay.clamp_to_frame(self.preview_size[0], self.preview_size[1]);
                                        }
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("Y");
                                        let changed = ui
                                            .add(egui::Slider::new(&mut image_overlay.y_percent, 0.0..=100.0).suffix("%"))
                                            .changed();
                                        if changed {
                                            image_overlay.clamp_to_frame(self.preview_size[0], self.preview_size[1]);
                                        }
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("サイズ");
                                        let changed = ui
                                            .add(egui::Slider::new(&mut image_overlay.width_percent, 5.0..=80.0).suffix("%"))
                                            .changed();
                                        if changed {
                                            image_overlay.clamp_to_frame(self.preview_size[0], self.preview_size[1]);
                                        }
                                    });
                                });
                        }
                    } else if !self.image_overlays.is_empty() {
                        ui.small(format!(
                            "画像レイヤー: {} 枚（レイヤー一覧から選択すると編集できます）",
                            self.image_overlays.len()
                        ));
                    }

                    if let Some(image_layer_id) = selected_image_id {
                        if remove_selected_image {
                            self.remove_image_layer(image_layer_id);
                            self.preview_resize_drag = false;
                            self.image_overlay_message = if self.image_overlays.is_empty() {
                                "画像オーバーレイ: 未追加".to_owned()
                            } else {
                                format!("画像削除 / 残り {} 枚", self.image_overlays.len())
                            };
                        }
                    }

                    egui::CollapsingHeader::new("ししゃも（テスト）")
                        .id_salt("fish_overlay_controls")
                        .default_open(false)
                        .show(ui, |ui| {
                            ui.checkbox(&mut self.overlay_source.enabled, "表示");
                            ui.horizontal(|ui| {
                                ui.label("X");
                                ui.add(egui::Slider::new(&mut self.overlay_source.x_percent, 0.0..=100.0).suffix("%"));
                            });
                            ui.horizontal(|ui| {
                                ui.label("Y");
                                ui.add(egui::Slider::new(&mut self.overlay_source.y_percent, 0.0..=100.0).suffix("%"));
                            });
                            ui.horizontal(|ui| {
                                ui.label("サイズ");
                                ui.add(egui::Slider::new(&mut self.overlay_source.width_percent, 5.0..=45.0).suffix("%"));
                            });
                            ui.checkbox(&mut self.overlay_source.bounce, "ぴょんぴょん動作");
                        });
                    ui.small("位置とサイズはプレビュー別窓でも直接編集できます");
                            });
                    },
                );

            });
        });

        if self.vm.is_live {
            self.elapsed_ms = self.elapsed_ms.saturating_add(16);
        }

        // Main controls, deferred preview, and deferred mixer no longer share the same repaint clock.
        // Sync controls changed during this pass, then let child viewports continue
        // at their own repaint cadences.
        self.sync_preview_snapshot();
        let repaint_ms = main_ui_repaint_ms(
            self.streaming.is_some(),
            self.preview_window_open,
            self.recording.is_some(),
            self.recording_preview_mode,
        );
        ctx.request_repaint_after(Duration::from_millis(repaint_ms));
    }
}

#[cfg(test)]
mod toolbar_tests {
    use super::{
        main_ui_repaint_ms, measured_fps, mixer_meter_refresh_due, mixer_ui_repaint_ms,
        performance_pipeline_text, prefer_selected_or_active_bgm, toggle_window_open,
        RuntimeFrameCounters,
    };
    use std::time::{Duration, Instant};
    use stream_capture::PreviewMode;

    #[test]
    fn toolbar_toggle_flips_auxiliary_window_state_both_ways() {
        let mut open = false;
        toggle_window_open(&mut open);
        assert!(open);
        toggle_window_open(&mut open);
        assert!(!open);
    }
    #[test]
    fn deferred_preview_does_not_force_main_ui_to_30fps() {
        assert_eq!(
            main_ui_repaint_ms(false, false, false, PreviewMode::Fps15),
            100
        );
        assert_eq!(
            main_ui_repaint_ms(false, true, false, PreviewMode::Fps15),
            100
        );
    }

    #[test]
    fn deferred_mixer_uses_independent_meter_repaint_period() {
        assert_eq!(mixer_ui_repaint_ms(true), 33);
        assert_eq!(mixer_ui_repaint_ms(false), 250);
    }

    #[test]
    fn mixer_meter_refresh_is_capped_at_about_30hz() {
        let start = Instant::now();
        assert!(mixer_meter_refresh_due(None, start));
        assert!(!mixer_meter_refresh_due(Some(start), start + Duration::from_millis(20)));
        assert!(mixer_meter_refresh_due(Some(start), start + Duration::from_millis(34)));
    }

    #[test]
    fn performance_labels_distinguish_encode_input_from_target_fps() {
        let text = performance_pipeline_text(24.2, 0.0, 58.1, 29.7, 23.6, 60, 0);
        assert!(text.contains("Capture 24.2 FPS"));
        assert!(text.contains("Mixer Render 58.1 FPS"));
        assert!(text.contains("Meter 29.7 FPS"));
        assert!(text.contains("Encode In 23.6 FPS"));
        assert!(text.contains("Target 60 FPS"));
    }

    #[test]
    fn active_bgm_controls_remain_available_when_layer_selection_changes() {
        assert_eq!(prefer_selected_or_active_bgm(Some(10_u64), Some(20_u64)), Some(10));
        assert_eq!(prefer_selected_or_active_bgm(None, Some(20_u64)), Some(20));
        assert_eq!(prefer_selected_or_active_bgm::<u64>(None, None), None);
    }

    #[test]
    fn measured_fps_uses_actual_elapsed_time() {
        let fps = measured_fps(45, 15, Duration::from_millis(1500));
        assert!((fps - 20.0).abs() < 0.001);
    }

    #[test]
    fn runtime_frame_counters_track_each_pipeline_independently() {
        let counters = RuntimeFrameCounters::default();
        counters.count_preview_ui_frame();
        counters.count_preview_ui_frame();
        counters.count_mixer_ui_frame();
        counters.count_mixer_meter_update();
        counters.count_streaming_output_frame();
        counters.count_streaming_output_frame();
        counters.count_streaming_output_frame();

        let snapshot = counters.snapshot();
        assert_eq!(snapshot.preview_ui_frames, 2);
        assert_eq!(snapshot.mixer_ui_frames, 1);
        assert_eq!(snapshot.mixer_meter_updates, 1);
        assert_eq!(snapshot.streaming_output_frames, 3);
    }

    #[test]
    fn audio_worker_can_be_shared_with_deferred_mixer() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<stream_audio::AudioWorker>();
    }

}
