#!/usr/bin/env perl
use strict;
use warnings;

# This is a deliberately restricted declaration/initializer reader, not a C
# compiler. C evaluates all numbers, escapes, macros, enum values and bitfields.
# Unknown declarations, expressions and field shapes are fatal.
my ($root, $out) = @ARGV;
die "usage: export.pl QUICKJS_SOURCE OUTPUT_DIRECTORY\n" unless defined $out;
my @sources = ('intl/locale-data.c', 'temporal/tzdata_generated.c', 'libunicode-table.h');
sub read_file {
    open my $f, '<', $_[0] or die "$_[0]: $!";
    local $/;
    return <$f>;
}
sub strip_comments {
    my $s = shift;
    $s =~ s{("(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*')|/\*.*?\*/|//[^\n]*}{defined $1 ? $1 : ' '}gse;
    return $s;
}
sub trim { my $s = shift; $s =~ s/^\s+|\s+$//g; return $s; }
sub cq {
    my $s = shift;
    $s =~ s/\\/\\\\/g; $s =~ s/"/\\"/g;
    $s =~ s/\n/\\n/g; $s =~ s/\r/\\r/g; $s =~ s/\t/\\t/g;
    return '"' . $s . '"';
}
sub split_top {
    my ($s, $delimiter) = @_;
    my @parts;
    my $depth = 0;
    # Don't split commas/semicolons inside strings or nested initializers.
    my @tokens = ($s =~ /"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|./gs);
    my $buf = '';
    for my $t (@tokens) {
        $depth++ if $t eq '{' || $t eq '(' || $t eq '[';
        $depth-- if $t eq '}' || $t eq ')' || $t eq ']';
        die "unbalanced initializer\n" if $depth < 0;
        if ($t eq $delimiter && $depth == 0) { push @parts, trim($buf); $buf = ''; }
        else { $buf .= $t; }
    }
    die "unbalanced initializer\n" if $depth;
    push @parts, trim($buf) if length trim($buf);
    return @parts;
}
sub node {
    my $s = trim(shift);
    return [map { node($_) } split_top(substr($s, 1, -1), ',')]
        if $s =~ /^\{.*\}$/s;
    return $s;
}

# Read only the data structs that appear in these three translation units.
my %wanted = map { $_ => 1 } qw(NumberSymbols NumberPatterns CompactDecimalPattern
    LocaleData DisplayNamesData IntlPrefData IntlWeekData IntlScriptDir
    TZTransition TZData TZAlias TiTzRegion);
my (%structs, @struct_order);
for my $header ('intl/intl_private.h', 'temporal/timezone.h') {
    my $s = strip_comments(read_file("$root/$header"));
    while ($s =~ /typedef\s+struct\s*\{([^{}]*)\}\s*(\w+)\s*;/gs) {
        my ($body, $name) = ($1, $2);
        next unless $wanted{$name};
        my @fields;
        for my $field (split_top($body, ';')) {
            $field =~ /^(.*?)\s*\b(\w+)\s*(?::\s*\d+)?$/s or die "field: $field\n";
            my ($type, $fname) = (trim($1), $2);
            $type =~ s/\bconst\b//g; $type =~ s/\s+//g;
            push @fields, [$type, $fname];
        }
        $structs{$name} = \@fields;
        push @struct_order, $name;
    }
}
die "missing data struct\n" if keys(%structs) != keys(%wanted);
my (%decls, @decl_order);
for my $source (@sources) {
    my $s = strip_comments(read_file("$root/$source"));
    # These pinned files use only includes, CURRENCY_* counts and this single
    # CONFIG_ALL_UNICODE conditional. The C compilation enables that branch.
    $s =~ s/^\s*#([^\n]*)$//mg;
    for my $decl (split_top($s, ';')) {
        if ($decl =~ /^typedef\s+enum\s*\{(.*)\}\s*(\w+)$/s) {
            for my $item (split_top($1, ',')) {
                $item =~ /^(\w+)(?:\s*=.*)?$/s or die "enum: $item\n";
                my $name = $1;
                $decls{$name} = { type => 'unsigned', value => $name };
                push @decl_order, $name;
            }
            next;
        }
        $decl =~ /^(?:static\s+)?const\s+(.+?)\s*\b(\w+)\s*(\[[^\]]*\])?\s*=\s*(.*)$/s
            or die "unrecognized declaration: " . substr($decl, 0, 100) . "\n";
        my ($type, $name, $array, $value) = ($1, $2, $3, $4);
        $type =~ s/\bconst\b//g; $type =~ s/\s+//g;
        die "duplicate $name\n" if exists $decls{$name};
        $decls{$name} = { type => $type, array => defined($array), value => node($value) };
        push @decl_order, $name;
    }
}
# Numbering-system data lives beside service code, not in locale-data.c.
# Extract only these pinned declarations; compile their original initializers
# with the same C exporter, never link or execute the Intl implementation.
my $intl = strip_comments(read_file("$root/intl/intl.c"));
my @number_declarations;
for my $name (qw(intl_all_numbering_systems nf_digit_base hanidec_cps)) {
    $intl =~ /((?:static\s+)?const\s+(?:char\s*\*|uint32_t)\s*\b\Q$name\E\s*\[[^\]]*\]\s*=\s*(\{[^{}]*\})\s*;)/s
        or die "missing numbering-system declaration: $name\n";
    my ($declaration, $initializer) = ($1, $2);
    push @number_declarations, $declaration;
    my $values = node($initializer);
    if ($name eq 'nf_digit_base') {
        # Designated indices are evaluated by C; emit_array reads every C slot,
        # including the unspecified zero entries for algorithmic systems.
        for my $item (@$values) {
            $item =~ /^\[NS_[A-Z0-9_]+\]\s*=\s*0x[0-9A-Fa-f]+$/
                or die "unsupported digit-base initializer: $item\n";
        }
        $values = [('0') x scalar(@{$decls{intl_all_numbering_systems}{value}})];
    }
    $decls{$name} = {type => $name eq 'intl_all_numbering_systems' ? 'char*' : 'uint32_t', array => 1, value => $values};
    push @decl_order, $name;
}
my %numbers = (uint8_t=>'u8', uint16_t=>'u16', uint32_t=>'u32',
    int32_t=>'i32', int64_t=>'i64', int=>'i32', unsigned=>'u32');
sub rust_type {
    my $t = shift;
    return $numbers{$t} if exists $numbers{$t};
    return $t if exists $structs{$t};
    return "Option<&'static [u8]>" if $t eq 'char*';
    return "Option<&'static [" . rust_type($1) . "]>" if $t =~ /^(\w+)\*$/;
    die "unsupported type: $t\n";
}
open my $c, '>', "$out/export.c" or die $!;
print $c <<'C';
#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <inttypes.h>
#include "cutils.h"
#define CONFIG_ALL_UNICODE
#include "intl/locale-data.c"
#include "temporal/tzdata_generated.c"
#include "libunicode-table.h"
static int wire;
static void text(const char *s) { if (!wire) fputs(s, stdout); }
static void word(uint64_t x) {
    for (int i = 0; i < 8; i++) { putchar((unsigned char)x); x >>= 8; }
}
static void number_u(uint64_t x) {
    if (wire) word(x); else printf("%" PRIu64, x);
}
static void number_i(int64_t x) {
    if (wire) word((uint64_t)x); else printf("%" PRId64, x);
}
static void bytes(const void *p, size_t n) {
    const unsigned char *s = p;
    if (wire) { word(n); fwrite(s, 1, n, stdout); return; }
    /* Rust byte literals preserve non-ASCII UTF-8 and embedded/trailing NULs. */
    fputs("b\"", stdout);
    for (size_t i = 0; i < n; i++) {
        unsigned c = s[i];
        if (c >= 32 && c <= 126 && c != '"' && c != '\\') putchar(c);
        else printf("\\x%02x", c);
    }
    fputs("\"", stdout);
}
C
print $c join("\n", @number_declarations), "\n";
print $c <<'C';
int main(int argc, char **argv) {
    wire = argc == 2 && strcmp(argv[1], "--wire") == 0;
C
sub text_c { print $c 'text(', cq($_[0]), ");\n"; }
my $rust_header = "// Generated by scripts/quickjs-data/generate.sh; do not edit.\n" .
    "// QuickJS 7d1c13c5d8f8c6b32893786ed35cdcebd88194d2. See provenance.txt.\n" .
    "#![forbid(unsafe_code)]\n#![allow(dead_code, non_upper_case_globals, non_snake_case)]\n";
for my $name (@struct_order) {
    $rust_header .= "#[derive(Debug)]\npub struct $name {\n";
    $rust_header .= '    pub ' . $_->[1] . ': ' . rust_type($_->[0]) . ",\n" for @{$structs{$name}};
    $rust_header .= "}\n";
}
text_c($rust_header);

my $loop = 0;
sub emit_array;
sub emit_value {
    my ($type, $n, $expr) = @_;
    if (exists $numbers{$type}) {
        die "unexpected numeric initializer\n" if ref $n;
        my $signed = $numbers{$type} =~ /^i/;
        print $c ($signed ? 'number_i((int64_t)(' : 'number_u((uint64_t)('), $expr, "));\n";
    } elsif (exists $structs{$type}) {
        my $fields = $structs{$type};
        die "field count $type at $expr\n" unless ref($n) && @$n == @$fields;
        text_c("$type { ");
        for my $i (0 .. $#$fields) {
            my ($ft, $fn) = @{$fields->[$i]};
            text_c("$fn: ");
            emit_value($ft, $n->[$i], "($expr).$fn");
            text_c(', ');
        }
        text_c('}');
    } elsif ($type =~ /\*$/) {
        die "pointer initializer must be scalar\n" if ref $n;
        if ($n eq 'NULL' || $n eq '0') {
            print $c "if ($expr != NULL) abort(); if (wire) putchar(0);\n";
            text_c('None');
            return;
        }
        print $c "if ($expr == NULL) abort(); if (wire) putchar(1);\n";
        text_c('Some(');
        if ($type eq 'char*' && $n =~ /^(?:"(?:\\.|[^"\\])*"\s*)+$/s) {
            # sizeof includes the compiler-added terminator, unlike strlen.
            print $c "bytes($expr, sizeof($n));\n";
        } elsif ($n =~ /^\w+$/ && exists $decls{$n} && $decls{$n}{array}) {
            my $d = $decls{$n};
            text_c($n);
            print $c "if (wire) {\n";
            emit_array($d->{type}, $d->{value}, $expr, "sizeof($n) / sizeof($n\[0])");
            print $c "}\n";
        } else { die "unsupported pointer $type: $n\n"; }
        text_c(')');
    } else { die "unsupported value type $type\n"; }
}
sub emit_array {
    my ($type, $n, $expr, $len) = @_;
    if ($type eq 'char') {
        die "char array needs string initializer\n" if ref $n;
        print $c "bytes($expr, $len);\n";
        return;
    }
    die "array needs braces\n" unless ref $n;
    print $c "if (wire) { bytes($expr, $len); } else {\n" if $type eq 'uint8_t';
    my $count = scalar @$n;
    print $c "if (($len) != $count) abort(); if (wire) word($len);\n";
    text_c('&[');
    if (exists $numbers{$type}) {
        my $idx = 'i' . $loop++;
        print $c "for (size_t $idx = 0; $idx < ($len); $idx++) {\n";
        emit_value($type, '0', "($expr)[$idx]"); text_c(',');
        print $c "}\n";
    } else {
        for my $i (0 .. $#$n) { emit_value($type, $n->[$i], "($expr)[$i]"); text_c(",\n"); }
    }
    text_c(']');
    print $c "}\n" if $type eq 'uint8_t';
}
for my $name (@decl_order) {
    my $d = $decls{$name};
    my $type = $d->{type};
    my $rt = $d->{array} ? "&'static [" . ($type eq 'char' ? 'u8' : rust_type($type)) . ']' : rust_type($type);
    text_c("pub static $name: $rt = ");
    if ($d->{array}) { emit_array($type, $d->{value}, $name, "sizeof($name) / sizeof($name\[0])"); }
    else { emit_value($type, $d->{value}, $name); }
    text_c(";\n");
}
print $c "return ferror(stdout) ? 1 : 0;\n}\n";
close $c or die $!;

# An independently compiled Rust consumer serializes every generated value.
# It reads the Rust statics, not the parsed C initializer tree.
open my $r, '>', "$out/verify.rs" or die $!;
print $r <<'R';
#![forbid(unsafe_code)]
#[path = "tables.rs"]
mod tables;
use tables::*;
use std::io::{self, Write};
trait Wire { fn wire(&self, out: &mut impl Write); }
macro_rules! integer {
    ($($t:ty),*) => { $(impl Wire for $t {
        fn wire(&self, out: &mut impl Write) {
            out.write_all(&(*self as u64).to_le_bytes()).unwrap();
        }
    })* };
}
integer!(u16, u32, u64, i32, i64);
// A byte slice has byte payload; other numeric arrays use 64-bit words.
// Explicit implementations avoid overlapping specialization on &[u8].
impl Wire for &[u8] {
    fn wire(&self, out: &mut impl Write) {
        (self.len() as u64).wire(out); out.write_all(self).unwrap();
    }
}
impl<T: Wire> Wire for Option<T> {
    fn wire(&self, out: &mut impl Write) {
        match self { None => out.write_all(&[0]).unwrap(), Some(x) => {
            out.write_all(&[1]).unwrap(); x.wire(out);
        }}
    }
}
R
# Byte arrays and string pools both use length followed by raw bytes.
my %slice_types = map { rust_type($_) => 1 } qw(uint16_t uint32_t int32_t int64_t int unsigned);
$slice_types{$_} = 1 for @struct_order;
$slice_types{rust_type('char*')} = 1;
$slice_types{rust_type('uint8_t*')} = 1;
for my $t (sort keys %slice_types) {
    print $r "impl Wire for &[$t] { fn wire(&self, out: &mut impl Write) {\n";
    print $r "(self.len() as u64).wire(out); for x in *self { x.wire(out); } } }\n";
}
print $r "impl Wire for u8 { fn wire(&self, out: &mut impl Write) { (*self as u64).wire(out); } }\n";
for my $name (@struct_order) {
    print $r "impl Wire for $name { fn wire(&self, out: &mut impl Write) {\n";
    print $r 'self.', $_->[1], ".wire(out);\n" for @{$structs{$name}};
    print $r "} }\n";
}
print $r "fn main() { let mut out = io::BufWriter::new(io::stdout().lock());\n";
print $r "$_.wire(&mut out);\n" for @decl_order;
print $r "out.flush().unwrap(); }\n";
close $r or die $!;
print scalar(@decl_order), " declarations; ", scalar(@struct_order), " record types\n";
