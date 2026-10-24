#!/usr/bin/env perl
use strict;
use warnings;
use Digest::SHA qw(sha256_hex);
use Cwd qw(abs_path);
use File::Path qw(make_path);

# Offline expansion of the verified case_conv_* / ID tables. C is only the
# pinned reference decoder, never part of the Rust runtime build.
my ($import, $source, $ucd, $out) = @ARGV;
die "usage: repack-unicode.pl VERIFIED_IMPORT QUICKJS_SOURCE UCD_17_DIRECTORY NEW_OUTPUT\n"
    unless defined $out && @ARGV == 4;
$source = abs_path($source);
sub read_file {
    open my $f, '<', $_[0] or die "$_[0]: $!";
    local $/; return <$f>;
}
sub write_file {
    open my $f, '>', $_[0] or die "$!";
    print $f $_[1]; close $f or die $!;
}
my %pins = (
    'libunicode.c' => '4393c6f495a2e98c3b83fc17cdb2174134328d795148ddc7172cb87a28408878',
    'libunicode.h' => '645fd5fc7f7268da025b717653a37b1b1dfe1d917f8f2ed270397db771cbf3da',
    'libunicode-table.h' => '7fb4fbeffc7bc3e12c837ce602cd9f5a7e9e2b06a9bba95e23d61cdb8b6bc409',
    'cutils.h' => 'd2da6d06a75b9e6c116c82b7a41df6bcc170c8b1779f374fa953ecf688eda647',
    'quickjs.h' => '09d208759171634a5b1e60aa314fd00da5d088ac28621b7ae4682ba2fb103415',
);
for my $name (sort keys %pins) {
    die "unpinned QuickJS $name\n" unless sha256_hex(read_file("$source/$name")) eq $pins{$name};
}
my %ucd_pins = (
    'CaseFolding.txt' => 'ff8d8fefbf123574205085d6714c36149eb946d717a0c585c27f0f4ef58c4183',
    'DerivedCoreProperties.txt' => '24c7fed1195c482faaefd5c1e7eb821c5ee1fb6de07ecdbaa64b56a99da22c08',
    'UnicodeData.txt' => '2e1efc1dcb59c575eedf5ccae60f95229f706ee6d031835247d843c11d96470c',
    'PropList.txt' => '130dcddcaadaf071008bdfce1e7743e04fdfbc910886f017d9f9ac931d8c64dd',
    'emoji-data.txt' => '2cb2bb9455cda83e8481541ecf5b6dfda66a3bb89efa3fa7c5297eccf607b72b',
    'DerivedBinaryProperties.txt' => '13dd09d35a9377e33eb388a01e6581d4bfec6b2685316078c341982fa444071a',
    'DerivedNormalizationProps.txt' => '71fd6a206a2c0cdd41feb6b7f656aa31091db45e9cedc926985d718397f9e488',
);
my %ucd_paths = (
    'emoji-data.txt' => 'emoji/emoji-data.txt',
    'DerivedBinaryProperties.txt' => 'extracted/DerivedBinaryProperties.txt',
);
for my $name (sort keys %ucd_pins) {
    my $text = read_file("$ucd/$name");
    die "unexpected Unicode input/version: $name\n"
        unless sha256_hex($text) eq $ucd_pins{$name};
}
my $provenance = read_file("$import/provenance.txt");
die "import does not identify pinned tables\n"
    unless $provenance =~ /$pins{'libunicode-table.h'}  libunicode-table.h/;
