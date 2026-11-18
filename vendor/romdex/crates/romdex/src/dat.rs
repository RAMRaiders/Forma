//! No-Intro, Redump and TOSEC DATs: reading them, and telling whether a ROM is a catalogued
//! dump.
//!
//! The DATs come from libretro-database (`metadat/no-intro`, `metadat/redump`, `metadat/tosec`), free to fetch
//! and use; romdex says which files a Batocera system needs ([`dats_for`]) and where they are
//! ([`DatFile::url`]), and the caller fetches and caches them. Both formats libretro-database
//! and the catalogues' own sites use are read: clrmamepro's text and Logiqx XML.
//!
//! A ROM matches on its size and CRC32, confirmed by its SHA1 when the DAT has one: a CRC32
//! alone collides too easily to call a dump verified.

use std::collections::HashMap;

use quick_xml::events::Event;
use quick_xml::events::attributes::Attribute;
use quick_xml::{Reader, XmlVersion};
use serde::Serialize;

use crate::hash::Hashes;

/// Which catalogue a DAT is from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Catalogue {
    NoIntro,
    Redump,
    /// TOSEC: what No-Intro leaves out (pirate carts, multicarts, computers' tapes and disks),
    /// asked after the others.
    Tosec,
}

impl Catalogue {
    /// `no-intro`, `redump`, `tosec`: libretro-database's folder, and the name stored by callers.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoIntro => "no-intro",
            Self::Redump => "redump",
            Self::Tosec => "tosec",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "no-intro" => Some(Self::NoIntro),
            "redump" => Some(Self::Redump),
            "tosec" => Some(Self::Tosec),
            _ => None,
        }
    }

    /// As people write it: `No-Intro`, `Redump`, `TOSEC`.
    pub fn label(self) -> &'static str {
        match self {
            Self::NoIntro => "No-Intro",
            Self::Redump => "Redump",
            Self::Tosec => "TOSEC",
        }
    }
}

/// One DAT of libretro-database: its catalogue and file name without `.dat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DatFile {
    pub catalogue: Catalogue,
    pub name: &'static str,
}

/// Where libretro-database's DATs are served from.
pub const LIBRETRO_BASE: &str =
    "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat";

impl DatFile {
    /// The DAT's address in libretro-database.
    pub fn url(&self) -> String {
        format!(
            "{LIBRETRO_BASE}/{}/{}.dat",
            self.catalogue.as_str(),
            self.name.replace(' ', "%20").replace('+', "%2B")
        )
    }

    /// A file name to cache it under: `no-intro/Nintendo - Game Boy.dat`.
    pub fn cache_name(&self) -> String {
        format!("{}/{}.dat", self.catalogue.as_str(), self.name)
    }
}

const fn ni(name: &'static str) -> DatFile {
    DatFile {
        catalogue: Catalogue::NoIntro,
        name,
    }
}

const fn rd(name: &'static str) -> DatFile {
    DatFile {
        catalogue: Catalogue::Redump,
        name,
    }
}

const fn tc(name: &'static str) -> DatFile {
    DatFile {
        catalogue: Catalogue::Tosec,
        name,
    }
}

