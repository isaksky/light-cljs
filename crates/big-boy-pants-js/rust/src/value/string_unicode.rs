//! Unicode 17 String case and normalization readers. Immutable data is generated
//! by scripts/quickjs-data/repack-unicode.pl; no host Unicode version is used.
#[rustfmt::skip]
#[path = "string_data.rs"]
mod data;

pub(crate) fn mapping(cp: u32, kind: usize) -> Option<&'static [u32]> {
    let table = [data::UPPER, data::LOWER, data::NFD, data::NFKD][kind];
    table
        .binary_search_by_key(&cp, |&(c, _)| c)
        .ok()
        .map(|i| table[i].1)
}
pub(crate) fn cc(cp: u32) -> u8 {
    data::CCC
        .binary_search_by_key(&cp, |&(c, _)| c)
        .ok()
        .map_or(0, |i| data::CCC[i].1)
}
pub(crate) fn compose(a: u32, b: u32) -> Option<u32> {
    if (0x1100..0x1113).contains(&a) && (0x1161..0x1176).contains(&b) {
        return Some(0xac00 + (a - 0x1100) * 588 + (b - 0x1161) * 28);
    }
    if (0xac00..=0xd7a3).contains(&a)
        && (a - 0xac00).is_multiple_of(28)
        && (0x11a8..0x11c3).contains(&b)
    {
        return Some(a + b - 0x11a7);
    }
    data::COMPOSE
        .binary_search_by_key(&(a, b), |&(pair, _)| pair)
        .ok()
        .map(|i| data::COMPOSE[i].1)
}