my $tables = read_file("$import/tables.rs");
for my $name (qw(case_conv_table1 case_conv_table2 case_conv_ext)) {
    die "missing verified $name\n" unless $tables =~ /pub static $name:/;
}
# Consume the verified byte pools, including their double terminal NUL, rather
# than interpreting C string literals or flattening away alias boundaries.
sub names {
    my ($pool) = @_;
    $tables =~ /pub static $pool: &\x27static \[u8\] = b"([^"]*)";/ or die "missing $pool\n";
    my $bytes = $1;
    $bytes =~ s/\\x([0-9a-f]{2})/chr(hex($1))/ge;
    die "invalid name pool $pool\n" unless $bytes =~ /^[A-Za-z_,\x00]+\x00\x00$/;
    $bytes =~ s/\x00\x00$//;
    return split /\x00/, $bytes;
}
# ECMA-262 Binary Unicode property aliases: intentionally excludes QuickJS's
# extra ID_Compat_Math_*, InCB and IDS_Unary_Operator properties.
my %binary_allowed = map { $_ => 1 } qw(ASCII ASCII_Hex_Digit Alphabetic Any Assigned
    Bidi_Control Bidi_Mirrored Case_Ignorable Cased Changes_When_Casefolded
    Changes_When_Casemapped Changes_When_Lowercased Changes_When_NFKC_Casefolded
    Changes_When_Titlecased Changes_When_Uppercased Dash Default_Ignorable_Code_Point
    Deprecated Diacritic Emoji Emoji_Component Emoji_Modifier Emoji_Modifier_Base
    Emoji_Presentation Extended_Pictographic Extender Grapheme_Base Grapheme_Extend
    Hex_Digit IDS_Binary_Operator IDS_Trinary_Operator ID_Continue ID_Start Ideographic
    Join_Control Logical_Order_Exception Lowercase Math Noncharacter_Code_Point
    Pattern_Syntax Pattern_White_Space Quotation_Mark Radical Regional_Indicator
    Sentence_Terminal Soft_Dotted Terminal_Punctuation Unified_Ideograph Uppercase
    Variation_Selector White_Space XID_Continue XID_Start);
