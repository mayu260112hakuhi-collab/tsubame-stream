use stream_capture::pack_pitched_rgba;

#[test]
fn pitched_rows_are_compacted_without_padding() {
    // 2 RGBA pixels = 8 useful bytes per row, but GPU row pitch is 12.
    let src = vec![
        1, 2, 3, 4, 5, 6, 7, 8, 99, 99, 99, 99, 9, 10, 11, 12, 13, 14, 15, 16, 88, 88, 88, 88,
    ];

    let packed = pack_pitched_rgba(&src, 12, 2, 2);

    assert_eq!(
        packed,
        vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,]
    );
}

#[test]
fn zero_geometry_produces_empty_preview_buffer() {
    assert!(pack_pitched_rgba(&[], 0, 0, 0).is_empty());
}
