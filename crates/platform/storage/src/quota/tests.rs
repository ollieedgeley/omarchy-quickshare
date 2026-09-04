use super::available_from_stat;

#[test]
fn available_from_stat_multiplies_blocks_by_fragment_size() {
    assert_eq!(available_from_stat(2_u64, 4096_u64), 8192_u64);
}

#[test]
fn available_from_stat_caps_overflow_at_u64_max() {
    assert_eq!(available_from_stat(u64::MAX, 2_u64), u64::MAX);
}

#[test]
fn available_from_stat_returns_zero_for_zero_operands() {
    assert_eq!(available_from_stat(0_u64, 4096_u64), 0_u64);
    assert_eq!(available_from_stat(8_u64, 0_u64), 0_u64);
}