/// The DATs whose dumps a Batocera system's folder holds, in the order they are asked: No-Intro
/// or Redump first, TOSEC after, for what they leave out; none for systems no catalogue covers
/// (arcade, computers' disk images…). Folder names as Batocera 43 has them.
pub fn dats_for(system: &str) -> Vec<DatFile> {
    match system {
        "nes" | "famicom" => vec![
            ni("Nintendo - Nintendo Entertainment System"),
            tc("Nintendo - Nintendo Entertainment System"),
        ],
        "fds" => vec![ni("Nintendo - Family Computer Disk System")],
        "snes" | "sfc" => vec![
            ni("Nintendo - Super Nintendo Entertainment System"),
            tc("Nintendo - Super Nintendo Entertainment System"),
        ],
        "satellaview" => vec![ni("Nintendo - Satellaview")],
        "sufami" => vec![ni("Nintendo - Sufami Turbo")],
        "n64" => vec![ni("Nintendo - Nintendo 64")],
        "n64dd" => vec![ni("Nintendo - Nintendo 64DD")],
        "gb" | "sgb" | "gb2players" => vec![ni("Nintendo - Game Boy"), tc("Nintendo - Game Boy")],
        "gbc" | "gbc2players" => {
            vec![
                ni("Nintendo - Game Boy Color"),
                tc("Nintendo - Game Boy Color"),
            ]
        }
        "gba" => vec![
            ni("Nintendo - Game Boy Advance"),
            tc("Nintendo - Game Boy Advance"),
        ],
        "nds" => vec![ni("Nintendo - Nintendo DS"), ni("Nintendo - Nintendo DSi")],
        "3ds" => vec![ni("Nintendo - Nintendo 3DS")],
        "virtualboy" => vec![ni("Nintendo - Virtual Boy")],
        "pokemini" => vec![ni("Nintendo - Pokemon Mini")],
        "gamecube" => vec![rd("Nintendo - GameCube")],
        "wii" => vec![rd("Nintendo - Wii")],
        "megadrive" | "genesis" => vec![
            ni("Sega - Mega Drive - Genesis"),
            tc("Sega - Mega Drive - Genesis"),
        ],
        "mastersystem" => vec![
            ni("Sega - Master System - Mark III"),
            tc("Sega - Master System - Mark III"),
        ],
        "gamegear" => vec![ni("Sega - Game Gear"), tc("Sega - Game Gear")],
        "sg1000" => vec![ni("Sega - SG-1000"), tc("Sega - SG-1000")],
        "sega32x" => vec![ni("Sega - 32X"), tc("Sega - 32X")],
        "pico" => vec![ni("Sega - PICO"), tc("Sega - PICO")],
        "segacd" | "megacd" => vec![rd("Sega - Mega-CD - Sega CD")],
        "saturn" => vec![rd("Sega - Saturn")],
        "dreamcast" => vec![rd("Sega - Dreamcast")],
        "pcengine" => vec![ni("NEC - PC Engine - TurboGrafx 16")],
        "supergrafx" => vec![ni("NEC - PC Engine SuperGrafx")],
        "pcenginecd" => vec![rd("NEC - PC Engine CD - TurboGrafx-CD")],
        "pcfx" => vec![rd("NEC - PC-FX")],
        "pc98" => vec![rd("NEC - PC-98"), tc("NEC - PC-98")],
        "psx" => vec![rd("Sony - PlayStation")],
        "ps2" => vec![rd("Sony - PlayStation 2")],
        "psp" => vec![rd("Sony - PlayStation Portable")],
        "ngp" => vec![ni("SNK - Neo Geo Pocket")],
        "ngpc" => vec![ni("SNK - Neo Geo Pocket Color")],
        "neogeocd" => vec![rd("SNK - Neo Geo CD")],
        "atari2600" => vec![ni("Atari - 2600"), tc("Atari - 2600")],
        "atari5200" => vec![ni("Atari - 5200"), tc("Atari - 5200")],
        "atari7800" => vec![ni("Atari - 7800"), tc("Atari - 7800")],
        "atari800" => vec![ni("Atari - 8-bit Family"), tc("Atari - 8-bit Family")],
        "jaguar" => vec![ni("Atari - Jaguar"), tc("Atari - Jaguar")],
        "jaguarcd" => vec![rd("Atari - Jaguar CD")],
        "lynx" => vec![ni("Atari - Lynx"), tc("Atari - Lynx")],
        "atarist" => vec![ni("Atari - ST"), tc("Atari - ST")],
        "wswan" => vec![ni("Bandai - WonderSwan")],
        "wswanc" => vec![ni("Bandai - WonderSwan Color")],
        "colecovision" => vec![ni("Coleco - ColecoVision")],
        "intellivision" => vec![ni("Mattel - Intellivision"), tc("Mattel - Intellivision")],
        "vectrex" => vec![ni("GCE - Vectrex")],
        "odyssey2" | "o2em" => vec![ni("Magnavox - Odyssey2")],
        "videopacplus" => vec![ni("Philips - Videopac+")],
        "channelf" => vec![ni("Fairchild - Channel F")],
        "arcadia" => vec![ni("Emerson - Arcadia 2001")],
        "advision" => vec![ni("Entex - Adventure Vision")],
        "scv" => vec![ni("Epoch - Super Cassette Vision")],
        "supervision" => vec![ni("Watara - Supervision")],
        "gamecom" => vec![ni("Tiger - Game.com")],
        "crvision" => vec![ni("VTech - CreatiVision")],
        "vsmile" => vec![ni("VTech - V.Smile")],
        "vc4000" => vec![ni("Interton - VC 4000")],
        "gmaster" => vec![ni("Hartung - Game Master")],
        "pv1000" => vec![ni("Casio - PV-1000")],
        "loopy" => vec![ni("Casio - Loopy")],
        "superacan" => vec![ni("Funtech - Super Acan")],
        "studio2" => vec![ni("RCA - Studio II")],
        "arduboy" => vec![ni("Arduboy Inc - Arduboy")],
        "gp32" => vec![ni("GamePark - GP32")],
        "c64" => vec![ni("Commodore - 64")],
        "c20" => vec![ni("Commodore - VIC-20")],
        "cplus4" => vec![ni("Commodore - Plus-4")],
        "amiga500" | "amiga1200" => vec![ni("Commodore - Amiga"), tc("Commodore - Amiga")],
        "amigacd32" => vec![rd("Commodore - CD32")],
        "amigacdtv" => vec![rd("Commodore - CDTV")],
        "msx1" => vec![ni("Microsoft - MSX")],
        "msx2" | "msx2+" => vec![ni("Microsoft - MSX2")],
        "x68000" => vec![ni("Sharp - X68000"), tc("Sharp - X68000")],
        "x1" => vec![ni("Sharp - X1"), tc("Sharp - X1")],
        "3do" => vec![rd("The 3DO Company - 3DO")],
        "cdi" => vec![rd("Philips - CD-i")],
        "fmtowns" => vec![rd("Fujitsu - FM Towns")],
        "xbox" => vec![rd("Microsoft - Xbox")],
        // Only TOSEC lists these.
        "amstradcpc" => vec![tc("Amstrad - CPC")],
        "gx4000" => vec![tc("Amstrad - GX4000")],
        "apple2" => vec![tc("Apple - II")],
        "pet" => vec![tc("Commodore - PET")],
        "pc88" => vec![tc("NEC - PC-88")],
        "zxspectrum" => vec![tc("Sinclair - ZX Spectrum")],
        "spectravideo" => vec![tc("Spectravideo - SVI-318 - SVI-328")],
        "thomson" => vec![tc("Thomson - MOTO")],
        _ => vec![],
    }
}

