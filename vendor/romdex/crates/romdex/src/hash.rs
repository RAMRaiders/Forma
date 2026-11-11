//! CRC32, MD5 and SHA1 in one pass, streamed: a multi-gigabyte disc image is never in memory.

use std::io::Read;

use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use sha1::Sha1;

const CHUNK: usize = 64 * 1024;

/// Lower-case hex, the form DATs and the import format use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hashes {
    pub crc32: String,
    pub md5: String,
    pub sha1: String,
    /// For a ROM with a copier or emulator header (an `.a78`, `.nes`, `.lnx`, `.fds`): the
    /// hashes of what follows it, which is what No-Intro lists for most of them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headerless: Option<Headerless>,
}

/// A ROM's hashes without its header, `header` bytes long.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Headerless {
    pub header: u64,
    pub crc32: String,
    pub md5: String,
    pub sha1: String,
}

/// How many bytes the header is at the start of `head` (the file's first bytes, up to 128),
/// when it is one DATs list ROMs without: Atari 7800 (128), Atari Lynx (64), NES and
/// Famicom Disk System (16).
pub fn header_len(head: &[u8]) -> u64 {
    if head.len() >= 128 && &head[1..10] == b"ATARI7800" {
        128
    } else if head.len() >= 64 && head.starts_with(b"LYNX\0") {
        64
    } else if head.len() >= 16 && (head.starts_with(b"NES\x1a") || head.starts_with(b"FDS\x1a")) {
        16
    } else {
        0
    }
}

/// The longest header `header_len` knows.
const HEAD: usize = 128;

#[derive(Default)]
struct Hashers {
    crc: crc32fast::Hasher,
    md5: Md5,
    sha1: Sha1,
}

impl Hashers {
    fn update(&mut self, chunk: &[u8]) {
        self.crc.update(chunk);
        self.md5.update(chunk);
        self.sha1.update(chunk);
    }

    fn finish(self) -> (String, String, String) {
        (
            format!("{:08x}", self.crc.finalize()),
            hex(&self.md5.finalize()),
            hex(&self.sha1.finalize()),
        )
    }
}

/// Hashes everything `reader` yields, and, past a known header, what follows it too (in the
/// same pass). Returns the hashes and the number of bytes read.
pub fn hash_reader(mut reader: impl Read) -> std::io::Result<(Hashes, u64)> {
    let mut whole = Hashers::default();
    let mut bare = Hashers::default();
    // The first bytes, until the header is known; then `Some(its length)`.
    let mut head: Vec<u8> = Vec::with_capacity(HEAD);
    let mut header: Option<u64> = None;
    let mut buf = vec![0u8; CHUNK];
    let mut total = 0u64;
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        let chunk = &buf[..n];
        whole.update(chunk);
        total += n as u64;
        match header {
            Some(h) if h > 0 => bare.update(chunk),
            Some(_) => {}
            None => {
                head.extend_from_slice(chunk);
                if head.len() >= HEAD {
                    let h = header_len(&head);
                    if h > 0 {
                        bare.update(&head[h as usize..]);
                    }
                    header = Some(h);
                }
            }
        }
    }
    // Shorter than the longest header: decided on what there is.
    let header = header.unwrap_or_else(|| {
        let h = header_len(&head);
        if h > 0 {
            bare.update(&head[h as usize..]);
        }
        h
    });
    let (crc32, md5, sha1) = whole.finish();
    let headerless = (header > 0 && total > header).then(|| {
        let (crc32, md5, sha1) = bare.finish();
        Headerless {
            header,
            crc32,
            md5,
            sha1,
        }
    });
    Ok((
        Hashes {
            crc32,
            md5,
            sha1,
            headerless,
        },
        total,
    ))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(bytes: &[u8]) -> Hashes {
        hash_reader(bytes).unwrap().0
    }

    #[test]
    fn matches_known_vectors() {
        assert_eq!(
            of(b""),
            Hashes {
                crc32: "00000000".into(),
                md5: "d41d8cd98f00b204e9800998ecf8427e".into(),
                sha1: "da39a3ee5e6b4b0d3255bfef95601890afd80709".into(),
                headerless: None,
            }
        );
        assert_eq!(
            of(b"The quick brown fox jumps over the lazy dog"),
            Hashes {
                crc32: "414fa339".into(),
                md5: "9e107d9d372bb6826bd81d3542a419d6".into(),
                sha1: "2fd4e1c67a2d28fced849ee1bb76e7391b93eb12".into(),
                headerless: None,
            }
        );
    }

    #[test]
    fn streaming_across_chunk_boundaries_matches_hashing_at_once() {
        let data: Vec<u8> = (0..(CHUNK * 3 + 17))
            .map(|i| (i * 31 % 251) as u8)
            .collect();
        // A reader that hands out awkward sizes, like a pipe or a network share.
        struct Dribble<'a>(&'a [u8]);
        impl Read for Dribble<'_> {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                let n = self.0.len().min(buf.len()).min(1000);
                buf[..n].copy_from_slice(&self.0[..n]);
                self.0 = &self.0[n..];
                Ok(n)
            }
        }
        let (dribbled, size) = hash_reader(Dribble(&data)).unwrap();
        assert_eq!(size, data.len() as u64);
        assert_eq!(dribbled, of(&data));
    }

    #[test]
    fn a_headered_rom_also_has_the_hashes_of_what_follows_its_header() {
        let rom: Vec<u8> = (0..(CHUNK + 300)).map(|i| (i * 7 % 253) as u8).collect();
        let mut a78 = vec![0u8; 128];
        a78[0] = 1;
        a78[1..10].copy_from_slice(b"ATARI7800");
        a78.extend_from_slice(&rom);
        let bare = of(&rom);
        let h = of(&a78).headerless.expect("an A78 header");
        assert_eq!(h.header, 128);
        assert_eq!((h.crc32, h.sha1), (bare.crc32, bare.sha1));
        // The same read a few bytes at a time.
        struct Dribble<'a>(&'a [u8]);
        impl Read for Dribble<'_> {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                let n = self.0.len().min(buf.len()).min(5);
                buf[..n].copy_from_slice(&self.0[..n]);
                self.0 = &self.0[n..];
                Ok(n)
            }
        }
        assert_eq!(hash_reader(Dribble(&a78)).unwrap().0, of(&a78));

        let mut nes = b"NES\x1a".to_vec();
        nes.resize(16, 0);
        nes.extend_from_slice(&rom[..40]);
        let h = of(&nes).headerless.expect("an iNES header");
        assert_eq!((h.header, h.crc32), (16, of(&rom[..40]).crc32));
        // No header, or nothing after one: nothing more.
        assert_eq!(of(&rom).headerless, None);
        assert_eq!(of(b"NES\x1a").headerless, None);
    }
}
