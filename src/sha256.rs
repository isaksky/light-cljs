//! SHA-256 (FIPS 180-4), in tree because the crate takes no hashing
//! dependency (docs/compiler-cache.md, "Settled decisions"). Namespace
//! artifacts use it for their integrity hashes, and the cache keys
//! ([`crate::cache_key`]) for their [`Fingerprint`]s. `build.rs` includes
//! this file too, for the compiler's identity. The tests check it against
//! NIST's CAVP short message, long message and Monte Carlo vectors
//! (`tests/fixtures/sha256`).

/// A streaming SHA-256 computation.
#[derive(Clone)]
pub struct Sha256 {
    state: [u32; 8],
    block: [u8; 64],
    filled: usize,
    length: u64,
}

const INITIAL: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

const ROUND: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha256 {
    pub fn new() -> Self {
        Self {
            state: INITIAL,
            block: [0; 64],
            filled: 0,
            length: 0,
        }
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.length = self.length.wrapping_add(data.len() as u64);
        if self.filled > 0 {
            let take = (64 - self.filled).min(data.len());
            self.block[self.filled..self.filled + take].copy_from_slice(&data[..take]);
            self.filled += take;
            data = &data[take..];
            if self.filled < 64 {
                return;
            }
            let block = self.block;
            compress(&mut self.state, &block);
            self.filled = 0;
        }
        let (blocks, rest) = data.as_chunks::<64>();
        for block in blocks {
            compress(&mut self.state, block);
        }
        self.block[..rest.len()].copy_from_slice(rest);
        self.filled = rest.len();
    }

    pub fn finish(mut self) -> [u8; 32] {
        let bits = self.length.wrapping_mul(8);
        self.block[self.filled] = 0x80;
        self.filled += 1;
        if self.filled > 56 {
            self.block[self.filled..64].fill(0);
            let block = self.block;
            compress(&mut self.state, &block);
            self.filled = 0;
        }
        self.block[self.filled..56].fill(0);
        self.block[56..64].copy_from_slice(&bits.to_be_bytes());
        let block = self.block;
        compress(&mut self.state, &block);
        let mut digest = [0; 32];
        for (out, word) in digest.as_chunks_mut::<4>().0.iter_mut().zip(self.state) {
            *out = word.to_be_bytes();
        }
        digest
    }
}

/// The SHA-256 digest of `data`.
pub fn digest(data: &[u8]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(data);
    hash.finish()
}

/// `digest` as lowercase hexadecimal.
pub fn hex(digest: &[u8; 32]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = [0u8; 64];
    for (at, &byte) in digest.iter().enumerate() {
        out[at * 2] = DIGITS[(byte >> 4) as usize];
        out[at * 2 + 1] = DIGITS[(byte & 0x0f) as usize];
    }
    String::from_utf8(out.to_vec()).expect("hex digits are ASCII")
}

/// Framed SHA-256: the fingerprint of a sequence of typed fields. Each
/// field is written as a kind byte, its length as a u64 little-endian, then
/// its bytes, so two different sequences never hash the same bytes:
/// `("ab", "c")` differs from `("a", "bc")`, text from the same bytes as a
/// digest, and a list is framed by its count. A fingerprint begins with its
/// domain, a name that keeps fingerprints of different things apart.
#[derive(Clone)]
pub struct Fingerprint {
    hash: Sha256,
}

/// The kind byte of a framed field.
#[derive(Clone, Copy)]
#[repr(u8)]
enum Field {
    Domain = 1,
    Bytes,
    Text,
    Number,
    Digest,
    Count,
    Absent,
    Present,
}

impl Fingerprint {
    pub fn new(domain: &str) -> Self {
        let mut fingerprint = Self {
            hash: Sha256::new(),
        };
        fingerprint.field(Field::Domain, domain.as_bytes());
        fingerprint
    }

    fn field(&mut self, kind: Field, bytes: &[u8]) -> &mut Self {
        let mut header = [0u8; 9];
        header[0] = kind as u8;
        header[1..9].copy_from_slice(&(bytes.len() as u64).to_le_bytes());
        self.hash.update(&header);
        self.hash.update(bytes);
        self
    }

    pub fn bytes(&mut self, bytes: &[u8]) -> &mut Self {
        self.field(Field::Bytes, bytes)
    }

