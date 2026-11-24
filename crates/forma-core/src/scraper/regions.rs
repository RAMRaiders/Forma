//! Which region's names, boxes and dates a game gets: the user's profile, in order.

/// The regions a user can prefer, as No-Intro names them in a ROM's file name.
pub const REGIONS: [&str; 17] = [
    "World",
    "USA",
    "Europe",
    "Japan",
    "UK",
    "France",
    "Germany",
    "Spain",
    "Italy",
    "Netherlands",
    "Sweden",
    "Portugal",
    "Brazil",
    "Canada",
    "Australia",
    "Korea",
    "Asia",
];

/// Before the user says otherwise: anybody can read these.
pub fn default_order() -> Vec<String> {
    ["World", "USA", "Europe"].map(str::to_owned).to_vec()
}

/// The regions in a ROM's name, `Sonic (USA, Europe) (Rev 1)`, in the order they come.
pub fn in_name(stem: &str) -> Vec<&'static str> {
    let mut found = Vec::new();
    for word in stem.split(['(', ')', ',']).map(str::trim) {
        if let Some(r) = REGIONS.iter().find(|r| **r == word)
            && !found.contains(r)
        {
            found.push(*r);
        }
    }
    found
}

/// A region's code as gamelists give it (`us`, `eu`, `wor`), as EmulationStation and themes
/// read a game's `region`.
pub fn code(region: &str) -> Option<&'static str> {
    Some(match region {
        "World" => "wor",
        "USA" => "us",
        "Europe" => "eu",
        "Japan" => "jp",
        "UK" => "uk",
        "France" => "fr",
        "Germany" => "de",
        "Spain" => "es",
        "Italy" => "it",
        "Netherlands" => "nl",
        "Sweden" => "se",
        "Portugal" => "pt",
        "Brazil" => "br",
        "Canada" => "ca",
        "Australia" => "au",
        "Korea" => "kr",
        "Asia" => "asi",
        _ => return None,
    })
}

/// A playlist's region, from its discs' names when its own gives none: one code when they agree
/// (`us`), each once when they do not (`us,eu`). `None` when its name gives one, or no disc does.
pub fn of_playlist(stem: &str, discs: &[&str]) -> Option<String> {
    if !in_name(stem).is_empty() {
        return None;
    }
    let mut codes: Vec<&str> = Vec::new();
    for disc in discs {
        for c in in_name(disc).into_iter().filter_map(code) {
            if !codes.contains(&c) {
                codes.push(c);
            }
        }
    }
    (!codes.is_empty()).then(|| codes.join(","))
}

/// The regions to look for, best first. With `own`, the ROM's own regions come first: of
/// several (`USA, Europe`), the one `preferred` puts first wins. Then `preferred`, in order.
pub fn order(stem: &str, own: bool, preferred: &[String]) -> Vec<String> {
    let rank = |r: &str| preferred.iter().position(|p| p == r).unwrap_or(usize::MAX);
    let mut mine = if own { in_name(stem) } else { Vec::new() };
    // Stable: regions the user did not rank keep the name's order.
    mine.sort_by_key(|r| rank(r));
    let mut all: Vec<String> = mine.into_iter().map(str::to_owned).collect();
    for r in preferred {
        if REGIONS.contains(&r.as_str()) && !all.contains(r) {
            all.push(r.clone());
        }
    }
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn a_playlist_without_a_region_takes_its_discs() {
        let same = ["Alice (USA) (Disc 1) (Unl)", "Alice (USA) (Disc 2) (Unl)"];
        assert_eq!(of_playlist("Alice", &same).as_deref(), Some("us"));
        let mixed = [
            "Game (USA) (Disc 1)",
            "Game (Europe) (Disc 2)",
            "Game (USA, Japan) (Disc 3)",
        ];
        assert_eq!(of_playlist("Game", &mixed).as_deref(), Some("us,eu,jp"));
        assert_eq!(
            of_playlist("Game (Japan)", &mixed),
            None,
            "its own name says"
        );
        assert_eq!(
            of_playlist("Game", &["Game (Disc 1)"]),
            None,
            "no disc says"
        );
    }

    #[test]
    fn the_roms_own_region_first_and_of_several_the_preferred_one() {
        let preferred = list(&["Europe", "USA", "Japan"]);
        assert_eq!(
            order("Sonic (USA, Europe) (Rev 1)", true, &preferred),
            ["Europe", "USA", "Japan"]
        );
        assert_eq!(
            order("Mother (Japan)", true, &preferred),
            ["Japan", "Europe", "USA"]
        );
    }

    #[test]
    fn a_country_of_ones_own_over_the_roms() {
        let preferred = list(&["France", "Europe"]);
        assert_eq!(
            order("Sonic (USA)", false, &preferred),
            ["France", "Europe"]
        );
    }

    #[test]
    fn what_forma_does_not_know_is_left_out() {
        assert_eq!(order("Tetris", true, &list(&["Mars", "USA"])), ["USA"]);
        assert_eq!(in_name("Game (Europe) (En,Fr,De)"), ["Europe"]);
    }
}
