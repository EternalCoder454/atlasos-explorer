//! Checksums of a file for the Properties window: SHA-256 and the others,
//! read in one pass that can be stopped, and the comparison with a checksum
//! someone pasted. No Qt; the hash functions are written out here (they are
//! tested against the published vectors) so the core needs no crate for
//! them. MD5 and SHA-1 are there to check downloads that only publish those;
//! they are not for anything that needs to resist an attacker.

use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Alg {
    Md5,
    Sha1,
    Sha256,
    Sha512,
}

impl Alg {
    pub const ALL: [Alg; 4] = [Alg::Sha256, Alg::Sha1, Alg::Md5, Alg::Sha512];

    pub fn name(self) -> &'static str {
        match self {
            Alg::Md5 => "MD5",
            Alg::Sha1 => "SHA-1",
            Alg::Sha256 => "SHA-256",
            Alg::Sha512 => "SHA-512",
        }
    }

    /// Hex digits in a checksum of this kind.
    pub fn hex_len(self) -> usize {
        match self {
            Alg::Md5 => 32,
            Alg::Sha1 => 40,
            Alg::Sha256 => 64,
            Alg::Sha512 => 128,
        }
    }

    pub fn from_index(i: u32) -> Option<Alg> {
        Alg::ALL.get(i as usize).copied()
    }

    pub fn index(self) -> u32 {
        Alg::ALL.iter().position(|a| *a == self).unwrap_or(0) as u32
    }

    fn of_hex_len(n: usize) -> Option<Alg> {
        Alg::ALL.into_iter().find(|a| a.hex_len() == n)
    }
}

// ---- The hash functions ----

enum State {
    Md5([u32; 4]),
    Sha1([u32; 5]),
    Sha256([u32; 8]),
    Sha512([u64; 8]),
}

/// A hash being computed.
pub struct Hasher {
    alg: Alg,
    state: State,
    buf: [u8; 128],
    buffered: usize,
    total: u128,
}

const MD5_S: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9,
    14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15,
    21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];

fn md5_k(i: usize) -> u32 {
    // floor(2^32 * abs(sin(i + 1)))
    ((i as f64 + 1.0).sin().abs() * 4_294_967_296.0) as u32
}

const K256: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

const K512: [u64; 80] = [
    0x428a2f98d728ae22,
    0x7137449123ef65cd,
    0xb5c0fbcfec4d3b2f,
    0xe9b5dba58189dbbc,
    0x3956c25bf348b538,
    0x59f111f1b605d019,
    0x923f82a4af194f9b,
    0xab1c5ed5da6d8118,
    0xd807aa98a3030242,
    0x12835b0145706fbe,
    0x243185be4ee4b28c,
    0x550c7dc3d5ffb4e2,
    0x72be5d74f27b896f,
    0x80deb1fe3b1696b1,
    0x9bdc06a725c71235,
    0xc19bf174cf692694,
    0xe49b69c19ef14ad2,
    0xefbe4786384f25e3,
    0x0fc19dc68b8cd5b5,
    0x240ca1cc77ac9c65,
    0x2de92c6f592b0275,
    0x4a7484aa6ea6e483,
    0x5cb0a9dcbd41fbd4,
    0x76f988da831153b5,
    0x983e5152ee66dfab,
    0xa831c66d2db43210,
    0xb00327c898fb213f,
    0xbf597fc7beef0ee4,
    0xc6e00bf33da88fc2,
    0xd5a79147930aa725,
    0x06ca6351e003826f,
    0x142929670a0e6e70,
    0x27b70a8546d22ffc,
    0x2e1b21385c26c926,
    0x4d2c6dfc5ac42aed,
    0x53380d139d95b3df,
    0x650a73548baf63de,
    0x766a0abb3c77b2a8,
    0x81c2c92e47edaee6,
    0x92722c851482353b,
    0xa2bfe8a14cf10364,
    0xa81a664bbc423001,
    0xc24b8b70d0f89791,
    0xc76c51a30654be30,
    0xd192e819d6ef5218,
    0xd69906245565a910,
    0xf40e35855771202a,
    0x106aa07032bbd1b8,
    0x19a4c116b8d2d0c8,
    0x1e376c085141ab53,
    0x2748774cdf8eeb99,
    0x34b0bcb5e19b48a8,
    0x391c0cb3c5c95a63,
    0x4ed8aa4ae3418acb,
    0x5b9cca4f7763e373,
    0x682e6ff3d6b2b8a3,
    0x748f82ee5defb2fc,
    0x78a5636f43172f60,
    0x84c87814a1f0ab72,
    0x8cc702081a6439ec,
    0x90befffa23631e28,
    0xa4506cebde82bde9,
    0xbef9a3f7b2c67915,
    0xc67178f2e372532b,
    0xca273eceea26619c,
    0xd186b8c721c0c207,
    0xeada7dd6cde0eb1e,
    0xf57d4f7fee6ed178,
    0x06f067aa72176fba,
    0x0a637dc5a2c898a6,
    0x113f9804bef90dae,
    0x1b710b35131c471b,
    0x28db77f523047d84,
    0x32caab7b40c72493,
    0x3c9ebe0a15c9bebc,
    0x431d67c49c100d4c,
    0x4cc5d4becb3e42b6,
    0x597f299cfc657e2a,
    0x5fcb6fab3ad6faec,
    0x6c44198c4a475817,
];

