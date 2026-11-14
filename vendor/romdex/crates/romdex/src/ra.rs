//! RetroAchievements hashes, computed by rcheevos itself (vendored in `vendor/rcheevos`).
//!
//! An RA hash looks like an MD5, and for many cartridges it is one, but rcheevos applies
//! per-console rules: it skips the NES header, hashes discs by sector, hashes arcade games by
//! file name. Guessed rules fail silently on the systems nobody tests, so romdex never computes
//! an RA hash itself: it asks rcheevos, and when rcheevos declines (`None`) it says "unknown".
//!
//! This is the only module allowed `unsafe`: the calls into rcheevos' C.
#![allow(unsafe_code)]

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::{Mutex, Once, OnceLock};

use crate::chd;

unsafe extern "C" {
    fn rc_hash_generate_from_file(hash: *mut c_char, console_id: u32, path: *const c_char)
    -> c_int;
    fn rc_hash_generate_from_buffer(
        hash: *mut c_char,
        console_id: u32,
        buffer: *const u8,
        buffer_size: usize,
    ) -> c_int;
    fn rc_hash_get_default_cdreader(reader: *mut CdReader);
    fn rc_hash_init_custom_cdreader(reader: *mut CdReader);
}

/// rcheevos' `rc_hash_cdreader_t`: how it opens and reads disc tracks.
#[repr(C)]
#[derive(Clone, Copy)]
struct CdReader {
    open_track: Option<unsafe extern "C" fn(*const c_char, u32) -> *mut c_void>,
    read_sector: Option<unsafe extern "C" fn(*mut c_void, u32, *mut c_void, usize) -> usize>,
    close_track: Option<unsafe extern "C" fn(*mut c_void)>,
    first_track_sector: Option<unsafe extern "C" fn(*mut c_void) -> u32>,
    open_track_iterator:
        Option<unsafe extern "C" fn(*const c_char, u32, *const c_void) -> *mut c_void>,
}

/// rcheevos' own reader, for CUE sheets, GDI and bare images.
static DEFAULT_READER: OnceLock<CdReader> = OnceLock::new();

/// A track handle given to rcheevos: a CHD's, or one from its own reader.
enum Handle {
    Chd(Box<chd::Track>),
    Default(*mut c_void),
}

/// rcheevos has no CHD reader of its own (RetroArch gives it one): this reader reads CHDs
/// with [`chd`] and passes everything else to rcheevos' reader. Installed once, before the
/// first hash.
fn install_disc_reader() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let mut default = CdReader {
            open_track: None,
            read_sector: None,
            close_track: None,
            first_track_sector: None,
            open_track_iterator: None,
        };
        // SAFETY: rcheevos fills the struct it is given, which has its layout.
        unsafe { rc_hash_get_default_cdreader(&mut default) };
        let _ = DEFAULT_READER.set(default);
        let mut ours = CdReader {
            open_track: None,
            read_sector: Some(read_sector),
            close_track: Some(close_track),
            first_track_sector: Some(first_track_sector),
            open_track_iterator: Some(open_track),
        };
        // SAFETY: rcheevos copies the struct; the functions live for the whole program.
        unsafe { rc_hash_init_custom_cdreader(&mut ours) };
    });
}

/// Opens track `track` of `path`. A panic must not cross into C: it opens nothing.
unsafe extern "C" fn open_track(
    path: *const c_char,
    track: u32,
    iterator: *const c_void,
) -> *mut c_void {
    if path.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: rcheevos passes a NUL-terminated path that lives across the call.
    let name = unsafe { CStr::from_ptr(path) };
    let Ok(name) = name.to_str() else {
        return std::ptr::null_mut();
    };
    let handle = if chd::is_chd(Path::new(name)) {
        let opened =
            catch_unwind(|| chd::Track::open(Path::new(name), chd::Select::from_rcheevos(track)));
        match opened {
            Ok(Some(t)) => Handle::Chd(Box::new(t)),
            _ => return std::ptr::null_mut(),
        }
    } else {
        let Some(open) = DEFAULT_READER.get().and_then(|d| d.open_track_iterator) else {
            return std::ptr::null_mut();
        };
        // SAFETY: rcheevos' own reader, called as rcheevos would with its own arguments.
        let inner = unsafe { open(path, track, iterator) };
        if inner.is_null() {
            return std::ptr::null_mut();
        }
        Handle::Default(inner)
    };
    Box::into_raw(Box::new(handle)).cast()
}

