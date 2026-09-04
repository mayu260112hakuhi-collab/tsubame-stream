use serde::{Deserialize, Serialize};
use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

pub const SETTINGS_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MixerWindowSettings {
    #[serde(default = "default_true")]
    pub open: bool,
    #[serde(default)]
    pub position: Option<[f32; 2]>,
    #[serde(default)]
    pub size: Option<[f32; 2]>,
}

impl Default for MixerWindowSettings {
    fn default() -> Self {
        Self {
            open: true,
            position: None,
            size: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default = "settings_format_version")]
    pub format_version: u32,
    #[serde(default)]
    pub first_run_complete: bool,
    #[serde(default = "default_preset")]
    pub preset: String,
    #[serde(default = "default_encoder")]
    pub encoder_preference: String,
    #[serde(default = "default_recording_preview")]
    pub recording_preview_mode: String,
    #[serde(default = "default_platform")]
    pub streaming_platform: String,
    #[serde(default)]
    pub streaming_server_url: String,
    #[serde(default = "default_bitrate")]
    pub streaming_video_bitrate_kbps: u32,
    #[serde(default)]
    pub input_device_id: Option<String>,
    #[serde(default)]
    pub output_device_id: Option<String>,
    #[serde(default = "default_true")]
    pub preview_window_open: bool,
    #[serde(default)]
    pub preview_window_position: Option<[f32; 2]>,
    #[serde(default)]
    pub preview_window_size: Option<[f32; 2]>,
    #[serde(default)]
    pub mixer_window: MixerWindowSettings,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            format_version: SETTINGS_FORMAT_VERSION,
            first_run_complete: false,
            preset: default_preset(),
            encoder_preference: default_encoder(),
            recording_preview_mode: default_recording_preview(),
            streaming_platform: default_platform(),
            streaming_server_url: String::new(),
            streaming_video_bitrate_kbps: default_bitrate(),
            input_device_id: None,
            output_device_id: None,
            preview_window_open: true,
            preview_window_position: None,
            preview_window_size: None,
            mixer_window: MixerWindowSettings::default(),
        }
    }
}

fn settings_format_version() -> u32 {
    SETTINGS_FORMAT_VERSION
}
fn default_true() -> bool {
    true
}
fn default_preset() -> String {
    "game".to_owned()
}
fn default_encoder() -> String {
    "amf".to_owned()
}
fn default_recording_preview() -> String {
    "fps15".to_owned()
}
fn default_platform() -> String {
    "youtube".to_owned()
}
fn default_bitrate() -> u32 {
    12_000
}

pub fn sanitize_window_position(position: Option<[f32; 2]>) -> Option<[f32; 2]> {
    position.filter(|[x, y]| x.is_finite() && y.is_finite())
}

pub fn sanitize_window_size(size: Option<[f32; 2]>) -> Option<[f32; 2]> {
    size.filter(|[w, h]| w.is_finite() && h.is_finite() && *w >= 320.0 && *h >= 240.0)
}

pub fn rects_intersect(
    a_pos: [f32; 2],
    a_size: [f32; 2],
    b_pos: [f32; 2],
    b_size: [f32; 2],
) -> bool {
    let a_right = a_pos[0] + a_size[0];
    let a_bottom = a_pos[1] + a_size[1];
    let b_right = b_pos[0] + b_size[0];
    let b_bottom = b_pos[1] + b_size[1];

    a_pos[0] < b_right
        && a_right > b_pos[0]
        && a_pos[1] < b_bottom
        && a_bottom > b_pos[1]
}

pub fn settings_path() -> PathBuf {
    let root = env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    root.join("Tsubame").join("settings.json")
}

pub fn load_settings() -> Result<AppSettings, String> {
    load_settings_from(&settings_path())
}

pub fn save_settings(settings: &AppSettings) -> Result<PathBuf, String> {
    let path = settings_path();
    save_settings_to(&path, settings)?;
    Ok(path)
}

fn load_settings_from(path: &Path) -> Result<AppSettings, String> {
    if !path.exists() {
        return Ok(AppSettings::default());
    }
    let text =
        fs::read_to_string(path).map_err(|err| format!("設定ファイルを読み込めません: {err}"))?;
    let mut settings: AppSettings = serde_json::from_str(&text)
        .map_err(|err| format!("設定ファイルのJSONが不正です: {err}"))?;
    if settings.format_version == 0 {
        settings.format_version = SETTINGS_FORMAT_VERSION;
    }
    Ok(settings)
}