impl Hasher {
    pub fn new(alg: Alg) -> Hasher {
        let state = match alg {
            Alg::Md5 => State::Md5([0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476]),
            Alg::Sha1 => State::Sha1([0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476, 0xc3d2e1f0]),
            Alg::Sha256 => State::Sha256([
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ]),
            Alg::Sha512 => State::Sha512([
                0x6a09e667f3bcc908,
                0xbb67ae8584caa73b,
                0x3c6ef372fe94f82b,
                0xa54ff53a5f1d36f1,
                0x510e527fade682d1,
                0x9b05688c2b3e6c1f,
                0x1f83d9abfb41bd6b,
                0x5be0cd19137e2179,
            ]),
        };
        Hasher {
            alg,
            state,
            buf: [0; 128],
            buffered: 0,
            total: 0,
        }
    }

    fn block_len(&self) -> usize {
        if self.alg == Alg::Sha512 { 128 } else { 64 }
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.total += data.len() as u128;
        let bl = self.block_len();
        if self.buffered > 0 {
            let take = (bl - self.buffered).min(data.len());
            self.buf[self.buffered..self.buffered + take].copy_from_slice(&data[..take]);
            self.buffered += take;
            data = &data[take..];
            if self.buffered == bl {
                let block = self.buf;
                self.compress(&block[..bl]);
                self.buffered = 0;
            }
        }
        while data.len() >= bl {
            let (block, rest) = data.split_at(bl);
            self.compress(block);
            data = rest;
        }
        if !data.is_empty() {
            self.buf[..data.len()].copy_from_slice(data);
            self.buffered = data.len();
        }
    }

    fn compress(&mut self, block: &[u8]) {
        match &mut self.state {
            State::Md5(s) => md5_block(s, block),
            State::Sha1(s) => sha1_block(s, block),
            State::Sha256(s) => sha256_block(s, block),
            State::Sha512(s) => sha512_block(s, block),
        }
    }

    /// The digest as lowercase hex.
    pub fn finish_hex(mut self) -> String {
        let bl = self.block_len();
        let bits = self.total * 8;
        let len_bytes = if bl == 128 { 16 } else { 8 };
        let mut pad = vec![0x80u8];
        let used = (self.total as usize) % bl;
        let zeros = if used + 1 + len_bytes <= bl {
            bl - used - 1 - len_bytes
        } else {
            2 * bl - used - 1 - len_bytes
        };
        pad.extend(std::iter::repeat_n(0u8, zeros));
        let little = self.alg == Alg::Md5;
        if bl == 128 {
            pad.extend_from_slice(&bits.to_be_bytes());
        } else if little {
            pad.extend_from_slice(&(bits as u64).to_le_bytes());
        } else {
            pad.extend_from_slice(&(bits as u64).to_be_bytes());
        }
        let total = self.total;
        self.update(&pad);
        self.total = total;
        let mut out = String::new();
        let mut hex = |b: &[u8]| {
            for x in b {
                out.push_str(&format!("{x:02x}"));
            }
        };
        match &self.state {
            State::Md5(s) => s.iter().for_each(|w| hex(&w.to_le_bytes())),
            State::Sha1(s) => s.iter().for_each(|w| hex(&w.to_be_bytes())),
            State::Sha256(s) => s.iter().for_each(|w| hex(&w.to_be_bytes())),
            State::Sha512(s) => s.iter().for_each(|w| hex(&w.to_be_bytes())),
        }
        out
    }
}

