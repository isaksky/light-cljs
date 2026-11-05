#![forbid(unsafe_code)]
#[path = "tables.rs"]
mod tables;
use tables::*;

#[path = "script_names.rs"]
mod script_names;

#[test]
fn regexp_script_alias_subset_is_exact() {
    assert_eq!(script_names::SCRIPT_ALIASES, unicode_script_name_table);
    assert!(script_names::SCRIPT_ALIASES.ends_with(b"\0\0"));
    assert!(script_names::SCRIPT_ALIASES.split(|&b| b == 0).any(|s| s == b"Greek,Grek"));
}

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
fn display_names_compound_language_literals_survive() {
    assert_eq!(dn_language_fallbacks, &[
        Some(&b"zh-Hans-CN\0"[..]), Some(&b"Chinese (Simplified, China)\0"[..]),
        Some(&b"zh-Hant-TW\0"[..]), Some(&b"Chinese (Traditional, Taiwan)\0"[..]),
    ]);
    let english = display_names_data[0].languages.unwrap();
    assert!(english.windows(b"en-US\0American English\0".len()).any(|s| s == b"en-US\0American English\0"));
    let french = display_names_data[1].languages.unwrap();
    assert!(french.windows("de\0allemand\0".len()).any(|s| s == b"de\0allemand\0"));
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

#[test]
fn numbering_system_indices_and_digits_agree() {
    assert_eq!(intl_all_numbering_systems.len(), nf_digit_base.len());
    let index = |name: &[u8]| intl_all_numbering_systems.iter().position(|&n| n == Some(name)).unwrap();
    assert_eq!(nf_digit_base[index(b"latn\0")], 0x30);
    assert_eq!(nf_digit_base[index(b"arab\0")], 0x660);
    assert_eq!(nf_digit_base[index(b"mathbold\0")], 0x1d7ce);
    assert_eq!(nf_digit_base[index(b"roman\0")], 0);
    assert_eq!(nf_digit_base[index(b"hanidec\0")], 0);
    assert_eq!(hanidec_cps, &[0x3007, 0x4e00, 0x4e8c, 0x4e09, 0x56db, 0x4e94, 0x516d, 0x4e03, 0x516b, 0x4e5d]);
    for &base in nf_digit_base.iter().filter(|&&base| base != 0) {
        for digit in 0..10 { assert!(char::from_u32(base + digit).is_some()); }
    }
}

#[test]
fn relative_time_literals_and_auto_switches_survive() {
    assert_eq!(rtf_units.len(), 11 * 37);
    assert_eq!(rtf_units[0], Some(&b"en\0"[..]));
    assert_eq!(rtf_units[4], Some(&b"day\0"[..]));
    assert_eq!(rtf_units[33], Some(&b"in \0"[..]));
    assert_eq!(rtf_en_narrow[5], Some(&b"mo\0"[..]));
    let index = |name: &[u8]| rtf_auto_locales.iter().position(|&n| n == Some(name)).unwrap();
    assert_eq!(rtf_auto.len(), rtf_auto_locales.len() * 40);
    assert_eq!(rtf_auto[index(b"de\0") * 40 + 8 + 3], Some("gestern\0".as_bytes()));
    assert_eq!(rtf_auto[index(b"en\0") * 40 + 16 + 3], Some("today\0".as_bytes()));
    assert_eq!(rtf_auto[index(b"ja\0") * 40 + 8 + 4], Some("先週\0".as_bytes()));
    for row in rtf_auto.chunks_exact(40) {
        assert!(row[..8].iter().all(Option::is_none));
        assert!(row[32..].iter().all(Option::is_none));
    }
}

#[test]
fn duration_unit_patterns_preserve_nullable_forms() {
    assert_eq!(nf_unit_patterns.len(), 28 * 8);
    let rows: Vec<_> = nf_unit_patterns.chunks_exact(8).collect();
    let gb = rows.iter().find(|r| r[0] == Some(&b"en-GB\0"[..]) && r[1] == Some(&b"minute\0"[..])).unwrap();
    assert_eq!(gb[2], Some(&b"{number} mins\0"[..]));
    assert_eq!(gb[3], Some(&b"{number} min\0"[..]));
    let de = rows.iter().find(|r| r[0] == Some(&b"de\0"[..]) && r[1] == Some(&b"hour\0"[..])).unwrap();
    assert_eq!(de[2], Some(&b"{number} Std.\0"[..]));
    assert_eq!(de[3], None);
    assert_eq!(rows.last().unwrap()[6], Some("{number} 秒\0".as_bytes()));
}
