use crate::addon::{AddonOrigin, AddonRegistry, ADDON_API_VERSION};
use crate::bgm::{BgmLayerSource, BgmPlayer};
use crate::scene::{ImageOverlaySource, LayerId, LayerKind, OverlaySource, SceneLayer};
use crate::settings::{load_settings, save_settings, AppSettings};
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
    sync::mpsc,
    time::{Duration, Instant},
};
use stream_audio::{
    application_source_label, dbfs, selection_label, AudioChannelKind, AudioDeviceSelection,
    AudioWorker, ChannelMixerControl, MixerControl,
};
use stream_capture::{enumerate_windows, CaptureSource, CaptureWorker, PreviewMode, WindowInfo};
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

#[derive(Debug)]
struct PerformanceMonitor {
    system: System,
    pid: Option<sysinfo::Pid>,
    last_refresh: Instant,
    cpu_percent: f32,
    memory_mb: f64,
}

impl PerformanceMonitor {
    fn new() -> Self {
        let mut monitor = Self {
            system: System::new(),
            pid: sysinfo::get_current_pid().ok(),
            last_refresh: Instant::now() - Duration::from_secs(2),
            cpu_percent: 0.0,
            memory_mb: 0.0,
        };
        monitor.refresh_if_due();
        monitor
    }