fn md5_block(s: &mut [u32; 4], block: &[u8]) {
    let mut m = [0u32; 16];
    for (i, w) in m.iter_mut().enumerate() {
        *w = u32::from_le_bytes([
            block[4 * i],
            block[4 * i + 1],
            block[4 * i + 2],
            block[4 * i + 3],
        ]);
    }
    let [mut a, mut b, mut c, mut d] = *s;
    for (i, &shift) in MD5_S.iter().enumerate() {
        let (f, g) = match i / 16 {
            0 => ((b & c) | (!b & d), i),
            1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
            2 => (b ^ c ^ d, (3 * i + 5) % 16),
            _ => (c ^ (b | !d), (7 * i) % 16),
        };
        let f2 = f.wrapping_add(a).wrapping_add(md5_k(i)).wrapping_add(m[g]);
        a = d;
        d = c;
        c = b;
        b = b.wrapping_add(f2.rotate_left(shift));
    }
    s[0] = s[0].wrapping_add(a);
    s[1] = s[1].wrapping_add(b);
    s[2] = s[2].wrapping_add(c);
    s[3] = s[3].wrapping_add(d);
}

fn sha1_block(s: &mut [u32; 5], block: &[u8]) {
    let mut w = [0u32; 80];
    for i in 0..16 {
        w[i] = u32::from_be_bytes([
            block[4 * i],
            block[4 * i + 1],
            block[4 * i + 2],
            block[4 * i + 3],
        ]);
    }
    for i in 16..80 {
        w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
    }
    let [mut a, mut b, mut c, mut d, mut e] = *s;
    for (i, wi) in w.iter().enumerate() {
        let (f, k) = match i / 20 {
            0 => ((b & c) | (!b & d), 0x5a827999),
            1 => (b ^ c ^ d, 0x6ed9eba1),
            2 => ((b & c) | (b & d) | (c & d), 0x8f1bbcdc),
            _ => (b ^ c ^ d, 0xca62c1d6u32),
        };
        let t = a
            .rotate_left(5)
            .wrapping_add(f)
            .wrapping_add(e)
            .wrapping_add(k)
            .wrapping_add(*wi);
        e = d;
        d = c;
        c = b.rotate_left(30);
        b = a;
        a = t;
    }
    s[0] = s[0].wrapping_add(a);
    s[1] = s[1].wrapping_add(b);
    s[2] = s[2].wrapping_add(c);
    s[3] = s[3].wrapping_add(d);
    s[4] = s[4].wrapping_add(e);
}

fn sha256_block(s: &mut [u32; 8], block: &[u8]) {
    let mut w = [0u32; 64];
    for i in 0..16 {
        w[i] = u32::from_be_bytes([
            block[4 * i],
            block[4 * i + 1],
            block[4 * i + 2],
            block[4 * i + 3],
        ]);
    }
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }
    let mut v = *s;
    for i in 0..64 {
        let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
        let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
        let t1 = v[7]
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(K256[i])
            .wrapping_add(w[i]);
        let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
        let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
        let t2 = s0.wrapping_add(maj);
        v[7] = v[6];
        v[6] = v[5];
        v[5] = v[4];
        v[4] = v[3].wrapping_add(t1);
        v[3] = v[2];
        v[2] = v[1];
        v[1] = v[0];
        v[0] = t1.wrapping_add(t2);
    }
    for i in 0..8 {
        s[i] = s[i].wrapping_add(v[i]);
    }
}

fn sha512_block(s: &mut [u64; 8], block: &[u8]) {
    let mut w = [0u64; 80];
    for i in 0..16 {
        let mut b = [0u8; 8];
        b.copy_from_slice(&block[8 * i..8 * i + 8]);
        w[i] = u64::from_be_bytes(b);
    }
    for i in 16..80 {
        let s0 = w[i - 15].rotate_right(1) ^ w[i - 15].rotate_right(8) ^ (w[i - 15] >> 7);
        let s1 = w[i - 2].rotate_right(19) ^ w[i - 2].rotate_right(61) ^ (w[i - 2] >> 6);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }
    let mut v = *s;
    for i in 0..80 {
        let s1 = v[4].rotate_right(14) ^ v[4].rotate_right(18) ^ v[4].rotate_right(41);
        let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
        let t1 = v[7]
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(K512[i])
            .wrapping_add(w[i]);
        let s0 = v[0].rotate_right(28) ^ v[0].rotate_right(34) ^ v[0].rotate_right(39);
        let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
        let t2 = s0.wrapping_add(maj);
        v[7] = v[6];
        v[6] = v[5];
        v[5] = v[4];
        v[4] = v[3].wrapping_add(t1);
        v[3] = v[2];
        v[2] = v[1];
        v[1] = v[0];
        v[0] = t1.wrapping_add(t2);
    }
    for i in 0..8 {
        s[i] = s[i].wrapping_add(v[i]);
    }
}

