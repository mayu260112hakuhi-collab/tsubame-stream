pub const NORMAL_PREVIEW_WIDTH: f32 = 300.0;
pub const PREVIEW_ASPECT_W: f32 = 16.0;
pub const PREVIEW_ASPECT_H: f32 = 9.0;

pub const STREAM_TOOLBAR_LABELS: [&str; 4] = ["録画", "配信", "録画＋配信", "配信先: 未設定"];
pub const MIXER_CHANNELS: [&str; 3] = ["PC音声", "マイク", "配信Mix"];
pub const SCENE_LABELS: [&str; 4] = ["ゲーム", "雑談", "作業", "離席"];
pub const SOURCE_LABELS: [&str; 4] = ["ゲームキャプチャ", "ワイプ", "画像 / アイコン", "テキスト"];

pub fn normal_preview_size() -> [f32; 2] {
    [
        NORMAL_PREVIEW_WIDTH,
        NORMAL_PREVIEW_WIDTH * PREVIEW_ASPECT_H / PREVIEW_ASPECT_W,
    ]
}

pub const SOURCE_SELECTOR_WIDTH: f32 = 260.0;
pub const AUDIO_METER_WIDTH: f32 = 220.0;
pub const AUDIO_METER_HEIGHT: f32 = 8.0;

pub const METER_GREEN_END_DB: f32 = -18.0;
pub const METER_YELLOW_END_DB: f32 = -12.0;
pub const METER_ORANGE_END_DB: f32 = -6.0;

pub fn compact_source_name(name: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }

    let count = name.chars().count();
    if count <= max_chars {
        return name.to_owned();
    }

    if max_chars == 1 {
        return "…".to_owned();
    }

    let mut compact: String = name.chars().take(max_chars - 1).collect();
    compact.push('…');
    compact
}