fn save_settings_to(path: &Path, settings: &AppSettings) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("設定フォルダを作成できません: {err}"))?;
    }

    let text = serde_json::to_string_pretty(settings)
        .map_err(|err| format!("設定をJSONに変換できません: {err}"))?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, text).map_err(|err| format!("設定ファイルを書き込めません: {err}"))?;
    replace_file(&tmp, path).map_err(|err| format!("設定ファイルを確定できません: {err}"))?;
    Ok(())
}

fn replace_file(tmp: &Path, path: &Path) -> io::Result<()> {
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_preserves_distribution_safe_fields() {
        let mut settings = AppSettings::default();
        settings.first_run_complete = true;
        settings.streaming_server_url = "rtmps://example.invalid/live2".to_owned();
        settings.input_device_id = Some("mic-id".to_owned());
        settings.output_device_id = Some("speaker-id".to_owned());
        settings.preview_window_position = Some([2020.0, 120.0]);
        settings.preview_window_size = Some([960.0, 620.0]);
        settings.mixer_window.open = false;
        settings.mixer_window.position = Some([120.0, 80.0]);
        settings.mixer_window.size = Some([920.0, 540.0]);

        let path =
            std::env::temp_dir().join(format!("tsubame-settings-test-{}.json", std::process::id()));
        let _ = fs::remove_file(&path);
        save_settings_to(&path, &settings).unwrap();
        let loaded = load_settings_from(&path).unwrap();
        let _ = fs::remove_file(&path);

        assert_eq!(settings, loaded);
    }

    #[test]
    fn mixer_window_defaults_open_on_first_run() {
        let settings = AppSettings::default();
        assert!(settings.mixer_window.open);
        assert_eq!(settings.mixer_window.position, None);
        assert_eq!(settings.mixer_window.size, None);
    }

    #[test]
    fn legacy_json_without_mixer_window_loads_with_open_default() {
        let json = r#"{
            "format_version": 1,
            "first_run_complete": true,
            "preset": "game",
            "encoder_preference": "amf",
            "recording_preview_mode": "fps15",
            "streaming_platform": "youtube",
            "streaming_server_url": "",
            "streaming_video_bitrate_kbps": 12000,
            "input_device_id": null,
            "output_device_id": null,
            "preview_window_open": true
        }"#;

        let loaded: AppSettings = serde_json::from_str(json).unwrap();
        assert!(loaded.mixer_window.open);
        assert_eq!(loaded.mixer_window.position, None);
        assert_eq!(loaded.mixer_window.size, None);
    }

    #[test]
    fn preview_window_geometry_defaults_for_legacy_settings() {
        let json = r#"{
            "format_version": 1,
            "first_run_complete": true,
            "preset": "game",
            "encoder_preference": "amf",
            "recording_preview_mode": "fps15",
            "streaming_platform": "youtube",
            "streaming_server_url": "",
            "streaming_video_bitrate_kbps": 12000,
            "input_device_id": null,
            "output_device_id": null,
            "preview_window_open": true
        }"#;

        let loaded: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(loaded.preview_window_position, None);
        assert_eq!(loaded.preview_window_size, None);
    }

    #[test]
    fn mixer_window_geometry_rejects_invalid_values() {
        assert_eq!(sanitize_window_position(Some([f32::NAN, 10.0])), None);
        assert_eq!(sanitize_window_size(Some([100.0, 100.0])), None);
        assert_eq!(
            sanitize_window_size(Some([640.0, 480.0])),
            Some([640.0, 480.0])
        );
    }

    #[test]
    fn window_rect_intersection_distinguishes_visible_and_offscreen() {
        assert!(rects_intersect(
            [100.0, 100.0],
            [500.0, 400.0],
            [0.0, 0.0],
            [1920.0, 1080.0]
        ));
        assert!(!rects_intersect(
            [2500.0, 100.0],
            [500.0, 400.0],
            [0.0, 0.0],
            [1920.0, 1080.0]
        ));
    }

}
