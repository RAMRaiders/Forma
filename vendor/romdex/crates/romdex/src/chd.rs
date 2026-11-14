//! CHD disc images (MAME's Compressed Hunks of Data), read as rcheevos reads a CUE sheet.
//!
//! A CD CHD stores every frame as 2352 bytes of sector data and 96 of subcode, pads each
//! track to a multiple of four frames, keeps audio big-endian, and lists its tracks in text
//! metadata. [`Track`] undoes all of that, so a track reads as the bytes of the `.bin` it was
//! made from, and finds its sector size and first sector the way rcheevos' own CUE reader
//! does (`vendor/rcheevos/src/rhash/cdreader.c`, ported line for line): the same disc gives
//! the same RA hash whether it is a CHD or a CUE sheet.
//!
//! Its first track, read whole, is what Redump lists for the disc's first `.bin` ([`hash_first_track`]).

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use chd::metadata::{KnownMetadata, Metadata};

use crate::hash::{Hashes, hash_reader};

/// A CD frame in a CHD: sector data, then subcode.
const FRAME: u64 = 2448;
/// A whole CD sector.
const SECTOR: usize = 2352;
/// Tracks start on a multiple of this many frames.
const PADDING: u32 = 4;

/// Whether `path` names a CHD, by its extension.
pub fn is_chd(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("chd"))
}

/// One track as the CHD's metadata describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackInfo {
    pub number: u32,
    /// `MODE1_RAW`, `MODE2_RAW`, `MODE1`, `AUDIO`…, as chdman writes it.
    pub kind: String,
    /// Frames stored, the pregap among them when it is stored.
    pub frames: u32,
    /// Of those, the pregap's.
    pub pregap: u32,
    /// Bytes of each frame that are the track's: 2352 for raw sectors and audio, 2048 for
    /// cooked ones.
    pub data_size: u32,
    /// Where its first frame is in the CHD, counting padding.
    chd_frame: u64,
    /// Its first frame's sector on the disc, counting the frames stored before it: what a
    /// CUE sheet over the same `.bin` files says, unless the sector headers say otherwise.
    disc_sector: i64,
}

impl TrackInfo {
    pub fn is_audio(&self) -> bool {
        self.kind == "AUDIO"
    }
}

/// Which track rcheevos asks for: a number, or one of its `RC_HASH_CDTRACK_` choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Select {
    Number(u32),
    FirstData,
    Last,
    Largest,
    /// CHD metadata does not record sessions, so this finds nothing.
    FirstOfSecondSession,
}

impl Select {
    /// rcheevos' track argument (`RC_HASH_CDTRACK_*` are the top values of a `uint32_t`).
    pub fn from_rcheevos(track: u32) -> Self {
        match track {
            // Backwards compatibility, as rcheevos' reader: 0 is the largest.
            0 | 0xFFFF_FFFD => Self::Largest,
            0xFFFF_FFFF => Self::FirstData,
            0xFFFF_FFFE => Self::Last,
            0xFFFF_FFFC => Self::FirstOfSecondSession,
            n => Self::Number(n),
        }
    }
}

/// An open CHD: its tracks, and the last hunk read.
pub struct Disc {
    chd: chd::Chd<BufReader<File>>,
    tracks: Vec<TrackInfo>,
    hunk_size: u64,
    hunk: Vec<u8>,
    compressed: Vec<u8>,
    loaded: Option<u32>,
}

impl Disc {
    /// Opens a CD or GD-ROM CHD. `Err` says why not: unreadable, not a CD, a CHD that needs
    /// its parent.
    pub fn open(path: &Path) -> Result<Self, String> {
        let file = File::open(path).map_err(|e| e.to_string())?;
        let mut chd = chd::Chd::open(BufReader::new(file), None).map_err(|e| e.to_string())?;
        if chd.header().has_parent() {
            return Err("a CHD that needs its parent".into());
        }
        let metadata: Vec<Metadata> = chd
            .metadata_refs()
            .try_into()
            .map_err(|e: chd::Error| e.to_string())?;
        let tracks = parse_tracks(&metadata).ok_or("not a CD image")?;
        let hunk_size = u64::from(chd.header().hunk_size());
        if hunk_size == 0 || hunk_size % FRAME != 0 {
            return Err(format!(
                "hunks of {hunk_size} bytes are not whole CD frames"
            ));
        }
        Ok(Self {
            hunk: chd.get_hunksized_buffer(),
            chd,
            tracks,
            hunk_size,
            compressed: Vec::new(),
            loaded: None,
        })
    }

    pub fn tracks(&self) -> &[TrackInfo] {
        &self.tracks
    }