    pub fn text(&mut self, text: &str) -> &mut Self {
        self.field(Field::Text, text.as_bytes())
    }

    /// Equivalent to [`Self::text`] on the concatenation of `pieces` (whose
    /// total byte length must equal `total_len`), without allocating the
    /// concatenated buffer.
    pub fn text_pieces<'a>(
        &mut self,
        pieces: impl IntoIterator<Item = &'a str>,
        total_len: usize,
    ) -> &mut Self {
        self.hash.update(&[Field::Text as u8]);
        self.hash.update(&(total_len as u64).to_le_bytes());
        let mut written = 0usize;
        for piece in pieces {
            written += piece.len();
            self.hash.update(piece.as_bytes());
        }
        debug_assert_eq!(written, total_len, "text_pieces length mismatch");
        self
    }

    pub fn number(&mut self, number: u64) -> &mut Self {
        self.field(Field::Number, &number.to_le_bytes())
    }

    pub fn flag(&mut self, flag: bool) -> &mut Self {
        self.number(u64::from(flag))
    }

    pub fn digest(&mut self, digest: &[u8; 32]) -> &mut Self {
        self.field(Field::Digest, digest)
    }

    /// The length of a list whose items follow.
    pub fn count(&mut self, count: usize) -> &mut Self {
        self.field(Field::Count, &(count as u64).to_le_bytes())
    }

    /// No value where an optional one goes.
    pub fn absent(&mut self) -> &mut Self {
        self.field(Field::Absent, &[])
    }

    /// The mark before an optional value that is there.
    pub fn present(&mut self) -> &mut Self {
        self.field(Field::Present, &[])
    }

    pub fn optional_text(&mut self, text: Option<&str>) -> &mut Self {
        match text {
            Some(text) => self.present().text(text),
            None => self.absent(),
        }
    }

    pub fn finish(self) -> [u8; 32] {
        self.hash.finish()
    }
}

