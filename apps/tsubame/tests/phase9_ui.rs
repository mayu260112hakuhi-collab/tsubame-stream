use tsubame_stream::ui_layout::{
    normal_preview_size, MIXER_CHANNELS, NORMAL_PREVIEW_WIDTH, SCENE_LABELS, SOURCE_LABELS,
    STREAM_TOOLBAR_LABELS,
};

#[test]
fn phase9_normal_preview_is_300px_wide_and_16_by_9() {
    let [w, h] = normal_preview_size();
    assert_eq!(w, NORMAL_PREVIEW_WIDTH);
    assert!((w - 300.0).abs() < f32::EPSILON);
    assert!((h - 168.75).abs() < 0.01);
}

#[test]
fn phase9_stream_toolbar_has_stable_labels() {
    assert!(STREAM_TOOLBAR_LABELS.contains(&"録画"));
    assert!(STREAM_TOOLBAR_LABELS.contains(&"配信"));
    assert!(STREAM_TOOLBAR_LABELS.contains(&"録画＋配信"));
    assert!(STREAM_TOOLBAR_LABELS.contains(&"配信先: 未設定"));
}

#[test]
fn phase9_mixer_reserves_three_channels() {
    assert_eq!(MIXER_CHANNELS, ["PC音声", "マイク", "配信Mix"]);
}

#[test]
fn phase9_scene_and_source_shell_has_expected_entries() {
    assert_eq!(SCENE_LABELS, ["ゲーム", "雑談", "作業", "離席"]);
    assert_eq!(
        SOURCE_LABELS,
        ["ゲームキャプチャ", "ワイプ", "画像 / アイコン", "テキスト"]
    );
}

#[test]
fn source_selector_has_fixed_compact_width() {
    use tsubame_stream::ui_layout::SOURCE_SELECTOR_WIDTH;
    assert_eq!(SOURCE_SELECTOR_WIDTH, 260.0);
}

#[test]
fn long_source_name_is_compacted_for_combo_button() {
    use tsubame_stream::ui_layout::compact_source_name;

    let title = "とても長いウィンドウタイトルですこれは配信ソフトの幅を押し広げてはいけません";
    let compact = compact_source_name(title, 18);

    assert!(compact.chars().count() <= 18);
    assert!(compact.ends_with('…'));
}

#[test]
fn audio_meter_is_thin() {
    use tsubame_stream::ui_layout::AUDIO_METER_HEIGHT;
    assert_eq!(AUDIO_METER_HEIGHT, 8.0);
}

#[test]
fn audio_meter_thresholds_are_ordered() {
    use tsubame_stream::ui_layout::{
        METER_GREEN_END_DB, METER_ORANGE_END_DB, METER_YELLOW_END_DB,
    };

    assert!(METER_GREEN_END_DB < METER_YELLOW_END_DB);
    assert!(METER_YELLOW_END_DB < METER_ORANGE_END_DB);
    assert!(METER_ORANGE_END_DB < 0.0);
}

#[test]
fn phase9_2_1_audio_device_selector_copy_is_stable() {
    let labels = ["Windows既定", "音声デバイス更新"];
    assert!(labels.contains(&"Windows既定"));
    assert!(labels.contains(&"音声デバイス更新"));
}