    /// The index in [`tracks`](Self::tracks) of the one `select` names.
    pub fn find(&self, select: Select) -> Option<usize> {
        let t = &self.tracks;
        match select {
            Select::Number(n) => t.iter().position(|x| x.number == n),
            Select::FirstData => t.iter().position(|x| !x.is_audio()),
            Select::Last => t.len().checked_sub(1),
            Select::Largest => t
                .iter()
                .enumerate()
                .filter(|(_, x)| !x.is_audio())
                // The first of equals, as rcheevos keeps the first it finds.
                .rev()
                .max_by_key(|(_, x)| x.frames)
                .map(|(i, _)| i),
            Select::FirstOfSecondSession => None,
        }
    }

    /// Bytes `offset..` of track `index`'s data, as its `.bin` holds them, into `buf`.
    /// Returns how many there were: fewer than asked at the end of the track.
    pub fn read_track(&mut self, index: usize, offset: u64, buf: &mut [u8]) -> usize {
        let track = &self.tracks[index];
        let data_size = u64::from(track.data_size);
        let end = u64::from(track.frames) * data_size;
        let (chd_frame, audio) = (track.chd_frame, track.is_audio());
        let mut done = 0;
        let mut at = offset;
        while done < buf.len() && at < end {
            let frame = at / data_size;
            let within = (at % data_size) as usize;
            let Some(data) = self.frame(chd_frame + frame, data_size as usize, audio) else {
                break;
            };
            let n = (data.len() - within).min(buf.len() - done);
            buf[done..done + n].copy_from_slice(&data[within..within + n]);
            done += n;
            at += n as u64;
        }
        done
    }

    /// The first `len` bytes of frame `frame`, audio turned back to little-endian as a `.bin`
    /// holds it. `None` when the hunk cannot be read.
    fn frame(&mut self, frame: u64, len: usize, audio: bool) -> Option<Vec<u8>> {
        let start = frame * FRAME;
        let hunk = u32::try_from(start / self.hunk_size).ok()?;
        if self.loaded != Some(hunk) {
            self.loaded = None;
            let mut h = self.chd.hunk(hunk).ok()?;
            h.read_hunk_in(&mut self.compressed, &mut self.hunk).ok()?;
            self.loaded = Some(hunk);
        }
        let within = (start % self.hunk_size) as usize;
        let mut data = self.hunk.get(within..within + len)?.to_vec();
        if audio {
            for pair in data.as_chunks_mut::<2>().0 {
                pair.swap(0, 1);
            }
        }
        Some(data)
    }
}

/// The tracks from CHD metadata (`CHT2`, the older `CHTR`, or GD-ROM `CHGD`), in order.
/// `None` when there are none: not a CD.
fn parse_tracks(metadata: &[Metadata]) -> Option<Vec<TrackInfo>> {
    let text_tags = [
        KnownMetadata::CdRomTrack2 as u32,
        KnownMetadata::CdRomTrack as u32,
        KnownMetadata::GdRomTrack as u32,
    ];
    let mut tracks = Vec::new();
    let (mut chd_frame, mut disc_sector) = (0u64, 0i64);
    for entry in metadata.iter().filter(|m| text_tags.contains(&m.metatag)) {
        let text = String::from_utf8_lossy(&entry.value);
        let field = |key: &str| {
            text.trim_end_matches('\0')
                .split_ascii_whitespace()
                .find_map(|kv| kv.strip_prefix(key)?.strip_prefix(':'))
        };
        let number = field("TRACK")?.parse().ok()?;
        let kind = field("TYPE")?.to_owned();
        let frames: u32 = field("FRAMES")?.parse().ok()?;
        // A pregap is in the data only when its type says so ("V" for "valid data").
        let pregap = match field("PGTYPE") {
            Some(t) if t.starts_with('V') => field("PREGAP").and_then(|p| p.parse().ok()),
            _ => None,
        }
        .unwrap_or(0);
        let data_size = data_size(&kind)?;
        tracks.push(TrackInfo {
            number,
            kind,
            frames,
            pregap,
            data_size,
            chd_frame,
            disc_sector,
        });
        chd_frame += u64::from(frames.div_ceil(PADDING) * PADDING);
        disc_sector += i64::from(frames);
    }
    tracks.sort_by_key(|t| t.number);
    (!tracks.is_empty()).then_some(tracks)
}