/// One dump a DAT lists: the file and what identifies it. Hex is lower-case.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Rom {
    pub name: String,
    pub size: Option<u64>,
    pub crc32: Option<String>,
    pub md5: Option<String>,
    pub sha1: Option<String>,
    /// `bad`, `verified`, `baddump`, `nodump`, as some DATs mark it.
    pub status: Option<String>,
}

/// A game of a DAT with one of its files: a game with several files gives several entries.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub game: String,
    pub region: Option<String>,
    pub serial: Option<String>,
    pub rom: Rom,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Dat {
    pub name: Option<String>,
    pub version: Option<String>,
    pub entries: Vec<Entry>,
}

/// Why a DAT could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("not a DAT: {0}")]
pub struct ParseError(pub String);

/// Reads a DAT in either format: Logiqx XML if it starts with `<`, else clrmamepro.
pub fn parse(text: &str) -> Result<Dat, ParseError> {
    let text = text.trim_start_matches('\u{feff}').trim_start();
    if text.starts_with('<') {
        parse_logiqx(text)
    } else {
        parse_clrmamepro(text)
    }
}

/// clrmamepro's tokens: words, quoted strings (with `\"` escapes), and parentheses.
fn tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else if c == '(' || c == ')' {
            out.push(c.to_string());
            chars.next();
        } else if c == '"' {
            chars.next();
            let mut s = String::new();
            while let Some(c) = chars.next() {
                match c {
                    '"' => break,
                    '\\' if chars.peek() == Some(&'"') => s.push(chars.next().unwrap_or('"')),
                    _ => s.push(c),
                }
            }
            // Quoted, even empty: marked so it is never read as a parenthesis.
            out.push(format!("\"{s}"));
        } else {
            let mut s = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() || c == '(' || c == ')' {
                    break;
                }
                s.push(c);
                chars.next();
            }
            out.push(s);
        }
    }
    out
}