unsafe extern "C" fn read_sector(
    handle: *mut c_void,
    sector: u32,
    buffer: *mut c_void,
    requested: usize,
) -> usize {
    if handle.is_null() || buffer.is_null() {
        return 0;
    }
    // SAFETY: `handle` came from `open_track` and is not yet closed; rcheevos uses a handle
    // from one thread at a time (under `RCHEEVOS`).
    match unsafe { &mut *handle.cast::<Handle>() } {
        Handle::Chd(track) => {
            // SAFETY: rcheevos' buffer holds `requested` bytes.
            let buf = unsafe { std::slice::from_raw_parts_mut(buffer.cast::<u8>(), requested) };
            catch_unwind(AssertUnwindSafe(|| track.read_sector(sector, buf))).unwrap_or(0)
        }
        Handle::Default(inner) => match DEFAULT_READER.get().and_then(|d| d.read_sector) {
            // SAFETY: the default reader's own handle and rcheevos' buffer.
            Some(read) => unsafe { read(*inner, sector, buffer, requested) },
            None => 0,
        },
    }
}

unsafe extern "C" fn close_track(handle: *mut c_void) {
    if handle.is_null() {
        return;
    }
    // SAFETY: `handle` came from `open_track` (a `Box<Handle>`) and is closed once.
    let handle = unsafe { Box::from_raw(handle.cast::<Handle>()) };
    if let Handle::Default(inner) = *handle
        && let Some(close) = DEFAULT_READER.get().and_then(|d| d.close_track)
    {
        // SAFETY: the default reader's own handle, closed once.
        unsafe { close(inner) };
    }
}

unsafe extern "C" fn first_track_sector(handle: *mut c_void) -> u32 {
    if handle.is_null() {
        return 0;
    }
    // SAFETY: `handle` came from `open_track` and is not yet closed.
    match unsafe { &*handle.cast::<Handle>() } {
        Handle::Chd(track) => track.first_sector(),
        Handle::Default(inner) => match DEFAULT_READER.get().and_then(|d| d.first_track_sector) {
            // SAFETY: the default reader's own handle.
            Some(first) => unsafe { first(*inner) },
            None => 0,
        },
    }
}

/// rcheevos keeps some global state (its default readers and callbacks); one call at a time.
static RCHEEVOS: Mutex<()> = Mutex::new(());

/// An RA console id, as rcheevos and the RA Web API number them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Console(pub u32);

impl Console {
    pub const ARCADE: Console = Console(27);

    /// Consoles rcheevos hashes by file name rather than contents: their hash must come from
    /// the path, archive or not.
    pub fn hashes_by_name(self) -> bool {
        self == Self::ARCADE
    }
}

/// The RA hash of a file on disk. Disc images (cue/bin, iso, m3u) are read by rcheevos' own
/// disc reader, CHDs by [`chd`].
pub fn hash_file(console: Console, path: &Path) -> Option<String> {
    let path = CString::new(path.to_str()?).ok()?;
    let mut hash = [0 as c_char; 33];
    let _guard = RCHEEVOS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    install_disc_reader();
    // SAFETY: `hash` is the 33 bytes rcheevos writes into; `path` is NUL-terminated and lives
    // across the call; rcheevos does not keep either pointer.
    let ok = unsafe { rc_hash_generate_from_file(hash.as_mut_ptr(), console.0, path.as_ptr()) };
    finish(ok, &hash)
}

/// The RA hash of a ROM already in memory (one read out of an archive).
pub fn hash_buffer(console: Console, data: &[u8]) -> Option<String> {
    let mut hash = [0 as c_char; 33];
    let _guard = RCHEEVOS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    install_disc_reader();
    // SAFETY: `data` is valid for `data.len()` bytes for the duration of the call and is only
    // read; `hash` is the 33 bytes rcheevos writes into.
    let ok = unsafe {
        rc_hash_generate_from_buffer(hash.as_mut_ptr(), console.0, data.as_ptr(), data.len())
    };
    finish(ok, &hash)
}

fn finish(ok: c_int, hash: &[c_char; 33]) -> Option<String> {
    if ok == 0 {
        return None;
    }
    let bytes: Vec<u8> = hash
        .iter()
        .take_while(|c| **c != 0)
        .map(|c| *c as u8)
        .collect();
    let hex = String::from_utf8(bytes).ok()?.to_ascii_lowercase();
    (hex.len() == 32 && hex.chars().all(|c| c.is_ascii_hexdigit())).then_some(hex)
}