/// How many bytes of a frame a track type's sectors fill, as MAME's `cdrom.cpp` has them.
fn data_size(kind: &str) -> Option<u32> {
    Some(match kind {
        "MODE1_RAW" | "MODE2_RAW" | "AUDIO" | "CDI/2352" => 2352,
        "MODE1" | "MODE1/2048" | "MODE2_FORM1" => 2048,
        "MODE2" | "MODE2_FORM_MIX" | "CDI/2336" => 2336,
        "MODE2_FORM2" => 2324,
        _ => return None,
    })
}

/// A track opened for rcheevos: its disc's reader, and where its sectors are.
pub struct Track {
    disc: Disc,
    index: usize,
    sector_size: i64,
    header_size: i64,
    raw_data_size: usize,
    first_sector: i64,
    pregap: i64,
}

impl Track {
    /// The track of `path` that `select` names. `None` when there is none or its sector size
    /// cannot be told, as rcheevos' reader returns no track.
    pub fn open(path: &Path, select: Select) -> Option<Self> {
        let disc = Disc::open(path).ok()?;
        let index = disc.find(select)?;
        let info = &disc.tracks[index];
        let mut track = Self {
            index,
            sector_size: 0,
            header_size: 0,
            raw_data_size: 2048,
            first_sector: info.disc_sector,
            pregap: i64::from(info.pregap),
            disc,
        };
        track.determine_sector_size();
        if track.sector_size == 0 {
            // The sync pattern was not found: as rcheevos does with a CUE sheet's mode.
            let info = &track.disc.tracks[index];
            let (size, header) = match info.kind.as_str() {
                "MODE1_RAW" => (2352, 16),
                "MODE2_RAW" | "CDI/2352" => (2352, 24),
                "MODE1" | "MODE1/2048" | "MODE2_FORM1" => (2048, 0),
                "MODE2" | "MODE2_FORM_MIX" | "CDI/2336" => (2336, 8),
                "AUDIO" => {
                    track.raw_data_size = SECTOR;
                    (2352, 0)
                }
                _ => return None,
            };
            track.sector_size = size;
            track.header_size = header;
        }
        Some(track)
    }

    /// `cdreader_determine_sector_size`: the sync pattern at sector 16 past the pregap tells
    /// the sector size, and its header the track's first sector on the disc.
    fn determine_sector_size(&mut self) {
        const SYNC: [u8; 12] = [0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 0];
        let toc = 16 + self.pregap;
        let mut header = [0u8; 32];
        for size in [2352, 2336] {
            let n = self.read_at(toc * size, &mut header);
            if n < header.len() {
                if size == 2352 {
                    return;
                }
                continue;
            }
            if header[..12] == SYNC {
                self.sector_size = size;
                self.header_size = if &header[25..30] == b"CD001" { 24 } else { 16 };
                self.first_sector = msf_sector(&header) - toc;
                return;
            }
        }
        self.read_at(toc * 2048, &mut header);
        if &header[1..6] == b"CD001" {
            self.sector_size = 2048;
            self.header_size = 0;
        }
    }

    fn read_at(&mut self, offset: i64, buf: &mut [u8]) -> usize {
        match u64::try_from(offset) {
            Ok(at) => self.disc.read_track(self.index, at, buf),
            Err(_) => 0,
        }
    }

    /// `cdreader_read_sector`: up to `buf.len()` bytes of data from absolute sector `sector`
    /// on, each sector's header left out. Returns how many were read.
    pub fn read_sector(&mut self, sector: u32, buf: &mut [u8]) -> usize {
        let sector = i64::from(sector);
        if sector < self.first_sector {
            return 0;
        }
        let mut start = (sector - self.first_sector) * self.sector_size + self.header_size;
        let mut total = 0;
        let mut rest = buf;
        while rest.len() > self.raw_data_size {
            let (now, later) = rest.split_at_mut(self.raw_data_size);
            let n = self.read_at(start, now);
            total += n;
            if n < self.raw_data_size {
                return total;
            }
            rest = later;
            start += self.sector_size;
        }
        total + self.read_at(start, rest)
    }

    /// `cdreader_first_track_sector`: the first sector past the pregap.
    pub fn first_sector(&self) -> u32 {
        u32::try_from(self.first_sector + self.pregap).unwrap_or(0)
    }
}

/// The sector a raw sector's header names (minutes, seconds, frames in BCD), less the two
/// seconds before sector 0.
fn msf_sector(header: &[u8]) -> i64 {
    let bcd = |b: u8| i64::from(b >> 4) * 10 + i64::from(b & 0x0F);
    (bcd(header[12]) * 60 + bcd(header[13])) * 75 + bcd(header[14]) - 150
}

