//! Which ID each source gives a platform, and which libretro thumbnails folder a system uses.
//! Both from tables in this folder (see `docs/scraper.md`).

use std::collections::HashMap;
use std::sync::OnceLock;

/// A platform's IDs at the sources that number them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlatformIds {
    pub screenscraper: Option<u32>,
    pub thegamesdb: Option<u32>,
    pub igdb: Option<u32>,
}

fn table() -> &'static HashMap<&'static str, PlatformIds> {
    static TABLE: OnceLock<HashMap<&'static str, PlatformIds>> = OnceLock::new();
    TABLE.get_or_init(|| {
        include_str!("platforms.tsv")
            .lines()
            .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
            .filter_map(|l| {
                let mut cols = l.split('\t');
                let name = cols.next()?;
                let mut id = || cols.next().and_then(|c| c.trim().parse().ok());
                Some((
                    name,
                    PlatformIds {
                        screenscraper: id(),
                        thegamesdb: id(),
                        igdb: id(),
                    },
                ))
            })
            .collect()
    })
}

/// The IDs for the first of `platforms` each source knows.
pub fn ids(platforms: &[String]) -> PlatformIds {
    let known: Vec<&PlatformIds> = platforms
        .iter()
        .filter_map(|p| table().get(p.trim()))
        .collect();
    PlatformIds {
        screenscraper: known.iter().find_map(|p| p.screenscraper),
        thegamesdb: known.iter().find_map(|p| p.thegamesdb),
        igdb: known.iter().find_map(|p| p.igdb),
    }
}

/// The libretro thumbnails folder for a Batocera system, e.g. `snes` →
/// `Nintendo - Super Nintendo Entertainment System`.
pub fn libretro_folder(system: &str) -> Option<&'static str> {
    static TABLE: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    TABLE
        .get_or_init(|| {
            include_str!("libretro.tsv")
                .lines()
                .filter(|l| !l.starts_with('#'))
                .filter_map(|l| l.split_once('\t'))
                .collect()
        })
        .get(system)
        .copied()
}

fn rows(table: &'static str) -> impl Iterator<Item = (&'static str, &'static str)> {
    table
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_once('\t'))
}

/// What a two-column table says for these `<platform>` names, in the table's order per name.
fn listed(table: &'static str, platforms: &[String]) -> Vec<&'static str> {
    let mut found = Vec::new();
    for platform in platforms {
        for (_, theirs) in rows(table).filter(|(ours, _)| *ours == platform.trim()) {
            if !found.contains(&theirs) {
                found.push(theirs);
            }
        }
    }
    found
}

/// LaunchBox's platforms for these `<platform>` names, the likeliest first (`launchbox.tsv`).
pub fn launchbox(platforms: &[String]) -> Vec<&'static str> {
    listed(include_str!("launchbox.tsv"), platforms)
}

/// HFSDB's system IDs for these `<platform>` names (`hfsdb.tsv`).
pub fn hfsdb(platforms: &[String]) -> Vec<&'static str> {
    listed(include_str!("hfsdb.tsv"), platforms)
}

/// Whether an HFSDB system is not an arcade one: a search for an arcade file can land there.
pub fn hfsdb_is_a_home_system(id: u64) -> bool {
    rows(include_str!("hfsdb.tsv")).any(|(ours, theirs)| {
        !matches!(ours, "arcade" | "neogeo" | "teknoparrot") && theirs.parse() == Ok(id)
    })
}

/// Whether the system is an arcade one (MAME short names).
pub fn is_arcade(system: &str, platforms: &[String]) -> bool {
    platforms.iter().any(|p| p == "arcade" || p == "neogeo")
        || matches!(
            system,
            "mame"
                | "fbneo"
                | "neogeo"
                | "naomi"
                | "naomi2"
                | "atomiswave"
                | "cps1"
                | "cps2"
                | "cps3"
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_up_platforms_and_libretro_folders() {
        let snes = ids(&["snes".to_owned()]);
        assert_eq!(snes.screenscraper, Some(4));
        assert_eq!(snes.thegamesdb, Some(6));
        assert_eq!(snes.igdb, Some(19));
        // Corrected from ES's table.
        assert_eq!(ids(&["psx".to_owned()]).igdb, Some(7));
        // The first platform a source knows.
        let md = ids(&[
            "nope".to_owned(),
            "genesis".to_owned(),
            "megadrive".to_owned(),
        ]);
        assert_eq!(md.screenscraper, Some(1));
        assert_eq!(ids(&[]), PlatformIds::default());
        assert_eq!(
            libretro_folder("snes"),
            Some("Nintendo - Super Nintendo Entertainment System")
        );
        assert_eq!(libretro_folder("ports"), None);
        assert!(is_arcade("mame", &["arcade".to_owned()]));
        assert!(!is_arcade("snes", &["snes".to_owned()]));
    }
}
