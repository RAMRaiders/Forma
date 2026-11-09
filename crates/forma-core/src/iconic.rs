//! Which games are iconic: the ones a system is remembered for, from lists Forma ships,
//! matched by name so it needs no scraper and no network.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::OnceLock;

use crate::scraper::title_key as key;

/// Each system's list: a game per line, `|` between other titles of it, `#` for a comment.
const LISTS: &[(&str, &str)] = &[("megadrive", include_str!("iconic/megadrive.txt"))];

fn lists() -> &'static HashMap<&'static str, HashSet<String>> {
    static BUILT: OnceLock<HashMap<&'static str, HashSet<String>>> = OnceLock::new();
    BUILT.get_or_init(|| {
        LISTS
            .iter()
            .map(|(system, text)| {
                let titles = text
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty() && !line.starts_with('#'))
                    .flat_map(|line| line.split('|'))
                    .map(key)
                    .collect();
                (*system, titles)
            })
            .collect()
    })
}

/// Whether a game of `system`, with this name and ROM path, is on the system's list.
pub fn is_iconic(system: &str, name: &str, path: &str) -> bool {
    let Some(list) = lists().get(system) else {
        return false;
    };
    let stem = Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    list.contains(&key(name)) || list.contains(&key(&stem))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_whatever_the_set_calls_it() {
        for (name, path) in [
            ("Sonic the Hedgehog", "x.md"),
            ("Sonic The Hedgehog (USA, Europe)", "x.md"),
            ("Streets of Rage II", "x.md"),
            (
                "Bare Knuckle II - Shitou e no Chinkonka (Japan)",
                "Streets of Rage 2 (USA).md",
            ),
            ("Sonic and Knuckles", "x.md"),
            ("Sonic & Knuckles (World)", "x.md"),
            ("unknown", "Gunstar Heroes (USA).zip"),
            ("Revenge of Shinobi, The", "x.md"),
            ("Castlevania - Bloodlines (USA)", "x.md"),
        ] {
            assert!(is_iconic("megadrive", name, path), "{name} / {path}");
        }
    }

    #[test]
    fn a_sequel_or_another_system_does_not_count() {
        assert!(!is_iconic("megadrive", "Golden Axe III (Japan)", "x.md"));
        assert!(!is_iconic("megadrive", "Some Homebrew", "x.md"));
        assert!(!is_iconic("snes", "Sonic the Hedgehog", "x.sfc"));
    }

    #[test]
    fn every_list_is_sound() {
        for (system, text) in LISTS {
            let lines = text
                .lines()
                .filter(|l| !l.trim().is_empty() && !l.starts_with('#'));
            for line in lines {
                for title in line.split('|') {
                    assert!(!key(title).is_empty(), "{system}: {line}");
                }
            }
        }
    }
}