    fn refresh_if_due(&mut self) {
        if self.last_refresh.elapsed() < Duration::from_secs(1) {
            return;
        }

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

pub struct YaoyorozuApp {
    vm: StreamViewModel,
    elapsed_ms: u64,
    stream_title: String,

    windows: Vec<WindowInfo>,
    selected_source: CaptureSource,
    capture: Option<CaptureWorker>,
    capture_target_fps: u32,

    preview_texture: Option<egui::TextureHandle>,
    preview_size: [u32; 2],
    preview_window_open: bool,

    capture_message: String,
    font_message: String,
    audio: Option<AudioWorker>,
    audio_message: String,
    selected_application_audio_pid: Option<u32>,
    application_audio_message: String,
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
    bgm_library_dir: PathBuf,
    bgm_library_files: Vec<PathBuf>,
    bgm_library_message: String,
    bgm_message: String,
    app_started_at: std::time::Instant,
    streaming: Option<StreamingSession>,
    streaming_message: String,
    performance: PerformanceMonitor,
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
        self.bgm_layers.push((id, source));
        id
    }

    fn remove_bgm_layer(&mut self, id: LayerId) {
        if let Some(player) = self.bgm_players.remove(&id) {
            player.stop();
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
        match BgmPlayer::play_file(&path, loop_enabled, volume) {
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
                (Some(worker), "WASAPI音声メーター: 稼働中".to_owned())
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

        Self {
            vm,
            elapsed_ms: 0,
            stream_title: "燕 / Tsubame".to_owned(),
            windows,
            selected_source,
            capture,
            capture_target_fps,
            preview_texture: None,
            preview_size: [0, 0],
            preview_window_open: persisted_settings.preview_window_open,
            capture_message,
            font_message,
            audio,
            audio_message,
            selected_application_audio_pid: None,
            application_audio_message: "アプリ音声: 一覧から選択してください".to_owned(),
            recording: None,
            finalize_rx: None,
            recording_message: "録画: 待機中".to_owned(),
            ffmpeg_location: ffmpeg_location_string(),
            encoder_preference,
            recording_preview_mode,
            capture_preview_mode: PreviewMode::Fps30,
            streaming_target,
            show_stream_key: false,
            overlay_source: OverlaySource::default(),
            layers,
            selected_layer: None,
            next_layer_id: fish_layer_id + 1,
            fish_layer_id,
            preview_resize_drag: false,
            image_overlays: Vec::new(),
            image_overlay_message: "画像オーバーレイ: 未追加".to_owned(),
            image_library_dir,
            image_library_files,
            image_library_message,
            bgm_layers: Vec::new(),
            bgm_players: HashMap::new(),
            bgm_library_dir,
            bgm_library_files,
            bgm_library_message,
            bgm_message: "BGM: 未追加".to_owned(),
            app_started_at: std::time::Instant::now(),
            streaming: None,
            streaming_message: "配信: 待機中".to_owned(),
            performance: PerformanceMonitor::new(),
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
        self.preview_texture = None;
        self.preview_size = [0, 0];

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

    fn update_preview_texture(&mut self, ctx: &egui::Context) {
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

        let needs_stream_frame = self.streaming.is_some();
        let needs_ui_texture = self.preview_window_open;
        if !needs_stream_frame && !needs_ui_texture {
            // Hidden preview + no live stream: dimensions are all we need.
            // Avoid RGBA clones, CPU overlay composition and egui texture uploads.
            return;
        }

        let overlay_enabled = self.layers.iter().any(|layer| match layer.kind {
            LayerKind::FishOverlay if layer.id == self.fish_layer_id => self.overlay_source.enabled,
            LayerKind::Image => self
                .image_overlays
                .iter()
                .find(|(id, _)| *id == layer.id)
                .is_some_and(|(_, overlay)| overlay.enabled),
            _ => false,
        });

        // Keep the original Arc-backed capture frame whenever no overlay is
        // active. This removes one full 960x540 RGBA allocation/copy per frame.
        //
        // `self.layers` is stored back-to-front. Composing in vector order
        // therefore makes the last item the visible front layer, matching the
        // layer list UI (which displays the vector in reverse).
        let composed_frame = if overlay_enabled {
            let mut composed_rgba = frame.rgba.to_vec();
            for layer in &self.layers {
                match layer.kind {
                    LayerKind::FishOverlay if layer.id == self.fish_layer_id => {
                        self.overlay_source.compose_test_overlay(
                            &mut composed_rgba,
                            frame.width,
                            frame.height,
                            self.app_started_at.elapsed().as_secs_f32(),
                        );
                    }
                    LayerKind::Image => {
                        if let Some((_, image_overlay)) = self
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
                rgba: std::sync::Arc::<[u8]>::from(composed_rgba),
            }
        } else {
            frame
        };

        if let Some(streaming) = &self.streaming {
            streaming.push_video_frame(&composed_frame);
        }

        if !needs_ui_texture {
            // Streaming can keep using the composited frame while the preview
            // window is closed, without paying for ColorImage/texture upload.
            return;
        }

        let image = egui::ColorImage::from_rgba_unmultiplied(
            [
                composed_frame.width as usize,
                composed_frame.height as usize,
            ],
            &composed_frame.rgba,
        );

        if let Some(texture) = &mut self.preview_texture {
            texture.set(image, egui::TextureOptions::LINEAR);
        } else {
            self.preview_texture =
                Some(ctx.load_texture("capture-preview", image, egui::TextureOptions::LINEAR));
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

        let channel_mixer = self
            .audio
            .as_ref()
            .map(|audio| audio.channel_mixer())
            .unwrap_or_default();

        match RecordingSession::start_with_audio_routing(
            "recordings",
            config,
            gpu,
            mixer,
            channel_mixer,
            audio_devices,
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

    fn draw_preview_canvas(
        &mut self,
        ui: &mut egui::Ui,
        outer_size: egui::Vec2,
        compact_info: bool,
    ) {
        let desired = egui::vec2(outer_size.x.max(120.0), outer_size.y.max(120.0));
        let (outer, outer_response) = ui.allocate_exact_size(desired, egui::Sense::click());
        ui.painter()
            .rect_filled(outer, 4.0, egui::Color32::from_rgb(18, 20, 24));

        let mut overlay_interacted = false;
        if let Some(texture) = &self.preview_texture {
            let (draw_w, draw_h) = fit_aspect(
                self.preview_size[0] as f32,
                self.preview_size[1] as f32,
                outer.width(),
                outer.height(),
            );
            let image_rect =
                egui::Rect::from_center_size(outer.center(), egui::vec2(draw_w, draw_h));
            ui.painter().image(
                texture.id(),
                image_rect,
                egui::Rect::from_min_max(egui::Pos2::new(0.0, 0.0), egui::Pos2::new(1.0, 1.0)),
                egui::Color32::WHITE,
            );

            if self.overlay_source.enabled && self.preview_size[0] > 0 && self.preview_size[1] > 0 {
                let fish_locked = self.layer_locked(self.fish_layer_id);
                let fish_w = image_rect.width() * self.overlay_source.width_percent / 100.0;
                let fish_h = fish_w * 0.42;
                let bounce = if self.overlay_source.bounce {
                    (self.app_started_at.elapsed().as_secs_f32() * 4.2)
                        .sin()
                        .abs()
                        * image_rect.height()
                        * 0.055
                } else {
                    0.0
                };
                let center = egui::pos2(
                    image_rect.left() + image_rect.width() * self.overlay_source.x_percent / 100.0,
                    image_rect.top() + image_rect.height() * self.overlay_source.y_percent / 100.0
                        - bounce,
                );
                let fish_rect = egui::Rect::from_center_size(center, egui::vec2(fish_w, fish_h));
                let hit_rect = fish_rect.expand(5.0).intersect(image_rect);
                let fish_response = ui.interact(
                    hit_rect,
                    ui.id().with("overlay_fish_drag"),
                    egui::Sense::click_and_drag(),
                );

                let handle_size = 12.0;
                let handle_rect = egui::Rect::from_center_size(
                    fish_rect.right_bottom(),
                    egui::vec2(handle_size, handle_size),
                );

                if fish_response.clicked() {
                    self.selected_layer = Some(self.fish_layer_id);
                    overlay_interacted = true;
                }
                if fish_response.drag_started() {
                    self.selected_layer = Some(self.fish_layer_id);
                    overlay_interacted = true;
                    self.preview_resize_drag = !fish_locked
                        && ui
                            .ctx()
                            .pointer_interact_pos()
                            .map(|p| handle_rect.expand(5.0).contains(p))
                            .unwrap_or(false);
                }
                if fish_response.dragged() {
                    overlay_interacted = true;
                    // egui::Response::drag_delta() is cumulative from drag start.
                    // Applying that value every repaint makes transform editing unstable.
                    // Use the pointer's per-frame delta while this layer owns the drag.
                    let delta = ui.ctx().input(|i| i.pointer.delta());
                    if fish_locked {
                        // Locked layers can still be selected, but transforms are ignored.
                    } else if self.preview_resize_drag {
                        self.overlay_source.resize_by_preview_delta(
                            delta.x,
                            image_rect.width(),
                            self.preview_size[0],
                            self.preview_size[1],
                        );
                    } else {
                        self.overlay_source.move_by_preview_delta(
                            delta.x,
                            delta.y,
                            image_rect.width(),
                            image_rect.height(),
                            self.preview_size[0],
                            self.preview_size[1],
                        );
                    }
                }
                if fish_response.drag_stopped() {
                    self.preview_resize_drag = false;
                }

                if self.selected_layer == Some(self.fish_layer_id) {
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

            let image_layer_ids: Vec<LayerId> = self
                .layers
                .iter()
                .rev()
                .filter(|layer| layer.kind == LayerKind::Image)
                .map(|layer| layer.id)
                .collect();

            for image_layer_id in image_layer_ids {
                let image_locked = self.layer_locked(image_layer_id);
                let Some(image_index) = self
                    .image_overlays
                    .iter()
                    .position(|(id, _)| *id == image_layer_id)
                else {
                    continue;
                };
                let image_overlay = &mut self.image_overlays[image_index].1;
                if !image_overlay.enabled || self.preview_size[0] == 0 || self.preview_size[1] == 0 {
                    continue;
                }

                let overlay_w = image_rect.width() * image_overlay.width_percent / 100.0;
                let overlay_h = overlay_w * image_overlay.aspect();
                let center = egui::pos2(
                    image_rect.left() + image_rect.width() * image_overlay.x_percent / 100.0,
                    image_rect.top() + image_rect.height() * image_overlay.y_percent / 100.0,
                );
                let overlay_rect =
                    egui::Rect::from_center_size(center, egui::vec2(overlay_w, overlay_h));
                let hit_rect = overlay_rect.expand(5.0).intersect(image_rect);
                let image_response = ui.interact(
                    hit_rect,
                    ui.id().with(("overlay_image_drag", image_layer_id)),
                    egui::Sense::click_and_drag(),
                );
                let handle_size = 12.0;
                let handle_rect = egui::Rect::from_center_size(
                    overlay_rect.right_bottom(),
                    egui::vec2(handle_size, handle_size),
                );

                if image_response.clicked() {
                    self.selected_layer = Some(image_layer_id);
                    overlay_interacted = true;
                }
                if image_response.drag_started() {
                    self.selected_layer = Some(image_layer_id);
                    overlay_interacted = true;
                    self.preview_resize_drag = !image_locked
                        && ui
                            .ctx()
                            .pointer_interact_pos()
                            .map(|p| handle_rect.expand(5.0).contains(p))
                            .unwrap_or(false);
                }
                if image_response.dragged() {
                    overlay_interacted = true;
                    let delta = ui.ctx().input(|i| i.pointer.delta());
                    if image_locked {
                        // Locked layers can still be selected, but transforms are ignored.
                    } else if self.preview_resize_drag {
                        image_overlay.resize_by_preview_delta(
                            delta.x,
                            image_rect.width(),
                            self.preview_size[0],
                            self.preview_size[1],
                        );
                    } else {
                        image_overlay.move_by_preview_delta(
                            delta.x,
                            delta.y,
                            image_rect.width(),
                            image_rect.height(),
                            self.preview_size[0],
                            self.preview_size[1],
                        );
                    }
                }
                if image_response.drag_stopped() {
                    self.preview_resize_drag = false;
                }

                if self.selected_layer == Some(image_layer_id) {
                    let stroke =
                        egui::Stroke::new(1.5_f32, egui::Color32::from_rgb(130, 220, 140));
                    ui.painter().rect_stroke(
                        overlay_rect,
                        2.0,
                        stroke,
                        egui::StrokeKind::Outside,
                    );
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
                        self.selected_layer = None;
                        self.preview_resize_drag = false;
                    }
                }
            }
        } else {
            ui.painter().text(
                outer.center(),
                egui::Align2::CENTER_CENTER,
                "キャプチャ待機中…",
                egui::FontId::proportional(16.0),
                egui::Color32::LIGHT_GRAY,
            );
        }

        if compact_info {
            ui.small(format!(
                "入力 {}×{} / プレビュー窓でドラッグ編集",
                self.preview_size[0], self.preview_size[1]
            ));
        } else {
            ui.small(format!(
                "プレビュー別窓 / 入力 {}×{} / 画像・ししゃものドラッグ編集対応",
                self.preview_size[0], self.preview_size[1]
            ));
        }
    }

    fn show_preview_viewport(&mut self, ctx: &egui::Context) {
        if !self.preview_window_open {
            return;
        }

        let [default_w, default_h] = normal_preview_size();
        let viewport_id = egui::ViewportId::from_hash_of("yaoyorozu_preview_window");
        let builder = egui::ViewportBuilder::default()
            .with_title("燕 / Tsubame Preview")
            .with_inner_size([default_w + 40.0, default_h + 80.0])
            .with_min_inner_size([360.0, 240.0]);
        let mut close_requested = false;

        ctx.show_viewport_immediate(viewport_id, builder, |preview_ctx, _class| {
            if preview_ctx.input(|i| i.viewport().close_requested()) {
                close_requested = true;
            }

            egui::CentralPanel::default().show(preview_ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("プレビュー");
                    ui.separator();
                    ui.small("閉じても配信・録画は続行");
                    if ui.button("閉じる").clicked() {
                        close_requested = true;
                    }
                });
                ui.separator();
                let available = ui.available_size();
                self.draw_preview_canvas(ui, available, false);
            });
        });

        if close_requested {
            self.preview_window_open = false;
        }
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

impl eframe::App for YaoyorozuApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.bgm_players.retain(|_, player| !player.is_finished());
        self.poll_recording_finalize();
        self.poll_streaming();
        self.performance.refresh_if_due();
        self.sync_capture_preview_mode();
        self.update_preview_texture(ctx);
        self.show_preview_viewport(ctx);
        self.show_settings_window(ctx);
        self.show_first_run_window(ctx);
        self.save_settings_if_due();

        egui::TopBottomPanel::top("tsubame_top_toolbar")
            .exact_height(38.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("⚙ 設定").clicked() {
                        self.settings_open = true;
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
                    "CPU {:.1}% | RAM {:.0} MB | Capture {:.1} FPS | Preview Drop {}",
                    self.performance.cpu_percent,
                    self.performance.memory_mb,
                    capture_fps,
                    preview_drops
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
            let levels = self.audio.as_ref().map(|a| a.levels()).unwrap_or_default();
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
                        let button_label = if self.preview_window_open { "閉じる" } else { "開く" };
                        if ui.button(button_label).clicked() {
                            self.preview_window_open = !self.preview_window_open;
                        }
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
                    let mut bgm_play = false;
                    let mut bgm_pause_resume = false;
                    let mut bgm_stop = false;
                    let mut bgm_remove = false;
                    let mut bgm_restart_for_loop = false;
                    let mut bgm_volume_change: Option<f32> = None;
                    let mut bgm_mute_change: Option<bool> = None;
                    if let Some(bgm_layer_id) = selected_bgm_id {
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
                    if let Some(bgm_layer_id) = selected_bgm_id {
                        if let Some(volume) = bgm_volume_change {
                            if let Some(player) = self.bgm_players.get(&bgm_layer_id) {
                                player.set_volume(volume);
                            }
                        }
                        if let Some(muted) = bgm_mute_change {
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

                ui.separator();

                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), content_height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("audio_mixer_scroll")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                    ui.heading("音声ミキサー");

                    // Phase 8A: BGMはまだWindows既定出力経由だが、
                    // ミキサーからGain/Muteを直接操作できるようにする。
                    // Mix/WAVの直接ルーティングはPhase 8Bでstream-audioへ統合する。
                    let bgm_mixer_snapshot: Vec<_> = self
                        .bgm_layers
                        .iter()
                        .map(|(id, source)| {
                            (
                                *id,
                                source.name.clone(),
                                source.volume_percent,
                                source.muted,
                            )
                        })
                        .collect();
                    let mut bgm_mixer_gain_changes: Vec<(LayerId, f32)> = Vec::new();
                    let mut bgm_mixer_mute_changes: Vec<(LayerId, bool)> = Vec::new();
                    let mut bgm_mixer_remove: Option<LayerId> = None;

                    if let Some(audio) = &self.audio {
                        let settings = audio.mixer_settings();
                        let output_devices = audio.output_devices();
                        let input_devices = audio.input_devices();
                        let device_switch_enabled = self.recording.is_none()
                            && self.finalize_rx.is_none()
                            && self.streaming.is_none();
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
                            .into_iter()
                            .filter(|channel| channel.kind == AudioChannelKind::Application)
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
                                            level: levels.desktop,
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
                                            level: levels.mic,
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

                                    for (bgm_id, bgm_name, volume_percent, muted) in &bgm_mixer_snapshot {
                                        let bgm_strip = draw_mixer_channel_strip(
                                            ui,
                                            MixerStripModel {
                                                title: "BGM".to_owned(),
                                                source: bgm_name.clone(),
                                                // Phase 8AではPCMメーターはまだstream-audio未統合。
                                                // ストリップ構造を先に統一し、実レベルは8Bで接続する。
                                                level: 0.0,
                                                gain_percent: *volume_percent,
                                                mute: Some(*muted),
                                                mix: None,
                                                wav: None,
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
                                                level: channel.current_level,
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
                                            level: levels.mix,
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
                                let app_sources = audio.application_sources();
                                let selected_app_label = self
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
                                                    self.selected_application_audio_pid == Some(source.capture_process_id),
                                                    &label,
                                                ).on_hover_text(&label).clicked() {
                                                    self.selected_application_audio_pid = Some(source.capture_process_id);
                                                }
                                            }
                                        });

                                    let selected_source = self
                                        .selected_application_audio_pid
                                        .and_then(|pid| app_sources.iter().find(|source| source.capture_process_id == pid).cloned());
                                    if ui.add_enabled(
                                        selected_source.is_some() && device_switch_enabled,
                                        egui::Button::new("＋ アプリ音声"),
                                    ).clicked() {
                                        if let Some(source) = selected_source {
                                            match audio.add_application_channel(source) {
                                                Ok(_) => {
                                                    self.application_audio_message =
                                                        "アプリ音声チャンネルを追加しました".to_owned();
                                                }
                                                Err(err) => {
                                                    self.application_audio_message =
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
                                                self.application_audio_message =
                                                    "アプリ音声一覧を更新しました".to_owned();
                                            }
                                            Err(err) => {
                                                self.application_audio_message =
                                                    format!("アプリ音声一覧更新失敗: {err}");
                                            }
                                        }
                                    }
                                    if ui.add_enabled(device_switch_enabled, egui::Button::new("音声デバイス更新")).clicked() {
                                        match audio.refresh_devices() {
                                            Ok(()) => {
                                                self.audio_message = "音声デバイス一覧を更新しました".to_owned();
                                            }
                                            Err(err) => {
                                                self.audio_message = format!("デバイス更新失敗: {err}");
                                            }
                                        }
                                    }
                                });
                                ui.small(&self.application_audio_message);
                                ui.small(&self.audio_message);
                                if !device_switch_enabled {
                                    ui.small(if self.streaming.is_some() {
                                        "配信中は音声デバイスとアプリ構成を固定します"
                                    } else {
                                        "録画中は音声デバイスとアプリ構成を固定します"
                                    });
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

                    // BGM操作はaudioへの借用を解放してから反映する。
                    for (id, gain_percent) in bgm_mixer_gain_changes {
                        if let Some((_, source)) = self.bgm_layers.iter_mut().find(|(layer_id, _)| *layer_id == id) {
                            source.volume_percent = gain_percent.clamp(0.0, 100.0);
                            if let Some(player) = self.bgm_players.get(&id) {
                                player.set_volume(source.effective_volume());
                            }
                        }
                    }
                    for (id, muted) in bgm_mixer_mute_changes {
                        if let Some((_, source)) = self.bgm_layers.iter_mut().find(|(layer_id, _)| *layer_id == id) {
                            source.muted = muted;
                            if let Some(player) = self.bgm_players.get(&id) {
                                player.set_volume(source.effective_volume());
                            }
                        }
                    }
                    if let Some(id) = bgm_mixer_remove {
                        self.remove_bgm_layer(id);
                        self.bgm_message = if self.bgm_layers.is_empty() {
                            "BGM: 未追加".to_owned()
                        } else {
                            format!("BGM削除 / 残り {} 曲", self.bgm_layers.len())
                        };
                    }

                    ui.small("BGMのMix/WAVと実レベルメーターはPhase 8Bで直接PCM統合します");
                    ui.small(&self.audio_message);
                            });
                    },
                );
            });
        });

        if self.vm.is_live {
            self.elapsed_ms = self.elapsed_ms.saturating_add(16);
        }

        let repaint_ms = if self.streaming.is_some() {
            // Live input readback currently runs at 30 FPS, so repainting the
            // egui control surface at 60 FPS only burns CPU without adding frames.
            33
        } else if self.preview_window_open {
            // Preview editing remains smooth enough at 30 FPS.
            33
        } else if self.recording.is_some() {
            match self.recording_preview_mode {
                PreviewMode::Fps30 => 33,
                PreviewMode::Fps15 => 66,
                PreviewMode::Off => 250,
            }
        } else {
            // Idle + hidden preview: meters/status do not need game-frame cadence.
            100
        };
        ctx.request_repaint_after(std::time::Duration::from_millis(repaint_ms));
    }
}
