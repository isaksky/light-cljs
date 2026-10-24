#!/usr/bin/env perl
use strict;
use warnings;
use Digest::SHA qw(sha256_hex);
use File::Path qw(make_path);
use Cwd qw(abs_path);

# Expand the verified snapshot through its pinned offline Unicode decoder.
# No collation/normalization C code is linked into the Rust runtime.
my ($import, $source, $out) = @ARGV;
die "usage: repack-collation.pl VERIFIED_IMPORT QUICKJS_SOURCE NEW_OUTPUT\n"
    unless @ARGV == 3 && !-e $out;
$source = abs_path($source);
sub read_file {
    open my $f, '<', $_[0] or die "$!";
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
    'cutils.c' => 'b73a403a59da30726257ddbdf5e399298941c1def997782ee0d4d33f796a80a2',
    'quickjs.h' => '09d208759171634a5b1e60aa314fd00da5d088ac28621b7ae4682ba2fb103415',
);
for my $name (sort keys %pins) {
    die "unpinned $name\n" unless sha256_hex(read_file("$source/$name")) eq $pins{$name};
}
my $provenance = read_file("$import/provenance.txt");
die "unverified import\n" unless $provenance =~ /$pins{'libunicode-table.h'}  libunicode-table.h/;
my $tables = read_file("$import/tables.rs");
for my $name (qw(unicode_cc_table unicode_decomp_table1 case_conv_table1)) {
    die "missing verified $name\n" unless $tables =~ /pub static $name:/;
}
make_path($out);
write_file("$out/export.c", <<'C');
#include <stdio.h>
#include <stdlib.h>
#include "cutils.h"
#include "libunicode.h"
static void *resize(void *opaque, void *ptr, size_t size) {
    (void)opaque; return realloc(ptr, size);
}
int main(void) {
    for (uint32_t cp = 0; cp <= 0x10ffff; cp++) {
        uint32_t *nfd, folded[3];
        int n = unicode_normalize(&nfd, &cp, 1, UNICODE_NFD, NULL, resize);
        if (n < 1) return 1;
        if (n != 1 || nfd[0] != cp) {
            printf("N %X", cp);
            for (int i = 0; i < n; i++) printf(" %X", nfd[i]);
            puts("");
        }
        free(nfd);
        n = lre_case_conv(folded, cp, 2);
        if (n != 1 || folded[0] != cp) {
            printf("F %X", cp);
            for (int i = 0; i < n; i++) printf(" %X", folded[i]);
            puts("");
        }
        unsigned cc = unicode_get_cc(cp);
        if (cc) printf("C %X %X\n", cp, cc);
    }
    return 0;
}
C
system($ENV{CC} // 'cc', '-std=c11', '-O2', '-ffunction-sections', '-fdata-sections',
    '-Wl,--gc-sections', '-I', $source, "$out/export.c", "$source/libunicode.c", "$source/cutils.c",
    '-o', "$out/export") == 0 or die "compile reference\n";
open my $pipe, '-|', "$out/export" or die $!;
my %rows = (N => [], F => [], C => []);
my $reference = '';
while (<$pipe>) {
    $reference .= $_;
    /^([NFC]) ([0-9A-F]+)((?: [0-9A-F]+)+)\n$/ or die "reference record\n";
    my ($kind, $cp, $rest) = ($1, hex($2), $3);
    my @values = map { hex($_) } split ' ', $rest;
    die "invalid scalar\n" if grep { $_ > 0x10ffff } ($cp, @values);
    die "canonical expansion exceeds reserved bound\n" if $kind eq 'N' && @values > 4;
    die "full folding exceeds reserved bound\n" if $kind eq 'F' && @values > 3;
    my $value = $kind eq 'C' ? $values[0] : '&[' . join(',', map { sprintf('0x%X', $_) } @values) . ']';
    push @{$rows{$kind}}, sprintf("(0x%X,%s),\n", $cp, $value);
}
close $pipe or die "reference failed\n";
write_file("$out/reference.txt", $reference);
my $rust = "// Generated from QuickJS 7d1c13c5d8f8 Unicode snapshot. Do not edit.\n" .
    "// Copyright (c) 2017-2018 Fabrice Bellard. MIT: QUICKJS-LICENSE.\n" .
    "// Unicode data: docs/UNICODE-LICENSE. Regenerate: scripts/quickjs-data/README.md.\n" .
    "#![forbid(unsafe_code)]\n";
for my $kind (qw(N F C)) {
    my $name = {N => 'NFD', F => 'FOLD', C => 'CCC'}->{$kind};
    my $ty = $kind eq 'C' ? 'u8' : '&[u32]';
    $rust .= "#[rustfmt::skip]\npub(crate) static $name: &[(u32, $ty)] = &[\n" . join('', @{$rows{$kind}}) . "];\n";
}
write_file("$out/collation.rs", $rust);
# Independently read every emitted value and compare its logical stream with C.
write_file("$out/verify.rs", <<'RUST');
#[path = "collation.rs"] mod data;
fn main() {
    let mut output = String::new();
    for cp in 0..=0x10ffff {
        for (kind, table) in [('N', data::NFD), ('F', data::FOLD)] {
            if let Ok(i) = table.binary_search_by_key(&cp, |&(c, _)| c) {
                output += &format!("{kind} {cp:X}");
                for v in table[i].1 { output += &format!(" {v:X}"); }
                output.push('\n');
            }
        }
        if let Ok(i) = data::CCC.binary_search_by_key(&cp, |&(c, _)| c) {
            output += &format!("C {cp:X} {:X}\n", data::CCC[i].1);
        }
    }
    assert_eq!(output, std::fs::read_to_string(std::env::args().nth(1).unwrap()).unwrap());
}
RUST
write_file("$out/provenance.txt", $provenance .
    "Collation: NFD, full case folding and canonical combining classes; pinned offline libunicode decoder.\n" .
    join('', map { "$pins{$_}  $_\n" } sort keys %pins) .
    sha256_hex(read_file($0)) . "  repack-collation.pl\n");
print "PASS: expanded NFD, full folding and combining classes\n";
