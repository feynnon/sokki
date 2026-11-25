//! The phone keyboard's two tables, read as it reads them (Typos.kt `Bloom`).
//!
//! words.bin (tools/gen_known_words.py): every word of wordfreq's large German and
//! English lists, as Bloom-filter keys "<b>:<word>" (b = 2 x zipf, 1..16), "d:"/"e:"
//! for words common in German/English, "w:" for possible fixes, "C:" for words German
//! writes capitalised. bigrams.bin (tools/gen_bigrams.py): "<b>:<prev> <word>".
//! SPDX-License-Identifier: GPL-3.0-only

pub const MAX_BUCKET: usize = 16;
const HEADER: usize = 16;
const OFFSET: u64 = 0xCBF2_9CE4_8422_2325;
const PRIME: u64 = 0x0000_0100_0000_01B3;

/// FNV-1a 64, continued from `state` over `bytes`.
pub fn hash(state: u64, bytes: &[u8]) -> u64 {
    let mut h = state;
    for &b in bytes {
        h = (h ^ b as u64).wrapping_mul(PRIME);
    }
    h
}

/// "SKW1", k (i32 BE), m (i64 BE), then m bits.
pub struct Bloom {
    k: u64,
    m: u64,
    data: Vec<u8>,
    /// Hash states after "<b>:" for b = 0..=MAX_BUCKET.
    pub prefix: [u64; MAX_BUCKET + 1],
    /// Hash state after "w:".
    pub candidate_state: u64,
}

impl Bloom {
    pub fn load(data: Vec<u8>) -> Result<Bloom, String> {
        if data.len() < HEADER || &data[..4] != b"SKW1" {
            return Err("not a word table".into());
        }
        let k = i32::from_be_bytes(data[4..8].try_into().unwrap()) as u64;
        let m = i64::from_be_bytes(data[8..16].try_into().unwrap()) as u64;
        if m == 0 || ((data.len() - HEADER) as u64) < m / 8 {
            return Err("word table cut short".into());
        }
        let mut prefix = [0u64; MAX_BUCKET + 1];
        for (b, p) in prefix.iter_mut().enumerate() {
            *p = hash(OFFSET, format!("{b}:").as_bytes());
        }
        Ok(Bloom {
            k,
            m,
            data,
            prefix,
            candidate_state: hash(OFFSET, b"w:"),
        })
    }

    /// `h1`: the FNV-1a hash of the key.
    pub fn contains_hash(&self, h1: u64) -> bool {
        let mut z = h1.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        let h2 = (z ^ (z >> 31)) | 1;
        for i in 0..self.k {
            let idx = h1.wrapping_add(i.wrapping_mul(h2)) % self.m;
            if self.data[HEADER + (idx >> 3) as usize] & (1 << (idx & 7)) == 0 {
                return false;
            }
        }
        true
    }

    pub fn contains(&self, key: &str) -> bool {
        self.contains_hash(hash(OFFSET, key.as_bytes()))
    }
}

/// words.bin with its lookups.
pub struct Table(pub Bloom);

impl Table {
    /// Zipf of a folded word in half steps (bucket middle); 0 if unlisted.
    pub fn zipf(&self, w: &str) -> f64 {
        let bytes = w.as_bytes();
        for b in (1..=MAX_BUCKET).rev() {
            if self.0.contains_hash(hash(self.0.prefix[b], bytes)) {
                return b as f64 / 2.0 + 0.25;
            }
        }
        0.0
    }

    /// The word is common (zipf 3+) in German ('d') or English ('e').
    pub fn common_in(&self, lang: char, w: &str) -> bool {
        self.0.contains(&format!("{lang}:{w}"))
    }

    /// German text writes this folded word with a capital.
    pub fn noun(&self, w: &str) -> bool {
        self.0.contains(&format!("C:{w}"))
    }

    /// At least zipf 2: the word may replace a typed one.
    pub fn candidate(&self, w: &str) -> bool {
        self.0
            .contains_hash(hash(self.0.candidate_state, w.as_bytes()))
    }
}