/// The checksum of `data` as lowercase hex.
pub fn hex_of(alg: Alg, data: &[u8]) -> String {
    let mut h = Hasher::new(alg);
    h.update(data);
    h.finish_hex()
}

// ---- Files ----

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileError {
    Cancelled,
    NotAFile,
    Read(String),
}

impl FileError {
    pub fn text(&self) -> String {
        match self {
            FileError::Cancelled => "Stopped.".into(),
            FileError::NotAFile => "Only files can have a checksum.".into(),
            FileError::Read(why) => format!("The file couldn't be read: {why}"),
        }
    }
}

const CHUNK: usize = 256 * 1024;

/// Reads `path` once and returns its checksum for `alg`. `progress` is told
/// the bytes read so far after every chunk; once `cancel` is set the read
/// stops with [`FileError::Cancelled`]. Only regular files are opened (a pipe
/// or a device would never end).
pub fn hash_file(
    path: &Path,
    alg: Alg,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64),
) -> Result<String, FileError> {
    // Looked at before it is opened (a link to a device is followed, and
    // opening a device can have effects of its own), and again when open.
    match std::fs::metadata(path) {
        Ok(m) if m.is_file() => {}
        Ok(_) => return Err(FileError::NotAFile),
        Err(e) => return Err(FileError::Read(e.kind().to_string())),
    }
    // Opened so that a pipe with no writer can't block: only regular files go on.
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY)
        .open(path)
        .map_err(|e| FileError::Read(e.kind().to_string()))?;
    let md = f
        .metadata()
        .map_err(|e| FileError::Read(e.kind().to_string()))?;
    if !md.is_file() {
        return Err(FileError::NotAFile);
    }
    let mut h = Hasher::new(alg);
    let mut buf = vec![0u8; CHUNK];
    let mut done = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(FileError::Cancelled);
        }
        let n = match f.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(FileError::Read(e.kind().to_string())),
        };
        h.update(&buf[..n]);
        done += n as u64;
        progress(done);
    }
    Ok(h.finish_hex())
}

// ---- Comparing with a pasted checksum ----

/// What was pasted, read as a checksum: its kind (from its length) and the
/// hex digits in lowercase. Accepts what people copy: upper or lower case,
/// spaces or colons between bytes, a `sha256:` label, `sha256sum`'s "hex
/// *name" line and the BSD form `SHA256 (name) = hex`. `None` when there is
/// no checksum of a known length in it.
pub fn parse_expected(text: &str) -> Option<(Alg, String)> {
    let t = text.trim();
    if t.is_empty() || t.len() > 4096 {
        return None;
    }
    // The BSD form: the digest is after the last '='.
    let t = match t.rsplit_once('=') {
        Some((_, rest)) if t.contains('(') => rest.trim(),
        _ => t,
    };
    // A label: "sha256:..." (a colon followed by something that is not hex pairs).
    let t = match t.split_once(':') {
        Some((label, rest))
            if label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') && !is_hex(label) =>
        {
            rest.trim()
        }
        _ => t,
    };
    // The first word (sha256sum's "hex  name"); or all of it when it is
    // bytes spread by spaces or colons.
    let first = t.split_whitespace().next().unwrap_or("");
    let compact: String = t
        .chars()
        .filter(|c| !c.is_whitespace() && *c != ':')
        .collect();
    for cand in [first.replace(':', ""), compact] {
        if is_hex(&cand)
            && let Some(alg) = Alg::of_hex_len(cand.len())
        {
            return Some((alg, cand.to_ascii_lowercase()));
        }
    }
    None
}