fn bare(token: &str) -> &str {
    token.strip_prefix('"').unwrap_or(token)
}

/// `key value` pairs and `key ( … )` blocks up to the matching `)`, from `at` (just inside a
/// block). Returns the pairs, the blocks, and where reading stopped (after the `)`).
type Pairs = Vec<(String, String)>;
type Blocks = Vec<(String, Pairs)>;

fn block(tokens: &[String], mut at: usize) -> (Pairs, Blocks, usize) {
    let mut pairs = Vec::new();
    let mut blocks = Vec::new();
    while at < tokens.len() {
        let key = &tokens[at];
        if key == ")" {
            return (pairs, blocks, at + 1);
        }
        match tokens.get(at + 1).map(String::as_str) {
            Some("(") => {
                let (inner, _, next) = block(tokens, at + 2);
                blocks.push((key.clone(), inner));
                at = next;
            }
            Some(value) => {
                pairs.push((bare(key).to_owned(), bare(value).to_owned()));
                at += 2;
            }
            None => at += 1,
        }
    }
    (pairs, blocks, at)
}

fn get<'a>(pairs: &'a Pairs, key: &str) -> Option<&'a str> {
    pairs
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

fn hex(value: Option<&str>) -> Option<String> {
    let v = value?.trim();
    (!v.is_empty() && v.chars().all(|c| c.is_ascii_hexdigit())).then(|| v.to_ascii_lowercase())
}

fn rom_from(pairs: &Pairs) -> Rom {
    Rom {
        name: get(pairs, "name").unwrap_or_default().to_owned(),
        size: get(pairs, "size").and_then(|s| s.parse().ok()),
        // Some DATs pad the CRC32 short; it is 8 digits everywhere else.
        crc32: hex(get(pairs, "crc")).map(|c| format!("{c:0>8}")),
        md5: hex(get(pairs, "md5")),
        sha1: hex(get(pairs, "sha1")),
        status: get(pairs, "flags")
            .or_else(|| get(pairs, "status"))
            .map(str::to_owned),
    }
}

fn parse_clrmamepro(text: &str) -> Result<Dat, ParseError> {
    let tokens = tokens(text);
    let mut dat = Dat::default();
    let mut at = 0;
    let mut games = 0;
    while at < tokens.len() {
        let kind = tokens[at].as_str();
        if tokens.get(at + 1).map(String::as_str) != Some("(") {
            at += 1;
            continue;
        }
        let (pairs, blocks, next) = block(&tokens, at + 2);
        at = next;
        match kind {
            "clrmamepro" => {
                dat.name = get(&pairs, "name").map(str::to_owned);
                dat.version = get(&pairs, "version").map(str::to_owned);
            }
            "game" | "machine" | "resource" => {
                games += 1;
                let game = get(&pairs, "name").unwrap_or_default().to_owned();
                for (key, rom) in &blocks {
                    if key != "rom" {
                        continue;
                    }
                    let rom_serial = get(rom, "serial");
                    dat.entries.push(Entry {
                        game: game.clone(),
                        region: get(&pairs, "region").map(str::to_owned),
                        serial: get(&pairs, "serial").or(rom_serial).map(str::to_owned),
                        rom: rom_from(rom),
                    });
                }
            }
            _ => {}
        }
    }
    if games == 0 && dat.name.is_none() {
        return Err(ParseError("no clrmamepro header and no games".into()));
    }
    Ok(dat)
}

fn value_of(a: &Attribute) -> String {
    a.normalized_value(XmlVersion::Implicit1_0)
        .map(|v| v.into_owned())
        .unwrap_or_default()
}