/// The RA console a Batocera system folder holds, when RetroAchievements supports it.
///
/// Batocera's own system ids (`es_systems.cfg` `<name>`); several share a console (the MSX
/// generations, the arcade emulators). A system missing here is not hashed for RA, rather than
/// hashed as a guess.
pub fn console_for(system: &str) -> Option<Console> {
    let id = match system {
        "megadrive" | "genesis" => 1,
        "n64" => 2,
        "snes" | "sfc" | "satellaview" | "sufami" => 3,
        // Super Game Boy and the two-player link folders hold Game Boy ROMs.
        "gb" | "sgb" | "gb2players" => 4,
        "gba" => 5,
        "gbc" | "gbc2players" => 6,
        "nes" | "famicom" => 7,
        "pcengine" | "supergrafx" => 8,
        "segacd" | "megacd" => 9,
        "sega32x" => 10,
        "mastersystem" => 11,
        "psx" => 12,
        "lynx" => 13,
        "ngp" | "ngpc" => 14,
        "gamegear" => 15,
        "gamecube" => 16,
        "jaguar" => 17,
        "nds" => 18,
        "wii" => 19,
        "ps2" => 21,
        // `odyssey2` is the folder on Batocera 43 (checked on a Pi 5); `o2em` is the emulator.
        "odyssey2" | "o2em" | "videopacplus" => 23,
        "pokemini" => 24,
        "atari2600" => 25,
        "dos" => 26,
        "mame" | "fbneo" | "neogeo" | "arcade" | "cps1" | "cps2" | "cps3" => 27,
        "virtualboy" => 28,
        "msx1" | "msx2" | "msx2+" | "msxturbor" => 29,
        "c64" => 30,
        "zx81" => 31,
        "oricatmos" => 32,
        "sg1000" => 33,
        "c20" => 34,
        "amiga500" | "amiga1200" => 35,
        "atarist" => 36,
        "amstradcpc" => 37,
        "apple2" => 38,
        "saturn" => 39,
        "dreamcast" => 40,
        "psp" => 41,
        "cdi" => 42,
        "3do" => 43,
        "colecovision" => 44,
        "intellivision" => 45,
        "vectrex" => 46,
        "pc88" => 47,
        "pc98" => 48,
        "pcfx" => 49,
        "atari5200" => 50,
        "atari7800" => 51,
        "x68000" => 52,
        "wswan" | "wswanc" => 53,
        "scv" => 54,
        "neogeocd" => 56,
        "channelf" => 57,
        "fmtowns" => 58,
        "zxspectrum" => 59,
        "gameandwatch" => 60,
        "3ds" => 62,
        "supervision" => 63,
        "tic80" => 65,
        "thomson" => 66,
        "pico" => 68,
        "megaduck" => 69,
        "arduboy" => 71,
        "wasm4" => 72,
        "arcadia" => 73,
        "vc4000" => 74,
        "pcenginecd" => 76,
        "jaguarcd" => 77,
        "uzebox" => 80,
        "fds" => 81,
        _ => return None,
    };
    Some(Console(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NES: Console = Console(7);
    const GAMEBOY: Console = Console(4);

    #[test]
    fn a_plain_cartridge_hash_is_its_md5() {
        assert_eq!(
            hash_buffer(GAMEBOY, b"abc").as_deref(),
            Some("900150983cd24fb0d6963f7d28e17f72")
        );
    }

    #[test]
    fn the_nes_header_is_skipped_as_rcheevos_rules_say() {
        let mut rom = vec![b'N', b'E', b'S', 0x1a];
        rom.resize(16, 0);
        rom.extend_from_slice(&[0u8; 32]);
        // MD5 of the 32 bytes after the iNES header, not of the whole file.
        assert_eq!(
            hash_buffer(NES, &rom).as_deref(),
            Some("70bc8f4b72a86921468bf8e8441dce51")
        );
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), &rom).unwrap();
        assert_eq!(hash_file(NES, file.path()), hash_buffer(NES, &rom));
    }

    #[test]
    fn declined_input_is_unknown_never_a_guess() {
        // A PlayStation "disc" of garbage: rcheevos will not hash what it does not recognise.
        let file = tempfile::Builder::new().suffix(".bin").tempfile().unwrap();
        std::fs::write(file.path(), [0u8; 4096]).unwrap();
        assert_eq!(hash_file(Console(12), file.path()), None);
        assert_eq!(hash_file(NES, Path::new("/definitely/not/here.nes")), None);
    }

    #[test]
    fn arcade_is_hashed_by_file_name() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("pacman.zip");
        let b = dir.path().join("other/pacman.zip");
        std::fs::create_dir_all(b.parent().unwrap()).unwrap();
        std::fs::write(&a, b"one").unwrap();
        std::fs::write(&b, b"two").unwrap();
        let ha = hash_file(Console::ARCADE, &a).unwrap();
        assert_eq!(
            Some(ha.clone()),
            hash_file(Console::ARCADE, &b),
            "same name, same hash"
        );
        // MD5 of the name without its extension.
        assert_eq!(ha, "64d1f88b9b276aece4b0edcc25b7a434");
        assert!(Console::ARCADE.hashes_by_name());
    }

    use crate::chd::tests::{TestTrack, mode1, write_disc};

    /// A CHD of a disc and the CUE sheet it was made from have one RA hash.
    fn same_hash_both_ways(console: Console, tracks: &[TestTrack]) -> String {
        let dir = tempfile::tempdir().unwrap();
        let (cue, chd) = write_disc(dir.path(), "Game", tracks);
        let from_cue = hash_file(console, &cue).expect("rcheevos hashes the CUE sheet");
        assert_eq!(hash_file(console, &chd).as_deref(), Some(from_cue.as_str()));
        from_cue
    }

    fn sega_disc(header: &[u8; 16]) -> Vec<Vec<u8>> {
        (0..24u32)
            .map(|lba| {
                let mut data = vec![lba as u8; 2048];
                if lba == 0 {
                    data[..16].copy_from_slice(header);
                }
                mode1(lba, &data)
            })
            .collect()
    }

    #[test]
    fn saturn_and_sega_cd_chds_hash_as_their_cue_sheets() {
        for (console, header) in [
            (Console(39), b"SEGA SEGASATURN "),
            (Console(9), b"SEGADISCSYSTEM  "),
        ] {
            same_hash_both_ways(
                console,
                &[TestTrack {
                    cue_mode: "MODE1/2352",
                    chd_type: "MODE1_RAW",
                    sectors: sega_disc(header),
                    pregap: 0,
                }],
            );
        }
    }

    #[test]
    fn a_data_track_after_audio_is_found_where_its_cue_sheet_puts_it() {
        // PC Engine CD: audio first, then the data track (with a pregap) holding the boot
        // header in its second sector and the program it points to.
        let audio: Vec<Vec<u8>> = (0..30u32)
            .map(|i| (0..2352).map(|b| (b as u32 ^ i) as u8).collect())
            .collect();
        let data: Vec<Vec<u8>> = (0..40u32)
            .map(|i| {
                let mut d = vec![(i * 3) as u8; 2048];
                // Disc sectors 30, 31 are pregap; the track starts at 32.
                if i == 3 {
                    d[..4].copy_from_slice(&[0, 0, 4, 3]);
                    d[32..55].copy_from_slice(b"PC Engine CD-ROM SYSTEM");
                    d[106..128].copy_from_slice(b"A TEST TITLE FOR ROMDX");
                }
                mode1(30 + i, &d)
            })
            .collect();
        let hash = same_hash_both_ways(
            Console(76),
            &[
                TestTrack {
                    cue_mode: "AUDIO",
                    chd_type: "AUDIO",
                    sectors: audio,
                    pregap: 0,
                },
                TestTrack {
                    cue_mode: "MODE1/2352",
                    chd_type: "MODE1_RAW",
                    sectors: data,
                    pregap: 2,
                },
            ],
        );
        assert_eq!(hash.len(), 32);
    }

    #[test]
    fn a_chd_that_is_not_a_disc_gets_no_hash() {
        let file = tempfile::Builder::new().suffix(".chd").tempfile().unwrap();
        std::fs::write(file.path(), [0u8; 4096]).unwrap();
        assert_eq!(hash_file(Console(39), file.path()), None);
    }

    #[test]
    fn maps_batocera_systems_and_refuses_unknown_ones() {
        assert_eq!(console_for("nes"), Some(NES));
        assert_eq!(console_for("msx2+"), Some(Console(29)));
        assert_eq!(console_for("odyssey2"), Some(Console(23)));
        assert_eq!(console_for("gb2players"), console_for("gb"));
        assert_eq!(console_for("fbneo"), Some(Console::ARCADE));
        assert_eq!(console_for("ports"), None);
        assert_eq!(console_for("NES"), None, "Batocera ids are lower-case");
    }
}