my @properties;
for my $category (0, 1) {
    for my $aliases (names($category ? 'unicode_gc_name_table' : 'unicode_prop_name_table')) {
        my ($name) = split /,/, $aliases;
        next unless $category || delete $binary_allowed{$name};
        push @properties, { name => $name, aliases => $aliases, category => $category, ranges => [] };
    }
}
die "missing ECMAScript binary properties\n" if keys %binary_allowed;
die "property inventory changed\n" unless @properties == 91;
die "output exists\n" if -e $out;
make_path($out);
my $property_calls = '';
for my $i (0 .. $#properties) {
    my $p = $properties[$i];
    my $function = $p->{category} ? 'unicode_general_category' : 'unicode_prop';
    $property_calls .= "    { CharRange cr; cr_init(&cr, NULL, resize);\n" .
        "      if ($function(&cr, \"$p->{name}\")) return 1;\n" .
        "      for (int j = 0; j < cr.len; j += 2) {\n" .
        "        if (cr.points[j] > 0x10ffff) break;\n" .
        "        unsigned end = cr.points[j + 1] - 1;\n" .
        "        printf(\"P %X %X %X\\n\", $i, cr.points[j], end > 0x10ffff ? 0x10ffff : end); }\n" .
        "      cr_free(&cr); }\n";
}
my $export = <<'C';
#include <stdio.h>
#include <stdlib.h>
#include "cutils.h"
#include "libunicode.h"
static void *resize(void *opaque, void *ptr, size_t size) {
    (void)opaque; return realloc(ptr, size);
}
int main(void) {
    for (unsigned cp = 0; cp <= 0x10ffff; cp++) {
        unsigned folded = lre_canonicalize(cp, 1);
        if (cp != folded) printf("F %X %X\n", cp, folded);
    }
    for (int which = 0; which < 2; which++) {
        unsigned start = 0; int active = 0;
        for (unsigned cp = 0x80; cp <= 0x110000; cp++) {
            int yes = cp <= 0x10ffff && (which ? lre_is_id_continue(cp) : lre_is_id_start(cp));
            if (yes && !active) { start = cp; active = 1; }
            if (!yes && active) { printf("%c %X %X\n", which ? 'C' : 'S', start, cp - 1); active = 0; }
        }
    }
C
write_file("$out/export.c", $export . $property_calls . "    return 0;\n}\n");
system($ENV{CC} // 'cc', '-std=c11', '-O2',
       '-ffunction-sections', '-fdata-sections', '-Wl,--gc-sections', '-I', $source,
       "$out/export.c", "$source/libunicode.c", '-o', "$out/export") == 0 or die "compile reference\n";
open my $pipe, '-|', "$out/export" or die $!;
my (@fold, @start, @continue);
while (<$pipe>) {
    if (/^P ([0-9A-F]+) ([0-9A-F]+) ([0-9A-F]+)\n$/) {
        my ($i, $a, $b) = (hex($1), hex($2), hex($3));
        die "invalid reference property\n" if $i > $#properties || $a > $b || $b > 0x10ffff;
        push @{$properties[$i]{ranges}}, [$a, $b];
        next;
    }
    /^([FSC]) ([0-9A-F]+) ([0-9A-F]+)\n$/ or die "reference output\n";
    push @{ $1 eq 'F' ? \@fold : $1 eq 'S' ? \@start : \@continue }, [hex($2), hex($3)];
}
close $pipe or die "reference failed\n";
my %expected_fold;
for (split /\n/, read_file("$ucd/CaseFolding.txt")) {
    $expected_fold{hex($1)} = hex($2) if /^([0-9A-F]+); [CS]; ([0-9A-F]+);/;
}
my %actual_fold = map { @$_ } @fold;
# This snapshot chooses the opposite representative for the FB05/FB06
# equivalence class. Preserve its equivalence, but emit normative C/S targets.
die "unexpected QuickJS ligature folding\n"
    unless !exists($actual_fold{0xfb05}) && $actual_fold{0xfb06} == 0xfb05;
delete $actual_fold{0xfb06};
$actual_fold{0xfb05} = 0xfb06;
@fold = map { [$_, $actual_fold{$_}] } sort { $a <=> $b } keys %actual_fold;
for my $cp (0 .. 0x10ffff) {
    die sprintf("folding differs from Unicode 17.0.0 at U+%X\n", $cp)
        unless ($actual_fold{$cp} // $cp) == ($expected_fold{$cp} // $cp);
}
for my $property (['ID_Start', \@start], ['ID_Continue', \@continue]) {
    my ($name, $ranges) = @$property;
    my ($expected, $actual) = ('', '');
    for (split /\n/, read_file("$ucd/DerivedCoreProperties.txt")) {
        if (/^([0-9A-F]+)(?:\.\.([0-9A-F]+))?\s*; $name\s*#/) {
            my ($a, $b) = (hex($1), hex($2 // $1));
            for my $cp ($a .. $b) { vec($expected, $cp, 1) = 1 if $cp >= 0x80; }
        }
    }
    for my $range (@$ranges) { vec($actual, $_, 1) = 1 for $range->[0] .. $range->[1]; }
    for my $cp (0x80 .. 0x10ffff) {
        die sprintf("$name differs from Unicode 17.0.0 at U+%X\n", $cp)
            if vec($expected, $cp, 1) != vec($actual, $cp, 1);
    }
}
# Independent Unicode 17 property sets, including all derived binary properties.
my $empty_bits = "\x00" x (0x110000 / 8);
my %expected_bits;
sub set_range {
    my ($name, $a, $b) = @_;
    $expected_bits{$name} //= $empty_bits;
    vec($expected_bits{$name}, $_, 1) = 1 for $a .. $b;
}
for my $file (qw(DerivedCoreProperties.txt PropList.txt emoji-data.txt
                 DerivedBinaryProperties.txt DerivedNormalizationProps.txt)) {
    for (split /\n/, read_file("$ucd/$file")) {
        if (/^([0-9A-F]+)(?:\.\.([0-9A-F]+))?\s*;\s*([A-Za-z_]+)\s*(?:#|$)/) {
            set_range($3, hex($1), hex($2 // $1));
        }
    }
}
my $first;
for (split /\n/, read_file("$ucd/UnicodeData.txt")) {
    my ($cp, $name, $gc) = split /;/;
    next unless defined $gc;
    if ($name =~ /, First>$/) { $first = hex($cp); next; }
    my $a = $name =~ /, Last>$/ ? $first : hex($cp);
    die "unpaired UnicodeData range\n" unless defined $a;
    set_range($gc, $a, hex($cp));
    undef $first;
}
die "unclosed UnicodeData range\n" if defined $first;
$expected_bits{Assigned} = $empty_bits;
for my $gc (qw(Lu Ll Lt Lm Lo Mn Mc Me Nd Nl No Sm Sc Sk So Pc Pd Ps Pe Pi Pf Po Zs Zl Zp Cc Cf Cs Co)) {
    $expected_bits{Assigned} |= $expected_bits{$gc};
    $expected_bits{substr($gc, 0, 1)} //= $empty_bits;
    $expected_bits{substr($gc, 0, 1)} |= $expected_bits{$gc};
}
$expected_bits{Cn} = ~$expected_bits{Assigned};
$expected_bits{C} |= $expected_bits{Cn};
$expected_bits{LC} = $expected_bits{Lu} | $expected_bits{Ll} | $expected_bits{Lt};
$expected_bits{Any} = ~$empty_bits;
set_range('ASCII', 0, 0x7f);
for my $p (@properties) {
    my $bits = $empty_bits;
    for my $range (@{$p->{ranges}}) { vec($bits, $_, 1) = 1 for $range->[0] .. $range->[1]; }
    my $expected = $expected_bits{$p->{name}} // die "no independent UCD property $p->{name}\n";
    if ($bits ne $expected) {
        for my $cp (0 .. 0x10ffff) {
            die sprintf("$p->{name} differs from Unicode 17.0.0 at U+%X\n", $cp)
                if vec($bits, $cp, 1) != vec($expected, $cp, 1);
        }
    }
    $p->{bits} = $bits;
    # Only nonidentity sources contribute additional canonical targets. The
    # complement is formed first: its contributions use !membership(source).
    my ($folded, $complement) = ($empty_bits, $empty_bits);
    for my $mapping (@fold) {
        my ($cp, $target) = @$mapping;
        if (vec($bits, $cp, 1)) { vec($folded, $target, 1) = 1; }
        else { vec($complement, $target, 1) = 1; }
    }
    $p->{folded} = $folded;
    $p->{folded_complement} = $complement;
}
sub bit_ranges {
    my $text = unpack('b*', $_[0]);
    my @ranges;
    while ($text =~ /1+/g) { push @ranges, [$-[0], $+[0] - 1]; }
    return \@ranges;
}
my $header = "// Generated by scripts/quickjs-data/repack-unicode.pl; do not edit.\n" .
    "// Unicode 17.0.0; QuickJS 7d1c13c5d8f8c6b32893786ed35cdcebd88194d2.\n" .
    "// Copyright (c) 2017-2018 Fabrice Bellard. MIT notice: src/intl/data/QUICKJS-LICENSE.\n" .
    "// Unicode License v3: docs/UNICODE-LICENSE. See scripts/quickjs-data/README.md.\n";
sub table {
    my ($name, $visibility, $values) = @_;
    my @rows = @$values;
    my $text = "#[rustfmt::skip]\n${visibility}const $name: &[(u32, u32)] = &[\n";
    while (my @line = splice(@rows, 0, 4)) {
        $text .= '   ' . join('', map { sprintf(" (0x%05X, 0x%05X),", @$_) } @line) . "\n";
    }
    return $text . "];\n";
}
write_file("$out/case_fold_data.rs", $header . table('SIMPLE_FOLD', 'pub(crate) ', \@fold));
write_file("$out/unicode_ident.rs", $header . table('ID_START', 'pub(super) ', \@start) .
    table('ID_CONTINUE', 'pub(super) ', \@continue));
my $property_data = $header . "use super::PropertyData;\n\npub(super) const PROPERTIES: &[PropertyData] = &[\n";
for my $p (@properties) {
    $property_data .= "    PropertyData {\n        aliases: \"$p->{aliases}\",\n" .
        '        category: ' . ($p->{category} ? 'true' : 'false') . ",\n";
    for my $field (qw(ranges folded folded_complement)) {
        $property_data .= "        $field: &[\n";
        my @rows = @{bit_ranges($p->{$field eq 'ranges' ? 'bits' : $field})};
        while (my @line = splice(@rows, 0, 4)) {
            $property_data .= '           ' . join('', map { sprintf(" (0x%05X, 0x%05X),", @$_) } @line) . "\n";
        }
        $property_data .= "        ],\n";
    }
    $property_data .= "    },\n";
}
$property_data .= "];\n";
# Keep generated ranges compact and repeatable across rustfmt releases.
$property_data =~ s/pub\(super\) const PROPERTIES/#[rustfmt::skip]\npub(super) const PROPERTIES/;
write_file("$out/property_data.rs", $property_data);
open my $property_binary, '>:raw', "$out/property-reference.bin" or die $!;
print $property_binary $_->{bits} for @properties;
close $property_binary or die $!;
write_file("$out/provenance.txt", $provenance .
    "Unicode 17.0.0: complete domain folding C/S and ID properties verified.\n" .
    "53 ECMA-262 binary properties and 38 General_Category values verified over the complete domain.\n" .
    "QuickJS FB05/FB06 representative reversed to normative C/S; equivalence unchanged.\n" .
    join('', map { "$pins{$_}  $_\n" } sort keys %pins) .
    join('', map { my $path = $ucd_paths{$_} // $_;
        "$ucd_pins{$_}  https://www.unicode.org/Public/17.0.0/ucd/$path\n" } sort keys %ucd_pins) .
    sha256_hex(read_file($0)) . "  repack-unicode.pl\n");
my ($start_bits, $continue_bits) = ('', '');
for my $range (@start) { vec($start_bits, $_, 1) = 1 for $range->[0] .. $range->[1]; }
for my $range (@continue) { vec($continue_bits, $_, 1) = 1 for $range->[0] .. $range->[1]; }
open my $binary, '>:raw', "$out/reference.bin" or die $!;
for my $cp (0 .. 0x10ffff) {
    print $binary pack('VC', $actual_fold{$cp} // $cp,
        vec($start_bits, $cp, 1) + 2 * vec($continue_bits, $cp, 1));
}
close $binary or die $!;
my $runtime = abs_path('src/regexp/case_fold.rs');
my $ident = abs_path('src/frontend/unicode_ident.rs');
die "run from repository root\n" unless defined $runtime && defined $ident;
write_file("$out/verify.rs", "#[path = \"$runtime\"] mod case_fold;\n" .
    "#[path = \"$ident\"] mod unicode_ident;\n" . <<'R');
fn member(cp: u32, ranges: &[(u32, u32)]) -> bool {
    let i = ranges.partition_point(|&(_, end)| end < cp);
    ranges.get(i).is_some_and(|&(start, _)| cp >= start)
}
fn main() {
    let bytes = std::fs::read(std::env::args_os().nth(1).unwrap()).unwrap();
    assert_eq!(bytes.len(), 0x110000 * 5);
    for cp in 0..=0x10ffffu32 {
        let index = cp as usize * 5;
        let expected = u32::from_le_bytes(bytes[index..index + 4].try_into().unwrap());
        assert_eq!(case_fold::canonicalize(cp, true), expected, "fold U+{cp:X}");
        let flags = u8::from(member(cp, unicode_ident::ID_START))
            + 2 * u8::from(member(cp, unicode_ident::ID_CONTINUE));
        assert_eq!(flags, bytes[index + 4], "identifier U+{cp:X}");
    }
    println!("PASS: runtime folding and identifiers match pinned reference over 1,114,112 code points");
}
R
my $property_runtime = abs_path('src/regexp/property.rs');
my $property_verifier = "#[path = \"$runtime\"] mod case_fold;\n" .
    "#[path = \"$property_runtime\"] mod property;\n" .
    "const ALIASES: &[(&str, bool)] = &[\n" .
    join('', map { '    ("' . $_->{aliases} . '", ' . ($_->{category} ? 'true' : 'false') . "),\n" } @properties) .
    "];\n";
write_file("$out/verify-properties.rs", $property_verifier . <<'R');
fn bit(bytes: &[u8], cp: u32) -> bool {
    bytes[cp as usize / 8] & (1 << (cp % 8)) != 0
}
fn main() {
    let bytes = std::fs::read(std::env::args_os().nth(1).unwrap()).unwrap();
    assert_eq!(bytes.len(), ALIASES.len() * (0x110000 / 8));
    for ((aliases, category), bits) in ALIASES.iter().zip(bytes.chunks_exact(0x110000 / 8)) {
        let p = property::lookup(aliases.split(',').next().unwrap()).unwrap();
        for alias in aliases.split(',') {
            assert_eq!(property::lookup(alias), Some(p));
            for key in ["gc", "General_Category"] {
                assert_eq!(property::lookup(&format!("{key}={alias}")), category.then_some(p));
            }
        }
        let mut positive = vec![false; 0x110000];
        let mut negative = vec![false; 0x110000];
        for cp in 0..=0x10ffffu32 {
            let yes = bit(bits, cp);
            assert_eq!(p.contains(cp, false, false), yes, "{aliases} U+{cp:X}");
            assert_eq!(p.contains(cp, true, false), !yes, "!{aliases} U+{cp:X}");
            let folded = case_fold::canonicalize(cp, true) as usize;
            if yes { positive[folded] = true; } else { negative[folded] = true; }
        }
        for cp in 0..=0x10ffffu32 {
            let folded = case_fold::canonicalize(cp, true);
            assert_eq!(p.contains(folded, false, true), positive[folded as usize], "iu {aliases} U+{cp:X}");
            assert_eq!(p.contains(folded, true, true), negative[folded as usize], "iu !{aliases} U+{cp:X}");
        }
    }
    println!("PASS: all 91 property sets, aliases, complements and iu images match the complete-domain reference");
}
R
print "PASS: full-domain QuickJS / Unicode 17.0.0 equality; ", scalar(@fold), " simple mappings\n";