#[inline(always)]
fn compress(state: &mut [u32; 8], block: &[u8; 64]) {
    let mut w = [
        u32::from_be_bytes([block[0], block[1], block[2], block[3]]),
        u32::from_be_bytes([block[4], block[5], block[6], block[7]]),
        u32::from_be_bytes([block[8], block[9], block[10], block[11]]),
        u32::from_be_bytes([block[12], block[13], block[14], block[15]]),
        u32::from_be_bytes([block[16], block[17], block[18], block[19]]),
        u32::from_be_bytes([block[20], block[21], block[22], block[23]]),
        u32::from_be_bytes([block[24], block[25], block[26], block[27]]),
        u32::from_be_bytes([block[28], block[29], block[30], block[31]]),
        u32::from_be_bytes([block[32], block[33], block[34], block[35]]),
        u32::from_be_bytes([block[36], block[37], block[38], block[39]]),
        u32::from_be_bytes([block[40], block[41], block[42], block[43]]),
        u32::from_be_bytes([block[44], block[45], block[46], block[47]]),
        u32::from_be_bytes([block[48], block[49], block[50], block[51]]),
        u32::from_be_bytes([block[52], block[53], block[54], block[55]]),
        u32::from_be_bytes([block[56], block[57], block[58], block[59]]),
        u32::from_be_bytes([block[60], block[61], block[62], block[63]]),
    ];

    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;

    macro_rules! round {
        ($a:ident, $b:ident, $c:ident, $d:ident, $e:ident, $f:ident, $g:ident, $h:ident, $k:expr, $word:expr) => {{
            let s1 = $e.rotate_right(6) ^ $e.rotate_right(11) ^ $e.rotate_right(25);
            let ch = $g ^ ($e & ($f ^ $g));
            let t1 = $h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add($k)
                .wrapping_add($word);
            let s0 = $a.rotate_right(2) ^ $a.rotate_right(13) ^ $a.rotate_right(22);
            let maj = ($a & $b) | ($c & ($a | $b));
            $d = $d.wrapping_add(t1);
            $h = t1.wrapping_add(s0.wrapping_add(maj));
        }};
    }

    macro_rules! extend {
        ($idx:expr) => {{
            let w15 = w[($idx + 1) & 15];
            let s0 = w15.rotate_right(7) ^ w15.rotate_right(18) ^ (w15 >> 3);
            let w2 = w[($idx + 14) & 15];
            let s1 = w2.rotate_right(17) ^ w2.rotate_right(19) ^ (w2 >> 10);
            let next = w[$idx]
                .wrapping_add(s0)
                .wrapping_add(w[($idx + 9) & 15])
                .wrapping_add(s1);
            w[$idx] = next;
            next
        }};
    }

    // Rounds 0..16: consume the initial 16 big-endian words directly.
    round!(a, b, c, d, e, f, g, h, ROUND[0], w[0]);
    round!(h, a, b, c, d, e, f, g, ROUND[1], w[1]);
    round!(g, h, a, b, c, d, e, f, ROUND[2], w[2]);
    round!(f, g, h, a, b, c, d, e, ROUND[3], w[3]);
    round!(e, f, g, h, a, b, c, d, ROUND[4], w[4]);
    round!(d, e, f, g, h, a, b, c, ROUND[5], w[5]);
    round!(c, d, e, f, g, h, a, b, ROUND[6], w[6]);
    round!(b, c, d, e, f, g, h, a, ROUND[7], w[7]);

    round!(a, b, c, d, e, f, g, h, ROUND[8], w[8]);
    round!(h, a, b, c, d, e, f, g, ROUND[9], w[9]);
    round!(g, h, a, b, c, d, e, f, ROUND[10], w[10]);
    round!(f, g, h, a, b, c, d, e, ROUND[11], w[11]);
    round!(e, f, g, h, a, b, c, d, ROUND[12], w[12]);
    round!(d, e, f, g, h, a, b, c, ROUND[13], w[13]);
    round!(c, d, e, f, g, h, a, b, ROUND[14], w[14]);
    round!(b, c, d, e, f, g, h, a, ROUND[15], w[15]);

    // Rounds 16..64: rolling 16-word message schedule in 8-round unrolled groups.
    for base in [16usize, 32, 48] {
        let w0 = extend!(0);
        round!(a, b, c, d, e, f, g, h, ROUND[base], w0);
        let w1 = extend!(1);
        round!(h, a, b, c, d, e, f, g, ROUND[base + 1], w1);
        let w2 = extend!(2);
        round!(g, h, a, b, c, d, e, f, ROUND[base + 2], w2);
        let w3 = extend!(3);
        round!(f, g, h, a, b, c, d, e, ROUND[base + 3], w3);
        let w4 = extend!(4);
        round!(e, f, g, h, a, b, c, d, ROUND[base + 4], w4);
        let w5 = extend!(5);
        round!(d, e, f, g, h, a, b, c, ROUND[base + 5], w5);
        let w6 = extend!(6);
        round!(c, d, e, f, g, h, a, b, ROUND[base + 6], w6);
        let w7 = extend!(7);
        round!(b, c, d, e, f, g, h, a, ROUND[base + 7], w7);

        let w8 = extend!(8);
        round!(a, b, c, d, e, f, g, h, ROUND[base + 8], w8);
        let w9 = extend!(9);
        round!(h, a, b, c, d, e, f, g, ROUND[base + 9], w9);
        let w10 = extend!(10);
        round!(g, h, a, b, c, d, e, f, ROUND[base + 10], w10);
        let w11 = extend!(11);
        round!(f, g, h, a, b, c, d, e, ROUND[base + 11], w11);
        let w12 = extend!(12);
        round!(e, f, g, h, a, b, c, d, ROUND[base + 12], w12);
        let w13 = extend!(13);
        round!(d, e, f, g, h, a, b, c, ROUND[base + 13], w13);
        let w14 = extend!(14);
        round!(c, d, e, f, g, h, a, b, ROUND[base + 14], w14);
        let w15 = extend!(15);
        round!(b, c, d, e, f, g, h, a, ROUND[base + 15], w15);
    }

    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
    state[5] = state[5].wrapping_add(f);
    state[6] = state[6].wrapping_add(g);
    state[7] = state[7].wrapping_add(h);
}

#[cfg(test)]
mod tests {
    use super::*;

    // FIPS 180-4 examples (NIST CSRC "SHA256.pdf" and "SHA2_Additional.pdf").
    #[test]
    fn nist_vectors() {
        let cases: [(&[u8], &str); 4] = [
            (
                b"",
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            ),
            (
                b"abc",
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            ),
            (
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
                "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
            ),
            (
                b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu",
                "cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1",
            ),
        ];
        for (message, expected) in cases {
            assert_eq!(hex(&digest(message)), expected);
        }
        let million = vec![b'a'; 1_000_000];
        assert_eq!(
            hex(&digest(&million)),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    fn unhex(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&text[at..at + 2], 16).unwrap())
            .collect()
    }

