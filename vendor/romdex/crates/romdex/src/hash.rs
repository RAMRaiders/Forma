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
}

/// Hashes everything `reader` yields. Returns the hashes and the number of bytes read.
pub fn hash_reader(mut reader: impl Read) -> std::io::Result<(Hashes, u64)> {
    let mut crc = crc32fast::Hasher::new();
    let mut md5 = Md5::new();
    let mut sha1 = Sha1::new();
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
        crc.update(chunk);
        md5.update(chunk);
        sha1.update(chunk);
        total += n as u64;
    }
    Ok((
        Hashes {
            crc32: format!("{:08x}", crc.finalize()),
            md5: hex(&md5.finalize()),
            sha1: hex(&sha1.finalize()),
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
            }
        );
        assert_eq!(
            of(b"The quick brown fox jumps over the lazy dog"),
            Hashes {
                crc32: "414fa339".into(),
                md5: "9e107d9d372bb6826bd81d3542a419d6".into(),
                sha1: "2fd4e1c67a2d28fced849ee1bb76e7391b93eb12".into(),
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
}
