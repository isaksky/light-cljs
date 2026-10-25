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
    'cutils.c' => 'b73a403a59da30726257ddbdf5e399298941c1def997782ee0d4d33f796a80a2',
    'cutils.h' => 'd2da6d06a75b9e6c116c82b7a41df6bcc170c8b1779f374fa953ecf688eda647',
    'quickjs.h' => '09d208759171634a5b1e60aa314fd00da5d088ac28621b7ae4682ba2fb103415',
);
for my $name (sort keys %pins) {
    die "unpinned QuickJS $name\n" unless sha256_hex(read_file("$source/$name")) eq $pins{$name};
}
my %ucd_pins = (
    'CaseFolding.txt' => 'ff8d8fefbf123574205085d6714c36149eb946d717a0c585c27f0f4ef58c4183',
    'SpecialCasing.txt' => 'efc25faf19de21b92c1194c111c932e03d2a5eaf18194e33f1156e96de4c9588',
    'DerivedCoreProperties.txt' => '24c7fed1195c482faaefd5c1e7eb821c5ee1fb6de07ecdbaa64b56a99da22c08',
    'UnicodeData.txt' => '2e1efc1dcb59c575eedf5ccae60f95229f706ee6d031835247d843c11d96470c',
    'PropList.txt' => '130dcddcaadaf071008bdfce1e7743e04fdfbc910886f017d9f9ac931d8c64dd',
    'emoji-data.txt' => '2cb2bb9455cda83e8481541ecf5b6dfda66a3bb89efa3fa7c5297eccf607b72b',
    'DerivedBinaryProperties.txt' => '13dd09d35a9377e33eb388a01e6581d4bfec6b2685316078c341982fa444071a',
    'DerivedNormalizationProps.txt' => '71fd6a206a2c0cdd41feb6b7f656aa31091db45e9cedc926985d718397f9e488',
    'Scripts.txt' => '9f5e50d3abaee7d6ce09480f325c706f485ae3240912527e651954d2d6b035bf',
    'ScriptExtensions.txt' => 'ec2107e58825a1586acee8e0911ce18260394ac8b87e535ca325f1ccbeb06bc6',
    'PropertyValueAliases.txt' => '64e9a5f76f7a1e8b5a47d6a1f9a26522a251208f5276bdfa1559dac7cf2e827a',
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
        push @properties, { name => $name, aliases => $aliases,
            kind => $category ? 'Category' : 'Binary', ranges => [] };
    }
}
die "missing ECMAScript binary properties\n" if keys %binary_allowed;
die "property inventory changed\n" unless @properties == 91;
# Check every exact script alias against PropertyValueAliases, independently
# of the QuickJS name pool. Both long names and deprecated aliases are valid.
my %script_aliases;
for (split /\n/, read_file("$ucd/PropertyValueAliases.txt")) {
    s/#.*//;
    next unless /^sc\s*;/;
    my @fields = split /\s*;\s*/;
    shift @fields;
    s/^\s+|\s+$//g for @fields;
    $script_aliases{$fields[1]} = join(',', sort @fields);
}
for my $kind ('Script', 'ScriptExtensions') {
    my %remaining = %script_aliases;
    for my $aliases (names('unicode_script_name_table')) {
        my ($name) = split /,/, $aliases;
        die "script aliases differ from Unicode 17: $name\n"
            unless (delete $remaining{$name} // '') eq join(',', sort split /,/, $aliases);
        push @properties, { name => $name, aliases => $aliases, kind => $kind, ranges => [] };
    }
    die "missing Unicode 17 scripts\n" if keys %remaining;
}
die "output exists\n" if -e $out;
make_path($out);
my $property_calls = '';
for my $i (0 .. $#properties) {
    my $p = $properties[$i];
    my $function = $p->{kind} eq 'Category' ? 'unicode_general_category' :
        $p->{kind} eq 'Binary' ? 'unicode_prop' : 'unicode_script';
    my $ext = $p->{kind} eq 'ScriptExtensions' ? ', 1' :
        $p->{kind} eq 'Script' ? ', 0' : '';
    $property_calls .= "    { CharRange cr; cr_init(&cr, NULL, resize);\n" .
        "      if ($function(&cr, \"$p->{name}\"$ext)) return 1;\n" .
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
int main(int argc, char **argv) {
    if (argc > 2) {
        FILE *input = fopen(argv[2], "r");
        unsigned pair[2];
        if (!input) return 1;
        while (fscanf(input, "%X %X", &pair[0], &pair[1]) == 2) {
            unsigned *dst;
            int n = unicode_normalize(&dst, pair, 2, UNICODE_NFC, NULL, resize);
            if (n != 1) return 1;
            printf("P %X %X %X\n", pair[0], pair[1], dst[0]);
            free(dst);
        }
        fclose(input);
        return 0;
    }
    if (argc > 1) {
        for (unsigned cp = 0; cp <= 0x10ffff; cp++) {
            unsigned buf[3];
            for (int dir = 0; dir < 2; dir++) {
                int n = lre_case_conv(buf, cp, dir);
                if (n != 1 || buf[0] != cp) {
                    printf("%c %X", dir ? 'D' : 'U', cp);
                    for (int j = 0; j < n; j++) printf(" %X", buf[j]);
                    puts("");
                }
            }
            for (int compat = 0; compat < 2; compat++) {
                unsigned *dst;
                int n = unicode_normalize(&dst, &cp, 1, compat ? UNICODE_NFKD : UNICODE_NFD, NULL, resize);
                if (n < 1) return 1;
                if (n != 1 || dst[0] != cp) {
                    printf("%c %X", compat ? 'K' : 'N', cp);
                    for (int j = 0; j < n; j++) printf(" %X", dst[j]);
                    puts("");
                }
                free(dst);
            }
            unsigned cc = unicode_get_cc(cp);
            if (cc) printf("B %X %X\n", cp, cc);
        }
        return 0;
    }
    for (unsigned cp = 0; cp <= 0xffff; cp++) {
        unsigned upper = lre_canonicalize(cp, 0);
        if (cp != upper) printf("L %X %X\n", cp, upper);
    }
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
       "$out/export.c", "$source/libunicode.c", "$source/cutils.c", '-o', "$out/export") == 0 or die "compile reference\n";
open my $pipe, '-|', "$out/export" or die $!;
my (@fold, @legacy, @start, @continue);
while (<$pipe>) {
    if (/^P ([0-9A-F]+) ([0-9A-F]+) ([0-9A-F]+)\n$/) {
        my ($i, $a, $b) = (hex($1), hex($2), hex($3));
        die "invalid reference property\n" if $i > $#properties || $a > $b || $b > 0x10ffff;
        push @{$properties[$i]{ranges}}, [$a, $b];
        next;
    }
    /^([FLSC]) ([0-9A-F]+) ([0-9A-F]+)\n$/ or die "reference output\n";
    push @{ $1 eq 'F' ? \@fold : $1 eq 'L' ? \@legacy : $1 eq 'S' ? \@start : \@continue }, [hex($2), hex($3)];
}
close $pipe or die "reference failed\n";
# Independently apply ECMA-262 non-Unicode Canonicalize to full default
# uppercase mappings. SpecialCasing overrides UnicodeData's simple mappings;
# conditional/locale-sensitive entries do not apply to default toUpperCase.
my %upper;
for (split /\n/, read_file("$ucd/UnicodeData.txt")) {
    my @fields = split /;/;
    $upper{hex($fields[0])} = [hex($fields[12])] if $fields[12];
}
for (split /\n/, read_file("$ucd/SpecialCasing.txt")) {
    s/#.*//;
    if (/^([0-9A-F]+);[^;]*;[^;]*;\s*([0-9A-F ]+);\s*$/) {
        $upper{hex($1)} = [map { hex } split ' ', $2];
    }
}
my %actual_legacy = map { @$_ } @legacy;
for my $cp (0 .. 0xffff) {
    my $upper = $upper{$cp} // [$cp];
    my $expected = @$upper == 1 && $upper->[0] <= 0xffff
        && !($cp >= 128 && $upper->[0] < 128) ? $upper->[0] : $cp;
    die sprintf("legacy Canonicalize differs from Unicode 17.0.0 at U+%X\n", $cp)
        unless ($actual_legacy{$cp} // $cp) == $expected;
}
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
# Script defaults to Unknown. Script_Extensions defaults to Script, but each
# explicit row replaces that default (including Common and Inherited).
my %script_bits = map { $_ => $empty_bits } keys %script_aliases;
my $assigned_scripts = $empty_bits;
for (split /\n/, read_file("$ucd/Scripts.txt")) {
    if (/^([0-9A-F]+)(?:\.\.([0-9A-F]+))?\s*;\s*([A-Za-z_]+)\s*#/) {
        my ($a, $b, $name) = (hex($1), hex($2 // $1), $3);
        die "unknown UCD script $name\n" unless exists $script_bits{$name};
        for my $cp ($a .. $b) {
            vec($script_bits{$name}, $cp, 1) = 1;
            vec($assigned_scripts, $cp, 1) = 1;
        }
    }
}
$script_bits{Unknown} = ~$assigned_scripts;
my %scx_bits = %script_bits;
my %long_name;
for my $name (keys %script_aliases) {
    $long_name{$_} = $name for split /,/, $script_aliases{$name};
}
for (split /\n/, read_file("$ucd/ScriptExtensions.txt")) {
    if (/^([0-9A-F]+)(?:\.\.([0-9A-F]+))?\s*;\s*([A-Za-z_ ]+?)\s*#/) {
        my ($a, $b, $names) = (hex($1), hex($2 // $1), $3);
        for my $name (keys %scx_bits) {
            vec($scx_bits{$name}, $_, 1) = 0 for $a .. $b;
        }
        for my $alias (split ' ', $names) {
            my $name = $long_name{$alias} // die "unknown scx alias $alias\n";
            vec($scx_bits{$name}, $_, 1) = 1 for $a .. $b;
        }
    }
}
for my $p (@properties) {
    my $bits = $empty_bits;
    for my $range (@{$p->{ranges}}) { vec($bits, $_, 1) = 1 for $range->[0] .. $range->[1]; }
    my $expected = ($p->{kind} eq 'Script' ? $script_bits{$p->{name}} :
        $p->{kind} eq 'ScriptExtensions' ? $scx_bits{$p->{name}} : $expected_bits{$p->{name}})
        // die "no independent UCD property $p->{name}\n";
    if ($bits ne $expected) {
        for my $cp (0 .. 0x10ffff) {
            die sprintf("$p->{kind}=$p->{name} differs from Unicode 17.0.0 at U+%X\n", $cp)
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
write_file("$out/case_fold_data.rs", $header . table('SIMPLE_FOLD', 'pub(crate) ', \@fold) .
    table('LEGACY_UPPER', 'pub(crate) ', \@legacy));
write_file("$out/unicode_ident.rs", $header . table('ID_START', 'pub(super) ', \@start) .
    table('ID_CONTINUE', 'pub(super) ', \@continue));
my $property_data = $header . "use super::{PropertyData, PropertyGroup};\n\n#[rustfmt::skip]\npub(super) const PROPERTIES: &[PropertyData] = &[\n";
my (%range_ids, @range_bits);
for my $p (@properties) {
    $property_data .= "    PropertyData {\n        aliases: \"$p->{aliases}\",\n" .
        "        group: PropertyGroup::$p->{kind},\n";
    for my $field (qw(ranges folded folded_complement)) {
        my $bits = $p->{$field eq 'ranges' ? 'bits' : $field};
        if (!exists $range_ids{$bits}) {
            $range_ids{$bits} = scalar @range_bits;
            push @range_bits, $bits;
        }
        $property_data .= "        $field: RANGES_$range_ids{$bits},\n";
    }
    $property_data .= "    },\n";
}
$property_data .= "];\n";
# Intern identical range sets, especially the many empty script folding sets
# and identical folded complements. Normal builds use only these safe ranges.
for my $i (0 .. $#range_bits) {
    $property_data .= "\n" . table("RANGES_$i", '', bit_ranges($range_bits[$i]));
}
write_file("$out/property_data.rs", $property_data);
open my $property_binary, '>:raw', "$out/property-reference.bin" or die $!;
print $property_binary $_->{bits} for @properties;
close $property_binary or die $!;
write_file("$out/provenance.txt", $provenance .
    "Unicode 17.0.0: complete domain folding C/S and ID properties verified.\n" .
    "Non-Unicode Canonicalize verified over all 65,536 code units against full default uppercase.\n" .
    "53 ECMA-262 binary properties and 38 General_Category values verified over the complete domain.\n" .
    "Script/Script_Extensions sets and all PropertyValueAliases verified against Unicode 17.0.0.\n" .
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
open my $legacy_binary, '>:raw', "$out/legacy-reference.bin" or die $!;
print $legacy_binary pack('V', $actual_legacy{$_} // $_) for 0 .. 0xffff;
close $legacy_binary or die $!;
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
    let bytes = std::fs::read(std::env::args_os().nth(2).unwrap()).unwrap();
    assert_eq!(bytes.len(), 0x10000 * 4);
    for (cp, bytes) in bytes.chunks_exact(4).enumerate() {
        let expected = u32::from_le_bytes(bytes.try_into().unwrap());
        assert_eq!(case_fold::canonicalize(cp as u32, false), expected, "legacy U+{cp:X}");
    }
    println!("PASS: runtime legacy Canonicalize matches full uppercase over all 65,536 code units");
}
R
my $property_runtime = abs_path('src/regexp/property.rs');
my $property_verifier = "#[path = \"$runtime\"] mod case_fold;\n" .
    "#[path = \"$property_runtime\"] mod property;\n" .
    "const ALIASES: &[(&str, &str)] = &[\n" .
    join('', map { '    ("' . $_->{aliases} . '", "' . $_->{kind} . "\"),\n" } @properties) .
    "];\n";
write_file("$out/verify-properties.rs", $property_verifier . <<'R');
fn bit(bytes: &[u8], cp: u32) -> bool {
    bytes[cp as usize / 8] & (1 << (cp % 8)) != 0
}
fn main() {
    let bytes = std::fs::read(std::env::args_os().nth(1).unwrap()).unwrap();
    assert_eq!(bytes.len(), ALIASES.len() * (0x110000 / 8));
    for ((aliases, kind), bits) in ALIASES.iter().zip(bytes.chunks_exact(0x110000 / 8)) {
        let keys: &[&str] = match *kind {
            "Binary" => &[""],
            "Category" => &["", "gc", "General_Category"],
            "Script" => &["sc", "Script"],
            "ScriptExtensions" => &["scx", "Script_Extensions"],
            _ => unreachable!(),
        };
        let name = |key: &str, alias: &str| if key.is_empty() { alias.to_owned() } else { format!("{key}={alias}") };
        let p = property::lookup(&name(keys[0], aliases.split(',').next().unwrap())).unwrap();
        for alias in aliases.split(',') {
            for key in ["", "gc", "General_Category", "sc", "Script", "scx", "Script_Extensions"] {
                let lookup = property::lookup(&name(key, alias));
                if keys.contains(&key) {
                    assert_eq!(lookup, Some(p));
                    assert!(matches!(property::syntax_kind(&name(key, alias)), Some(property::PropertyKind::CodePoint)));
                } else {
                    assert_ne!(lookup, Some(p));
                }
            }
        }
        let mut positive = vec![false; 0x110000];
        let mut negative = vec![false; 0x110000];
        for cp in 0..=0x10ffffu32 {
            let yes = bit(bits, cp);
            let ranges = p.ranges();
            let i = ranges.partition_point(|&(_, end)| end < cp);
            assert_eq!(ranges.get(i).is_some_and(|&(start, _)| start <= cp), yes);
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
    println!("PASS: all {} property sets, aliases, complements and iu images match the complete-domain reference", ALIASES.len());
}
R
print "PASS: full-domain QuickJS / Unicode 17.0.0 equality; ", scalar(@fold), " simple mappings\n";

# String methods: independently decode the pinned normative UCD, then compare
# every nonidentity mapping against QuickJS's offline decoder. Hangul is
# algorithmic at runtime; keep its 11,172 syllables out of immutable tables.
my (%lower, %canonical, %compatibility, %ccc, %excluded);
for (split /\n/, read_file("$ucd/UnicodeData.txt")) {
    my @f = split /;/;
    my $cp = hex($f[0]);
    $lower{$cp} = [hex($f[13])] if $f[13];
    $ccc{$cp} = 0 + $f[3] if $f[3];
    if ($f[5]) {
        my $decomp = $f[5];
        my $compat = $decomp =~ s/^<[^>]+> //;
        my $parts = [map { hex } split ' ', $decomp];
        $canonical{$cp} = $parts unless $compat;
        $compatibility{$cp} = $parts;
    }
}
for (split /\n/, read_file("$ucd/SpecialCasing.txt")) {
    s/#.*//;
    if (/^([0-9A-F]+);\s*([0-9A-F ]+);[^;]*;[^;]*;\s*$/) {
        $lower{hex($1)} = [map { hex } split ' ', $2];
    }
}
for (split /\n/, read_file("$ucd/DerivedNormalizationProps.txt")) {
    if (/^([0-9A-F]+)(?:\.\.([0-9A-F]+))?\s*; Full_Composition_Exclusion\s*#/) {
        $excluded{$_} = 1 for hex($1) .. hex($2 // $1);
    }
}
sub expanded {
    my ($cp, $table) = @_;
    if ($cp >= 0xac00 && $cp <= 0xd7a3) {
        my $n = $cp - 0xac00;
        return [0x1100 + int($n / 588), 0x1161 + int($n % 588 / 28),
            ($n % 28 ? 0x11a7 + $n % 28 : ())];
    }
    return [$cp] unless exists $table->{$cp};
    my @result = map { @{expanded($_, $table)} } @{$table->{$cp}};
    # Stable canonical ordering, also across recursive expansion boundaries.
    for my $i (1 .. $#result) {
        my $j = $i;
        while ($j && ($ccc{$result[$j]} // 0) &&
            ($ccc{$result[$j-1]} // 0) > ($ccc{$result[$j]} // 0)) {
            @result[$j-1, $j] = @result[$j, $j-1]; --$j;
        }
    }
    return \@result;
}
for my $map (\%upper, \%lower) {
    for my $cp (keys %$map) {
        delete $map->{$cp} if @{$map->{$cp}} == 1 && $map->{$cp}[0] == $cp;
    }
}
my %nfd = map { $_ => expanded($_, \%canonical) } keys %canonical;
my %nfkd = map { $_ => expanded($_, \%compatibility) } keys %compatibility;
for my $spec ([\%upper, 3], [\%lower, 3], [\%nfd, 18], [\%nfkd, 18]) {
    my ($map, $max) = @$spec;
    for my $cp (keys %$map) {
        my $units = 0; $units += $_ > 0xffff ? 2 : 1 for @{$map->{$cp}};
        die "String expansion exceeds runtime reserve bound\n" if $units > $max * ($cp > 0xffff ? 2 : 1);
    }
}
my %string_maps = (U => \%upper, D => \%lower, N => \%nfd, K => \%nfkd,
    B => {map { $_ => [$ccc{$_}] } keys %ccc});
my %seen;
open my $strings, '-|', "$out/export", 'strings' or die $!;
my $reference = '';
while (<$strings>) {
    $reference .= $_;
    /^([UDNKB]) ([0-9A-F]+)((?: [0-9A-F]+)+)\n$/ or die "string reference output\n";
    my ($kind, $cp, $values) = ($1, hex($2), $3);
    my @values = map { hex } split ' ', $values;
    my $expected = $kind =~ /^[NK]$/ && $cp >= 0xac00 && $cp <= 0xd7a3
        ? expanded($cp, {}) : $string_maps{$kind}{$cp};
    die sprintf("string %s mismatch U+%X\n", $kind, $cp)
        unless $expected && "@values" eq "@$expected";
    $seen{"$kind:$cp"} = 1;
}
close $strings or die "string reference failed\n";
for my $kind (keys %string_maps) {
    die "missing $kind reference\n" if grep { !$seen{"$kind:$_"} } keys %{$string_maps{$kind}};
}
my $string_data = "// Generated from pinned Unicode 17.0.0; do not edit.\n" .
    "// Unicode data license: docs/UNICODE-LICENSE; see string_provenance.txt.\n";
for my $spec (['UPPER', \%upper], ['LOWER', \%lower], ['NFD', \%nfd], ['NFKD', \%nfkd]) {
    my ($name, $map) = @$spec;
    $string_data .= "pub(super) static $name: &[(u32, &[u32])] = &[\n";
    for my $cp (sort { $a <=> $b } keys %$map) {
        $string_data .= sprintf("(0x%X,&[%s]),\n", $cp,
            join(',', map { sprintf '0x%X', $_ } @{$map->{$cp}}));
    }
    $string_data .= "];\n";
}
$string_data .= "pub(super) static CCC: &[(u32, u8)] = &[\n" .
    join('', map { sprintf "(0x%X,%d),\n", $_, $ccc{$_} } sort { $a <=> $b } keys %ccc) . "];\n";
my @composition = sort { $a->[0] <=> $b->[0] || $a->[1] <=> $b->[1] }
    map { [@{$canonical{$_}}, $_] }
    grep { !$excluded{$_} && @{$canonical{$_}} == 2 } keys %canonical;
$string_data .= "pub(super) static COMPOSE: &[((u32, u32), u32)] = &[\n" .
    join('', map { sprintf "((0x%X,0x%X),0x%X),\n", @$_ } @composition) . "];\n";
write_file("$out/string_data.rs", $string_data);
write_file("$out/compose-input.txt", join('', map { sprintf "%X %X\n", @$_[0,1] } @composition));
open my $pairs, '-|', "$out/export", 'compose', "$out/compose-input.txt" or die $!;
my $i = 0;
while (<$pairs>) {
    /^P ([0-9A-F]+) ([0-9A-F]+) ([0-9A-F]+)\n$/ or die "compose reference\n";
    my @row = map { hex } ($1, $2, $3);
    die "composition differs from QuickJS\n" unless "@row" eq "@{$composition[$i++]}";
    $reference .= $_;
}
close $pairs or die "compose reference failed\n";
die "incomplete composition reference\n" unless $i == @composition;
write_file("$out/string-reference.txt", $reference);
write_file("$out/provenance.txt", read_file("$out/provenance.txt") .
    "String full upper/lower, NFD/NFKD, CCC: full-domain C/UCD equality; 961 canonical composition pairs; algorithmic Hangul.\n");
my $string_runtime = abs_path('src/value/string_unicode.rs');
write_file("$out/verify-strings.rs", "#[path = \"$string_runtime\"] mod unicode;\n" . <<'R');
use std::collections::BTreeMap;
fn main() {
    let input = std::fs::read_to_string(std::env::args_os().nth(1).unwrap()).unwrap();
    let mut maps: [BTreeMap<u32, Vec<u32>>; 5] = std::array::from_fn(|_| BTreeMap::new());
    let mut pairs = 0;
    for line in input.lines() {
        let (kind, row) = line.split_once(' ').unwrap();
        let row: Vec<u32> = row.split(' ').map(|s| u32::from_str_radix(s, 16).unwrap()).collect();
        if kind == "P" {
            assert_eq!(unicode::compose(row[0], row[1]), Some(row[2])); pairs += 1;
        } else {
            let k = ["U", "D", "N", "K", "B"].iter().position(|&s| s == kind).unwrap();
            assert!(maps[k].insert(row[0], row[1..].to_vec()).is_none());
        }
    }
    assert_eq!(pairs, 961);
    for cp in 0..=0x10ffff {
        for (kind, map) in maps[..4].iter().enumerate() {
            if kind >= 2 && (0xac00..=0xd7a3).contains(&cp) { continue; }
            let identity = [cp];
            assert_eq!(unicode::mapping(cp, kind).unwrap_or(&identity), map.get(&cp).map_or(identity.as_slice(), Vec::as_slice), "kind {kind} U+{cp:X}");
        }
        assert_eq!(unicode::cc(cp) as u32, maps[4].get(&cp).map_or(0, |v| v[0]), "CCC U+{cp:X}");
    }
    for cp in 0xac00..=0xd7a3 {
        let n = cp-0xac00;
        let lv = unicode::compose(0x1100+n/588, 0x1161+n%588/28).unwrap();
        assert_eq!(if n%28 == 0 { lv } else { unicode::compose(lv, 0x11a7+n%28).unwrap() }, cp);
    }
    assert_eq!(unicode::compose(0x0308, 0x0301), None); // Composition exclusion.
    println!("PASS: installed String readers match full-domain C case, NFD/NFKD, CCC, all 961 composition pairs and Hangul");
}
R
print "PASS: full Unicode 17 string case/decomposition/CCC equality; ", scalar(@composition), " composition pairs\n";