    /// The `(Len, Msg, MD)` records of a CAVP `.rsp` file, with each
    /// message cut to its length in bits (the empty message is `Msg = 00`).
    fn records(file: &str) -> Vec<(Vec<u8>, String)> {
        let mut records = Vec::new();
        let mut length = None;
        let mut message = None;
        for line in file.lines() {
            let Some((key, value)) = line.split_once(" = ") else {
                continue;
            };
            match key {
                "Len" => length = Some(value.parse::<usize>().unwrap()),
                "Msg" => message = Some(unhex(value)),
                "MD" => {
                    let bits = length.take().unwrap();
                    assert_eq!(bits % 8, 0, "byte-oriented vectors only");
                    let mut bytes = message.take().unwrap();
                    bytes.truncate(bits / 8);
                    records.push((bytes, value.to_owned()));
                }
                _ => {}
            }
        }
        records
    }

    #[test]
    fn cavp_short_and_long_message_vectors() {
        let short = records(include_str!("../tests/fixtures/sha256/SHA256ShortMsg.rsp"));
        let long = records(include_str!("../tests/fixtures/sha256/SHA256LongMsg.rsp"));
        assert_eq!((short.len(), long.len()), (65, 64));
        for (message, expected) in short.iter().chain(&long) {
            assert_eq!(&hex(&digest(message)), expected, "{} bytes", message.len());
            // The same message streamed in uneven pieces.
            let mut hash = Sha256::new();
            for piece in message.chunks(37) {
                hash.update(piece);
            }
            assert_eq!(&hex(&hash.finish()), expected);
        }
    }

    // The CAVP Monte Carlo test for SHA-2: from the seed, each checkpoint is
    // the last of 1000 digests of the three digests before it.
    #[test]
    fn cavp_monte_carlo_vectors() {
        let file = include_str!("../tests/fixtures/sha256/SHA256Monte.rsp");
        let mut seed = None;
        let mut checkpoints = Vec::new();
        for line in file.lines() {
            match line.split_once(" = ") {
                Some(("Seed", value)) => seed = Some(unhex(value)),
                Some(("MD", value)) => checkpoints.push(value.to_owned()),
                _ => {}
            }
        }
        assert_eq!(checkpoints.len(), 100);
        let mut seed: [u8; 32] = seed.unwrap().try_into().unwrap();
        for expected in checkpoints {
            let mut window = [seed, seed, seed];
            for _ in 0..1000 {
                let mut hash = Sha256::new();
                for md in &window {
                    hash.update(md);
                }
                window = [window[1], window[2], hash.finish()];
            }
            seed = window[2];
            assert_eq!(hex(&seed), expected);
        }
    }

    #[test]
    fn framed_fields_do_not_alias() {
        let fingerprint = |build: &dyn Fn(&mut Fingerprint)| {
            let mut fingerprint = Fingerprint::new("test");
            build(&mut fingerprint);
            fingerprint.finish()
        };
        let split = fingerprint(&|f| {
            f.text("ab").text("c");
        });
        assert_ne!(
            split,
            fingerprint(&|f| {
                f.text("a").text("bc");
            })
        );
        assert_ne!(
            split,
            fingerprint(&|f| {
                f.text("abc");
            })
        );
        // The kind of a field is part of it.
        assert_ne!(
            fingerprint(&|f| {
                f.text("ab");
            }),
            fingerprint(&|f| {
                f.bytes(b"ab");
            })
        );
        assert_ne!(
            fingerprint(&|f| {
                f.optional_text(None).text("x");
            }),
            fingerprint(&|f| {
                f.optional_text(Some("x"));
            })
        );
        // Counts frame lists: [[a] []] is not [[] [a]].
        assert_ne!(
            fingerprint(&|f| {
                f.count(1).text("a").count(0);
            }),
            fingerprint(&|f| {
                f.count(0).count(1).text("a");
            })
        );
        // The domain separates otherwise equal fingerprints.
        let mut other = Fingerprint::new("other");
        other.text("ab").text("c");
        assert_ne!(split, other.finish());
        // And the framing is fixed: this is the digest of its bytes.
        let mut bytes = vec![1];
        bytes.extend_from_slice(&4u64.to_le_bytes());
        bytes.extend_from_slice(b"test");
        bytes.push(3);
        bytes.extend_from_slice(&2u64.to_le_bytes());
        bytes.extend_from_slice(b"ab");
        bytes.push(3);
        bytes.extend_from_slice(&1u64.to_le_bytes());
        bytes.extend_from_slice(b"c");
        assert_eq!(split, digest(&bytes));
    }