/// The hashes of `path`'s first track, read whole, and its size and name: what Redump lists
/// for a disc's first `.bin`, as for the first track of a CUE sheet.
pub fn hash_first_track(path: &Path) -> Result<(Hashes, u64, String), String> {
    let mut disc = Disc::open(path)?;
    let number = disc.tracks[0].number;
    let reader = TrackReader {
        disc: &mut disc,
        index: 0,
        at: 0,
    };
    let (hashes, size) =
        hash_reader(BufReader::with_capacity(1 << 20, reader)).map_err(|e| e.to_string())?;
    let expected = {
        let t = &disc.tracks[0];
        u64::from(t.frames) * u64::from(t.data_size)
    };
    if size != expected {
        return Err(format!(
            "track {number} ends after {size} of its {expected} bytes"
        ));
    }
    Ok((hashes, size, format!("Track {number:02}")))
}

struct TrackReader<'a> {
    disc: &'a mut Disc,
    index: usize,
    at: u64,
}

impl Read for TrackReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.disc.read_track(self.index, self.at, buf);
        self.at += n as u64;
        Ok(n)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use chd::write::{ChdWriteOptions, ChdWriter};
    use std::ffi::CString;
    use std::io::{Seek, Write};

    /// A track of a test disc: its type, as a CUE sheet and as CHD metadata, and its sectors.
    pub struct TestTrack {
        pub cue_mode: &'static str,
        pub chd_type: &'static str,
        /// Each sector's bytes as the `.bin` holds them (2352, or 2048 cooked).
        pub sectors: Vec<Vec<u8>>,
        /// Leading sectors that are pregap (`INDEX 00`), stored in the data.
        pub pregap: u32,
    }

    /// A raw Mode 1 sector: sync, header naming disc sector `lba`, then `data` (to 2048).
    pub fn mode1(lba: u32, data: &[u8]) -> Vec<u8> {
        let mut s = vec![0u8; SECTOR];
        s[1..11].fill(0xFF);
        let msf = lba + 150;
        let bcd = |v: u32| ((v / 10) << 4 | (v % 10)) as u8;
        s[12] = bcd(msf / 75 / 60);
        s[13] = bcd(msf / 75 % 60);
        s[14] = bcd(msf % 75);
        s[15] = 1;
        s[16..16 + data.len()].copy_from_slice(data);
        s
    }

    /// Writes the disc both ways in `dir`: `name.cue` with one `.bin` per track, and
    /// `name.chd` as chdman lays it out. Returns their paths.
    pub fn write_disc(
        dir: &Path,
        name: &str,
        tracks: &[TestTrack],
    ) -> (std::path::PathBuf, std::path::PathBuf) {
        let mut cue = String::new();
        let mut frames: Vec<u8> = Vec::new();
        for (i, t) in tracks.iter().enumerate() {
            let n = i + 1;
            let bin = format!("{name} (Track {n}).bin");
            let mut bytes = Vec::new();
            for s in &t.sectors {
                bytes.extend_from_slice(s);
            }
            std::fs::write(dir.join(&bin), &bytes).unwrap();
            cue += &format!("FILE \"{bin}\" BINARY\n  TRACK {n:02} {}\n", t.cue_mode);
            if t.pregap > 0 {
                let p = t.pregap;
                cue += &format!(
                    "    INDEX 00 00:00:00\n    INDEX 01 {:02}:{:02}:{:02}\n",
                    p / 75 / 60,
                    p / 75 % 60,
                    p % 75
                );
            } else {
                cue += "    INDEX 01 00:00:00\n";
            }
            let audio = t.chd_type == "AUDIO";
            for s in &t.sectors {
                let mut frame = s.clone();
                if audio {
                    for pair in frame.as_chunks_mut::<2>().0 {
                        pair.swap(0, 1);
                    }
                }
                frame.resize(FRAME as usize, 0);
                frames.extend_from_slice(&frame);
            }
            let padded = (t.sectors.len() as u32).div_ceil(PADDING) * PADDING;
            frames.resize(
                frames.len() + (padded as usize - t.sectors.len()) * FRAME as usize,
                0,
            );
        }
        let cue_path = dir.join(format!("{name}.cue"));
        std::fs::write(&cue_path, cue).unwrap();

        let chd_path = dir.join(format!("{name}.chd"));
        let file = std::fs::File::create(&chd_path).unwrap();
        let mut w = ChdWriter::new(file, ChdWriteOptions::cd()).unwrap();
        for (i, t) in tracks.iter().enumerate() {
            let (pregap, pgtype) = if t.pregap > 0 {
                (t.pregap, format!("V{}", t.chd_type))
            } else {
                (0, t.chd_type.to_owned())
            };
            let text = format!(
                "TRACK:{} TYPE:{} SUBTYPE:NONE FRAMES:{} PREGAP:{pregap} PGTYPE:{pgtype} PGSUB:NONE POSTGAP:0",
                i + 1,
                t.chd_type,
                t.sectors.len()
            );
            let text = CString::new(text).unwrap();
            w.add_metadata(KnownMetadata::CdRomTrack2, text.as_c_str(), true)
                .unwrap();
        }
        let hunk = 8 * FRAME as usize;
        frames.resize(frames.len().div_ceil(hunk) * hunk, 0);
        for chunk in frames.chunks(hunk) {
            w.write_hunk(chunk).unwrap();
        }
        let mut file = w.finish().unwrap();
        file.flush().unwrap();
        file.rewind().unwrap();
        (cue_path, chd_path)
    }

    fn audio(n: u32, seed: u8) -> Vec<Vec<u8>> {
        (0..n)
            .map(|i| (0..SECTOR).map(|b| (b as u8) ^ seed ^ (i as u8)).collect())
            .collect()
    }

    fn data_track(sectors: u32, first_lba: u32) -> Vec<Vec<u8>> {
        (0..sectors)
            .map(|i| mode1(first_lba + i, &[i as u8; 2048]))
            .collect()
    }

    #[test]
    fn reads_tracks_back_as_their_bins_audio_and_padding_undone() {
        let dir = tempfile::tempdir().unwrap();
        let tracks = [
            TestTrack {
                cue_mode: "MODE1/2352",
                chd_type: "MODE1_RAW",
                sectors: data_track(21, 0),
                pregap: 0,
            },
            TestTrack {
                cue_mode: "AUDIO",
                chd_type: "AUDIO",
                sectors: audio(13, 7),
                pregap: 3,
            },
        ];
        let (_, chd) = write_disc(dir.path(), "Disc", &tracks);
        let mut disc = Disc::open(&chd).unwrap();
        assert_eq!(disc.tracks().len(), 2);
        assert_eq!(disc.tracks()[1].pregap, 3);
        for (i, t) in tracks.iter().enumerate() {
            let want: Vec<u8> = t.sectors.concat();
            let mut got = vec![0u8; want.len() + 100];
            assert_eq!(disc.read_track(i, 0, &mut got), want.len(), "track {i}");
            assert_eq!(&got[..want.len()], &want[..]);
        }
        assert_eq!(disc.find(Select::FirstData), Some(0));
        assert_eq!(disc.find(Select::Last), Some(1));
        assert_eq!(disc.find(Select::Largest), Some(0));
        assert_eq!(disc.find(Select::Number(2)), Some(1));
        assert_eq!(disc.find(Select::Number(3)), None);
        assert_eq!(disc.find(Select::FirstOfSecondSession), None);

        let (hashes, size, name) = hash_first_track(&chd).unwrap();
        let bin = std::fs::read(dir.path().join("Disc (Track 1).bin")).unwrap();
        let (want, want_size) = hash_reader(&bin[..]).unwrap();
        assert_eq!((hashes, size, name.as_str()), (want, want_size, "Track 01"));
    }

    #[test]
    fn a_track_finds_its_sectors_from_their_headers() {
        let dir = tempfile::tempdir().unwrap();
        let tracks = [
            TestTrack {
                cue_mode: "AUDIO",
                chd_type: "AUDIO",
                sectors: audio(30, 1),
                pregap: 0,
            },
            TestTrack {
                cue_mode: "MODE1/2352",
                chd_type: "MODE1_RAW",
                // Two pregap sectors, then the track at disc sector 32.
                sectors: data_track(40, 30),
                pregap: 2,
            },
        ];
        let (_, chd) = write_disc(dir.path(), "Disc", &tracks);
        let mut track = Track::open(&chd, Select::FirstData).unwrap();
        assert_eq!(track.first_sector(), 32);
        let mut buf = [0u8; 2048 + 10];
        assert_eq!(track.read_sector(33, &mut buf), buf.len());
        assert_eq!(buf[0], 3, "disc sector 33 is the track's fourth sector");
        assert_eq!(
            buf[2048], 4,
            "the next sector's data follows, its header left out"
        );
        assert_eq!(track.read_sector(10, &mut buf), 0, "before the track");
    }

    #[test]
    fn what_is_not_a_cd_chd_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let junk = dir.path().join("junk.chd");
        std::fs::write(&junk, b"not a chd").unwrap();
        assert!(Disc::open(&junk).is_err());
        assert!(Disc::open(&dir.path().join("missing.chd")).is_err());
        assert!(is_chd(Path::new("A.CHD")) && !is_chd(Path::new("a.cue")));
    }
}
