#![forbid(unsafe_code)]
#[path = "tables.rs"]
mod tables;
use tables::*;

#[test]
fn complete_top_level_collections() {
    assert_eq!(locale_data_table.len(), 769);
    assert_eq!(locale_data_count as usize, locale_data_table.len());
    assert_eq!(ti_tz_zone_data.len(), 59);
    assert_eq!(ti_tz_zone_count as usize, ti_tz_zone_data.len());
    assert_eq!(ti_tz_names.len(), 598);
    assert_eq!(likely_subtags_tab.len(), 7788);
    assert_eq!(display_names_data.len(), 2);
}

#[test]
fn embedded_nuls_and_utf8_survive() {
    assert_eq!(likely_subtags_tab[0], Some(&b"aa\0aa-Latn-ET\0"[..]));
    let french = locale_data_table.iter().find(|l| l.locale == Some(&b"fr\0"[..])).unwrap();
    let group = group_tab[french.symbols.group_idx as usize].unwrap();
    assert_eq!(group, "\u{202f}\0".as_bytes());
    let months = month_long_tab[french.month_long_idx as usize].unwrap();
    assert!(months.windows("février".len()).any(|s| s == "février".as_bytes()));
    assert!(months.iter().filter(|&&b| b == 0).count() >= 12);
    assert_eq!(months.last(), Some(&0));
}

#[test]
fn negative_offsets_and_minimum_timestamp_survive() {
    let zone = ti_tz_zone_data.iter().find(|z| z.canonical_name == Some(&b"Africa/Monrovia\0"[..])).unwrap();
    let transitions = zone.transitions.unwrap();
    assert_eq!(transitions[0].start_sec, i64::MIN);
    assert_eq!(transitions[0].offset_sec, -2588);
    assert_eq!(transitions.len(), zone.transition_count as usize);
}

#[test]
fn reference_lengths_and_packed_fields_agree() {
    for locale in locale_data_table {
        assert_eq!(locale.compactPatterns.unwrap().len(), locale.numCompactPatterns as usize);
        assert!((locale.symbols.decimal_idx as usize) < decimal_tab.len());
        assert!((locale.patterns.dec_pos as usize) < pattern_tab.len());
    }
    for zone in ti_tz_zone_data {
        let transitions = zone.transitions.unwrap();
        assert_eq!(transitions.len(), zone.transition_count as usize);
        assert!(transitions.windows(2).all(|w| w[0].start_sec < w[1].start_sec));
    }
    for alias in ti_tz_aliases {
        assert!(alias.canonical_id >= 0 && (alias.canonical_id as usize) < ti_tz_zone_data.len());
    }
    assert_eq!(unicode_prop_table.len(), unicode_prop_len_table.len());
    for (table, &length) in unicode_prop_table.iter().zip(unicode_prop_len_table) {
        assert_eq!(table.unwrap().len(), length as usize);
    }
    assert_eq!(case_conv_table1.len(), 378);
    assert_eq!(case_conv_table1.len(), case_conv_table2.len());
}
