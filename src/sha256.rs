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
        self.update(&[0x80]);
        while self.filled != 56 {
            self.update(&[0]);
        }
        self.update(&bits.to_be_bytes());
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
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
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
        self.hash.update(&[kind as u8]);
        self.hash.update(&(bytes.len() as u64).to_le_bytes());
        self.hash.update(bytes);
        self
    }

    pub fn bytes(&mut self, bytes: &[u8]) -> &mut Self {
        self.field(Field::Bytes, bytes)
    }

    pub fn text(&mut self, text: &str) -> &mut Self {
        self.field(Field::Text, text.as_bytes())
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

fn compress(state: &mut [u32; 8], block: &[u8; 64]) {
    let mut w = [0u32; 64];
    for (word, bytes) in w.iter_mut().zip(block.as_chunks::<4>().0) {
        *word = u32::from_be_bytes(*bytes);
    }
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
    for i in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let choose = (e & f) ^ (!e & g);
        let t1 = h
            .wrapping_add(s1)
            .wrapping_add(choose)
            .wrapping_add(ROUND[i])
            .wrapping_add(w[i]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let majority = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(majority);
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
}