    #[test]
    fn streaming_matches_one_shot_at_every_split() {
        let message: Vec<u8> = (0..200u32).map(|i| (i * 7 + 3) as u8).collect();
        let whole = digest(&message);
        for split in 0..message.len() {
            let mut hash = Sha256::new();
            hash.update(&message[..split]);
            hash.update(&message[split..]);
            assert_eq!(hash.finish(), whole, "split at {split}");
        }
    }

    #[test]
    fn text_pieces_matches_text_across_chunk_boundaries() {
        let pieces = [
            "",
            "alpha ",
            &"x".repeat(70),
            " beta ",
            &"y".repeat(129),
            "",
        ];
        let joined: String = pieces.concat();
        let mut whole = Fingerprint::new("pieces-test");
        whole.text(&joined);
        let mut streamed = Fingerprint::new("pieces-test");
        streamed.text_pieces(pieces, joined.len());
        assert_eq!(streamed.finish(), whole.finish());
    }

    /// Textbook FIPS 180-4 SHA-256 reference for verifying the optimized
    /// rolling-schedule implementation across every length and boundary.
    fn reference_digest(data: &[u8]) -> [u8; 32] {
        let mut state = INITIAL;
        let mut padded = data.to_vec();
        let bits = (data.len() as u64).wrapping_mul(8);
        padded.push(0x80);
        while padded.len() % 64 != 56 {
            padded.push(0);
        }
        padded.extend_from_slice(&bits.to_be_bytes());
        for chunk in padded.chunks_exact(64) {
            let mut w = [0u32; 64];
            for (at, bytes) in chunk.chunks_exact(4).enumerate() {
                w[at] = u32::from_be_bytes(bytes.try_into().unwrap());
            }
            for i in 16..64 {
                let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
                let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
                w[i] = w[i - 16]
                    .wrapping_add(s0)
                    .wrapping_add(w[i - 7])
                    .wrapping_add(s1);
            }
            let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
            for i in 0..64 {
                let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
                let ch = (e & f) ^ (!e & g);
                let t1 = h
                    .wrapping_add(s1)
                    .wrapping_add(ch)
                    .wrapping_add(ROUND[i])
                    .wrapping_add(w[i]);
                let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
                let maj = (a & b) ^ (a & c) ^ (b & c);
                let t2 = s0.wrapping_add(maj);
                h = g;
                g = f;
                f = e;
                e = d.wrapping_add(t1);
                d = c;
                c = b;
                b = a;
                a = t1.wrapping_add(t2);
            }
            for (word, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
                *word = word.wrapping_add(value);
            }
        }
        let mut out = [0u8; 32];
        for (slot, word) in out.chunks_exact_mut(4).zip(state) {
            slot.copy_from_slice(&word.to_be_bytes());
        }
        out
    }

    #[test]
    fn compress_matches_reference_for_all_lengths_0_to_4096() {
        let mut buf = Vec::with_capacity(4096);
        let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
        for _ in 0..4096 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            buf.push((x >> 56) as u8);
        }
        for len in 0..=4096 {
            let slice = &buf[..len];
            let expected = reference_digest(slice);
            assert_eq!(digest(slice), expected, "one-shot mismatch at len {len}");
        }
        for &len in &[
            0usize, 1, 55, 56, 57, 63, 64, 65, 119, 120, 127, 128, 129, 1024, 4096,
        ] {
            let slice = &buf[..len];
            let expected = reference_digest(slice);
            for &chunk_size in &[1usize, 7, 31, 55, 56, 63, 64, 65, 128] {
                let mut hasher = Sha256::new();
                for piece in slice.chunks(chunk_size) {
                    hasher.update(piece);
                }
                assert_eq!(
                    hasher.finish(),
                    expected,
                    "chunked mismatch at len {len}, chunk {chunk_size}"
                );
            }
        }
    }
}