fn parse_logiqx(text: &str) -> Result<Dat, ParseError> {
    let mut reader = Reader::from_str(text);
    let mut dat = Dat::default();
    let mut game: Option<(String, Option<String>)> = None;
    // Text of the element being read, under <header> or <game>.
    let mut path: Vec<String> = Vec::new();
    let mut seen_root = false;
    loop {
        let event = reader
            .read_event()
            .map_err(|e| ParseError(format!("XML: {e}")))?;
        match event {
            Event::Start(e) | Event::Empty(e) if matches!(e.name().as_ref(), "rom" | "disk") => {
                if let Some((name, region)) = &game {
                    let mut pairs = Pairs::new();
                    for a in e.attributes().flatten() {
                        let key = a.key.as_ref().to_owned();
                        pairs.push((key, value_of(&a)));
                    }
                    dat.entries.push(Entry {
                        game: name.clone(),
                        region: region.clone(),
                        serial: get(&pairs, "serial").map(str::to_owned),
                        rom: rom_from(&pairs),
                    });
                }
            }
            Event::Start(e) => {
                let tag = e.name().as_ref().to_owned();
                if tag == "datafile" {
                    seen_root = true;
                }
                if tag == "game" || tag == "machine" {
                    let name = e
                        .try_get_attribute("name")
                        .ok()
                        .flatten()
                        .map(|a| value_of(&a))
                        .unwrap_or_default();
                    game = Some((name, None));
                }
                path.push(tag);
            }
            Event::End(e) => {
                if matches!(e.name().as_ref(), "game" | "machine") {
                    game = None;
                }
                path.pop();
            }
            Event::Text(t) => {
                let value = t.xml10_content().trim().to_owned();
                if value.is_empty() {
                    continue;
                }
                let at: Vec<&str> = path.iter().map(String::as_str).collect();
                match at.as_slice() {
                    [.., "header", "name"] => dat.name = Some(value),
                    [.., "header", "version"] => dat.version = Some(value),
                    [.., "game" | "machine", "region"] => {
                        if let Some((_, region)) = &mut game {
                            *region = Some(value);
                        }
                    }
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !seen_root {
        return Err(ParseError("no <datafile>".into()));
    }
    Ok(dat)
}

/// How sure a match is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Confidence {
    /// Size, CRC32 and SHA1 all as the DAT says.
    Sha1,
    /// Size and CRC32 as the DAT says, which has no SHA1 for it.
    Crc32,
}

/// A ROM found in a DAT.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Match<'a> {
    pub catalogue: Catalogue,
    /// The DAT's name for the game, e.g. `Super Mario Bros. (World)`.
    pub game: &'a str,
    pub region: Option<&'a str>,
    pub confidence: Confidence,
    /// The DAT marks the dump bad (`baddump`, `bad`).
    pub bad: bool,
}

/// The DATs of a system, looked up by size and CRC32.
#[derive(Debug, Default)]
pub struct Index {
    entries: Vec<(Catalogue, Entry)>,
    by_crc: HashMap<(u64, String), Vec<usize>>,
    by_sha1: HashMap<String, Vec<usize>>,
}

impl Index {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a DAT's dumps.
    pub fn add(&mut self, catalogue: Catalogue, dat: Dat) {
        for entry in dat.entries {
            let at = self.entries.len();
            if let (Some(size), Some(crc)) = (entry.rom.size, &entry.rom.crc32) {
                self.by_crc.entry((size, crc.clone())).or_default().push(at);
            }
            if let Some(sha1) = &entry.rom.sha1 {
                self.by_sha1.entry(sha1.clone()).or_default().push(at);
            }
            self.entries.push((catalogue, entry));
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The dump with this size and these hashes, if a DAT lists it. A DAT entry with a SHA1
    /// that differs is not a match, however the CRC32 agrees.
    pub fn lookup(&self, size: u64, hashes: &Hashes) -> Option<Match<'_>> {
        self.lookup_exact(size, hashes).or_else(|| {
            // Not as it is: perhaps without its header, as DATs list most headered ROMs.
            let bare = hashes.headerless.as_ref()?;
            let without = Hashes {
                crc32: bare.crc32.clone(),
                md5: bare.md5.clone(),
                sha1: bare.sha1.clone(),
                headerless: None,
            };
            self.lookup_exact(size.checked_sub(bare.header)?, &without)
        })
    }

    fn lookup_exact(&self, size: u64, hashes: &Hashes) -> Option<Match<'_>> {
        let crc = hashes.crc32.to_ascii_lowercase();
        let sha1 = hashes.sha1.to_ascii_lowercase();
        let found = |at: usize, confidence| {
            let (catalogue, e) = &self.entries[at];
            let bad = e
                .rom
                .status
                .as_deref()
                .is_some_and(|s| s.contains("baddump") || s == "bad");
            Match {
                catalogue: *catalogue,
                game: &e.game,
                region: e.region.as_deref(),
                confidence,
                bad,
            }
        };
        let candidates = self.by_crc.get(&(size, crc));
        if let Some(at) = candidates
            .into_iter()
            .flatten()
            .find(|&&at| self.entries[at].1.rom.sha1.as_deref() == Some(sha1.as_str()))
        {
            return Some(found(*at, Confidence::Sha1));
        }
        if let Some(at) = candidates
            .into_iter()
            .flatten()
            .find(|&&at| self.entries[at].1.rom.sha1.is_none())
        {
            return Some(found(*at, Confidence::Crc32));
        }
        // A DAT without sizes still lists the SHA1.
        self.by_sha1
            .get(&sha1)
            .and_then(|ats| {
                ats.iter()
                    .find(|&&at| self.entries[at].1.rom.size.is_none_or(|s| s == size))
            })
            .map(|&at| found(at, Confidence::Sha1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLRMAMEPRO: &str = r#"clrmamepro (
	name "Nintendo - Nintendo Entertainment System"
	description "Nintendo - Nintendo Entertainment System"
	version "2026.10.07"
)

game (
	name "Super Mario Bros. (World)"
	region "World"
	rom ( name "Super Mario Bros. (World).nes" size 40976 crc 3337EC46 md5 811B027EAF99C2DEF7B933C5208636DE sha1 EA343F4E445A9050D4B4FBAC2C77D0693B1D0922 )
)
game (
	name "Only a CRC (USA)"
	rom ( name "Only a CRC (USA).nes" size 16 crc 0000ABCD )
)
game (
	name "Say \"Hi\" (Japan) [b]"
	rom ( name "Say \"Hi\" (Japan) [b].nes" size 32 crc 12345678 sha1 1111111111111111111111111111111111111111 flags baddump )
)
"#;

    fn hashes(crc32: &str, sha1: &str) -> Hashes {
        Hashes {
            crc32: crc32.into(),
            md5: String::new(),
            sha1: sha1.into(),
            headerless: None,
        }
    }

    #[test]
    fn reads_clrmamepro_with_quotes_inside_names() {
        let dat = parse(CLRMAMEPRO).unwrap();
        assert_eq!(dat.version.as_deref(), Some("2026.10.07"));
        assert_eq!(dat.entries.len(), 3);
        let mario = &dat.entries[0];
        assert_eq!(mario.game, "Super Mario Bros. (World)");
        assert_eq!(mario.region.as_deref(), Some("World"));
        assert_eq!(mario.rom.size, Some(40976));
        assert_eq!(mario.rom.crc32.as_deref(), Some("3337ec46"));
        assert_eq!(dat.entries[2].game, "Say \"Hi\" (Japan) [b]");
        assert_eq!(dat.entries[2].rom.status.as_deref(), Some("baddump"));
    }

    #[test]
    fn reads_logiqx() {
        let xml = r#"<?xml version="1.0"?>
<!DOCTYPE datafile PUBLIC "-//Logiqx//DTD ROM Management Datafile//EN" "http://www.logiqx.com/dtds/datafile.dtd">
<datafile>
	<header><name>Sega - Mega Drive - Genesis</name><version>20261001</version></header>
	<game name="Sonic &amp; Knuckles (World)">
		<description>Sonic &amp; Knuckles (World)</description>
		<region>World</region>
		<rom name="Sonic &amp; Knuckles (World).md" size="2097152" crc="0658f691" sha1="88d6499d874dcb5721ff58d76fe1b9af811192e3" status="verified"/>
	</game>
</datafile>"#;
        let dat = parse(xml).unwrap();
        assert_eq!(dat.name.as_deref(), Some("Sega - Mega Drive - Genesis"));
        assert_eq!(dat.entries.len(), 1);
        assert_eq!(dat.entries[0].game, "Sonic & Knuckles (World)");
        assert_eq!(dat.entries[0].region.as_deref(), Some("World"));
        assert_eq!(dat.entries[0].rom.size, Some(2_097_152));
    }

    #[test]
    fn what_is_not_a_dat_says_so() {
        assert!(parse("hello").is_err());
        assert!(parse("<html></html>").is_err());
    }

    #[test]
    fn matches_on_size_and_crc_confirmed_by_sha1() {
        let mut index = Index::new();
        index.add(Catalogue::NoIntro, parse(CLRMAMEPRO).unwrap());
        let sha1 = "ea343f4e445a9050d4b4fbac2c77d0693b1d0922";
        let m = index.lookup(40976, &hashes("3337EC46", sha1)).unwrap();
        assert_eq!(m.game, "Super Mario Bros. (World)");
        assert_eq!(m.catalogue, Catalogue::NoIntro);
        assert_eq!(m.confidence, Confidence::Sha1);
        assert!(!m.bad);
        // Same CRC32, another SHA1: a collision or a changed file, not the dump.
        assert_eq!(
            index.lookup(40976, &hashes("3337ec46", &"0".repeat(40))),
            None
        );
        // Another size: not it.
        assert_eq!(index.lookup(40960, &hashes("3337ec46", sha1)), None);
        // The DAT has no SHA1: the CRC32 is all there is to go on.
        let crc = index
            .lookup(16, &hashes("0000abcd", &"0".repeat(40)))
            .unwrap();
        assert_eq!(crc.confidence, Confidence::Crc32);
        // A bad dump is still that dump, marked.
        let bad = index
            .lookup(32, &hashes("12345678", &"1".repeat(40)))
            .unwrap();
        assert!(bad.bad);
        // A file with a header the DAT lists it without: found by what follows the header.
        let mut headered = hashes("deadbeef", &"2".repeat(40));
        headered.headerless = Some(crate::hash::Headerless {
            header: 128,
            crc32: "3337ec46".into(),
            md5: String::new(),
            sha1: sha1.into(),
        });
        let m = index.lookup(40976 + 128, &headered).unwrap();
        assert_eq!(m.game, "Super Mario Bros. (World)");
        assert_eq!(m.confidence, Confidence::Sha1);
    }

    #[test]
    fn systems_have_their_dats_and_addresses() {
        assert_eq!(dats_for("nes")[0].catalogue, Catalogue::NoIntro);
        assert_eq!(dats_for("psx")[0].catalogue, Catalogue::Redump);
        assert!(dats_for("mame").is_empty());
        assert_eq!(
            dats_for("gb")[0].url(),
            "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat/no-intro/Nintendo%20-%20Game%20Boy.dat"
        );
        assert_eq!(
            dats_for("videopacplus")[0].url(),
            "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat/no-intro/Philips%20-%20Videopac%2B.dat"
        );
        // TOSEC after No-Intro, for what No-Intro leaves out; alone where it is all there is.
        let atari: Vec<_> = dats_for("atari2600").iter().map(|d| d.catalogue).collect();
        assert_eq!(atari, [Catalogue::NoIntro, Catalogue::Tosec]);
        assert_eq!(
            dats_for("atari2600")[1].url(),
            "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat/tosec/Atari%20-%202600.dat"
        );
        assert_eq!(dats_for("zxspectrum")[0].catalogue, Catalogue::Tosec);
        assert_eq!(Catalogue::parse("tosec"), Some(Catalogue::Tosec));
        assert_eq!(Catalogue::Tosec.label(), "TOSEC");
    }

    #[test]
    fn the_first_catalogue_to_list_a_dump_names_it() {
        let mut index = Index::new();
        index.add(Catalogue::NoIntro, parse(CLRMAMEPRO).unwrap());
        index.add(Catalogue::Tosec, parse(CLRMAMEPRO).unwrap());
        let sha1 = "ea343f4e445a9050d4b4fbac2c77d0693b1d0922";
        let m = index.lookup(40976, &hashes("3337EC46", sha1)).unwrap();
        assert_eq!(m.catalogue, Catalogue::NoIntro);
    }
}
