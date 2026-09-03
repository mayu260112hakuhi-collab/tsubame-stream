use serde::{Deserialize, Serialize};
use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

pub const SETTINGS_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

        let path =
            std::env::temp_dir().join(format!("tsubame-settings-test-{}.json", std::process::id()));
        let _ = fs::remove_file(&path);
        save_settings_to(&path, &settings).unwrap();
        let loaded = load_settings_from(&path).unwrap();
        let _ = fs::remove_file(&path);

        assert_eq!(settings, loaded);
    }
}
