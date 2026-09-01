#[cfg(windows)]
#[test]
fn hresult_e_fail_value_is_stable() {
    let value = 0x80004005u32 as i32;
    assert_eq!(value as u32, 0x80004005);
}