fn is_hex(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Whether two hex checksums are the same (case does not matter).
pub fn matches(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn published_vectors() {
        assert_eq!(hex_of(Alg::Md5, b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(hex_of(Alg::Md5, b"abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(
            hex_of(Alg::Sha1, b"abc"),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            hex_of(Alg::Sha1, b""),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
        assert_eq!(
            hex_of(Alg::Sha256, b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex_of(Alg::Sha256, b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hex_of(Alg::Sha512, b"abc"),
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
        );
        assert_eq!(
            hex_of(Alg::Sha512, b""),
            "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e"
        );
    }

    #[test]
    fn long_inputs_and_block_edges() {
        let a = vec![b'a'; 1_000_000];
        assert_eq!(
            hex_of(Alg::Sha256, &a),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
        assert_eq!(
            hex_of(Alg::Sha1, &a),
            "34aa973cd4c4daa4f61eeb2bdbad27316534016f"
        );
        assert_eq!(hex_of(Alg::Md5, &a), "7707d6ae4e027c70eea2a935c2296f21");
        assert_eq!(
            hex_of(Alg::Sha512, &a),
            "e718483d0ce769644e2e42c7bc15b4638e1f98b13b2044285632a803afa973ebde0ff244877ea60a4cb0432ce577c31beb009c5c2c49aa2e4eadb217ad8cc09b"
        );
        // 55, 56, 63, 64, 111, 112, 127, 128 bytes: the padding's edges.
        for n in [55usize, 56, 63, 64, 65, 111, 112, 127, 128, 129] {
            let d = vec![0x5au8; n];
            for alg in Alg::ALL {
                let whole = hex_of(alg, &d);
                let mut h = Hasher::new(alg);
                for chunk in d.chunks(7) {
                    h.update(chunk);
                }
                assert_eq!(h.finish_hex(), whole, "{alg:?} {n}");
                assert_eq!(whole.len(), alg.hex_len());
            }
        }
    }

    #[test]
    fn files_and_cancel() {
        let dir = std::env::temp_dir().join(format!("telamon-sum-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("abc.txt");
        std::fs::write(&f, b"abc").unwrap();
        let no = AtomicBool::new(false);
        let mut seen = 0;
        let got = hash_file(&f, Alg::Sha256, &no, &mut |n| seen = n).unwrap();
        assert_eq!(
            got,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(seen, 3);
        let yes = AtomicBool::new(true);
        assert_eq!(
            hash_file(&f, Alg::Sha256, &yes, &mut |_| {}),
            Err(FileError::Cancelled)
        );
        assert_eq!(
            hash_file(&dir, Alg::Sha256, &no, &mut |_| {}),
            Err(FileError::NotAFile)
        );
        // A pipe is refused, not waited for.
        let fifo = dir.join("pipe");
        let c = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
        // SAFETY: a NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        assert_eq!(
            hash_file(&fifo, Alg::Sha256, &no, &mut |_| {}),
            Err(FileError::NotAFile)
        );
        assert!(matches!(
            hash_file(&dir.join("nope"), Alg::Md5, &no, &mut |_| {}),
            Err(FileError::Read(_))
        ));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn pasted_checksums() {
        let h = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let want = Some((Alg::Sha256, h.to_string()));
        assert_eq!(parse_expected(h), want);
        assert_eq!(parse_expected(&format!("  {}\n", h.to_uppercase())), want);
        assert_eq!(parse_expected(&format!("sha256:{h}")), want);
        assert_eq!(parse_expected(&format!("SHA256: {h}")), want);
        assert_eq!(parse_expected(&format!("{h}  file.iso")), want);
        assert_eq!(parse_expected(&format!("{h} *file.iso")), want);
        assert_eq!(parse_expected(&format!("SHA256 (file.iso) = {h}")), want);
        let spaced: Vec<String> = h
            .as_bytes()
            .chunks(2)
            .map(|c| String::from_utf8_lossy(c).into_owned())
            .collect();
        assert_eq!(parse_expected(&spaced.join(" ")), want);
        assert_eq!(parse_expected(&spaced.join(":")), want);
        assert_eq!(
            parse_expected("900150983cd24fb0d6963f7d28e17f72"),
            Some((Alg::Md5, "900150983cd24fb0d6963f7d28e17f72".into()))
        );
        assert_eq!(parse_expected("hello world"), None);
        assert_eq!(parse_expected("abc123"), None);
        assert_eq!(parse_expected(""), None);
        assert_eq!(parse_expected(&"g".repeat(64)), None);
        assert!(matches(h, &h.to_uppercase()));
        assert!(!matches(h, &h[1..]));
    }

    #[test]
    fn algorithm_numbers() {
        for a in Alg::ALL {
            assert_eq!(Alg::from_index(a.index()), Some(a));
        }
        assert_eq!(Alg::from_index(99), None);
        assert_eq!(Alg::ALL[0], Alg::Sha256);
    }
}
