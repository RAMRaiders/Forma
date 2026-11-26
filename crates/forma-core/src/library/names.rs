//! How games' names are shown and sorted: as given, or with a leading article or phrase moved to
//! the end (`The Adventures of Batman & Robin` → `Batman & Robin, The Adventures of`), so lists
//! order games by what they are about. The name as given is kept (`game_field`); the style is put
//! over it each time the game is materialised, so changing it back restores every name.

use serde::{Deserialize, Serialize};

/// Forma's setting holding the [`NameRule`], as JSON.
pub const NAME_STYLE_SETTING: &str = "library.nameStyle";

/// What is moved to the end of a game's name.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NameStyle {
    /// As the gamelist or the scraper gave it.
    #[default]
    AsGiven,
    /// A leading article: `Legend of Zelda, The`, as No-Intro and Redump name games.
    ArticleLast,
    /// A leading phrase of [`NameRule::prefixes`], else an article: `Zelda, The Legend of`.
    SubjectFirst,
}

/// The style, and the phrases [`NameStyle::SubjectFirst`] moves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NameRule {
    pub style: NameStyle,
    pub prefixes: Vec<String>,
}

impl Default for NameRule {
    fn default() -> Self {
        Self {
            style: NameStyle::AsGiven,
            prefixes: DEFAULT_PREFIXES.map(str::to_owned).to_vec(),
        }
    }
}

/// What [`NameStyle::SubjectFirst`] moves unless the user changes the list.
pub const DEFAULT_PREFIXES: [&str; 6] = [
    "The Adventures of",
    "The New Adventures of",
    "The Legend of",
    "Adventures of",
    "Legend of",
    "Disney's",
];

/// The articles moved: English only. `Die Hard`, `Las Vegas`, `As Told by…` would go wrong in
/// other languages.
const ARTICLES: [&str; 3] = ["The", "A", "An"];

/// Where a name's subtitle starts, the moved words going before it:
/// `Legend of Zelda, The: A Link to the Past`.
const SUBTITLE: [&str; 3] = [": ", " - ", " ("];

impl NameRule {
    /// The rule stored as `raw` ([`NAME_STYLE_SETTING`]); the default when none or unreadable.
    pub fn load(raw: Option<&str>) -> Self {
        raw.and_then(|r| serde_json::from_str(r).ok())
            .unwrap_or_default()
    }

    /// `name` in this style.
    pub fn apply(&self, name: &str) -> String {
        let phrases: Vec<&str> = match self.style {
            NameStyle::AsGiven => return name.to_owned(),
            NameStyle::ArticleLast => ARTICLES.to_vec(),
            NameStyle::SubjectFirst => {
                let mut longest_first: Vec<&str> = self
                    .prefixes
                    .iter()
                    .map(|p| p.trim())
                    .filter(|p| !p.is_empty())
                    .collect();
                longest_first.sort_by_key(|p| std::cmp::Reverse(p.len()));
                longest_first.extend(ARTICLES);
                longest_first
            }
        };
        let name = name.trim();
        let end = SUBTITLE
            .iter()
            .filter_map(|s| name.find(s))
            .min()
            .unwrap_or(name.len());
        let (title, rest) = name.split_at(end);
        // Already with its article last, as No-Intro names it (`Legend of Zelda, The`): read as
        // `The Legend of Zelda`, so a listed phrase it starts with can be moved too.
        let unmoved = ARTICLES.iter().find_map(|article| {
            let at = title.len().checked_sub(article.len() + 2)?;
            let tail = title.get(at..)?;
            (tail.starts_with(", ") && tail[2..].eq_ignore_ascii_case(article))
                .then(|| format!("{} {}", &tail[2..], &title[..at]))
        });
        let title = unmoved.as_deref().unwrap_or(title);
        for phrase in phrases {
            let Some(head) = title.get(..phrase.len()) else {
                continue;
            };
            let after = &title[phrase.len()..];
            // A whole phrase (`The` in `Theme Park` is not), with words after it.
            if head.eq_ignore_ascii_case(phrase)
                && after.starts_with(' ')
                && !after.trim().is_empty()
            {
                return format!("{}, {head}{rest}", after.trim());
            }
        }
        name.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(style: NameStyle) -> NameRule {
        NameRule {
            style,
            ..NameRule::default()
        }
    }

    #[test]
    fn as_given_leaves_a_name_alone() {
        let r = rule(NameStyle::AsGiven);
        assert_eq!(
            r.apply("The Adventures of Batman & Robin"),
            "The Adventures of Batman & Robin"
        );
    }

    #[test]
    fn article_last_moves_a_leading_article_before_the_subtitle() {
        let r = rule(NameStyle::ArticleLast);
        assert_eq!(
            r.apply("The Adventures of Batman & Robin"),
            "Adventures of Batman & Robin, The"
        );
        assert_eq!(
            r.apply("The Legend of Zelda: A Link to the Past"),
            "Legend of Zelda, The: A Link to the Past"
        );
        assert_eq!(r.apply("A Bug's Life"), "Bug's Life, A");
        assert_eq!(r.apply("the 7th Guest"), "7th Guest, the");
    }

    #[test]
    fn subject_first_moves_the_longest_listed_phrase_else_an_article() {
        let r = rule(NameStyle::SubjectFirst);
        assert_eq!(
            r.apply("The Adventures of Batman & Robin"),
            "Batman & Robin, The Adventures of"
        );
        assert_eq!(
            r.apply("The New Adventures of Robin Hood"),
            "Robin Hood, The New Adventures of"
        );
        assert_eq!(
            r.apply("The Legend of Zelda - Ocarina of Time"),
            "Zelda, The Legend of - Ocarina of Time"
        );
        assert_eq!(r.apply("Disney's Aladdin"), "Aladdin, Disney's");
        assert_eq!(r.apply("The Lost World"), "Lost World, The");
    }

    #[test]
    fn subject_first_uses_the_users_phrases() {
        let r = NameRule {
            style: NameStyle::SubjectFirst,
            prefixes: vec!["Sid Meier's".into()],
        };
        assert_eq!(
            r.apply("Sid Meier's Civilization"),
            "Civilization, Sid Meier's"
        );
        assert_eq!(r.apply("The Legend of Zelda"), "Legend of Zelda, The");
    }

    #[test]
    fn a_name_with_nothing_to_move_or_nothing_after_it_stays() {
        let r = rule(NameStyle::SubjectFirst);
        for name in ["Theme Park", "The", "Anarchy", "Abadox", "A-Train"] {
            assert_eq!(r.apply(name), name);
        }
    }

    #[test]
    fn a_name_with_its_article_last_already_is_read_with_it_first() {
        let last = rule(NameStyle::ArticleLast);
        assert_eq!(last.apply("Legend of Zelda, The"), "Legend of Zelda, The");
        let subject = rule(NameStyle::SubjectFirst);
        assert_eq!(
            subject.apply("Legend of Zelda, The"),
            "Zelda, The Legend of"
        );
        assert_eq!(
            subject.apply("Adventures of Batman & Robin, The (USA)"),
            "Batman & Robin, The Adventures of (USA)"
        );
    }

    #[test]
    fn styling_twice_changes_nothing_more() {
        for style in [NameStyle::ArticleLast, NameStyle::SubjectFirst] {
            let r = rule(style);
            let once = r.apply("The Adventures of Batman & Robin");
            assert_eq!(r.apply(&once), once);
        }
    }

    #[test]
    fn an_unreadable_setting_is_the_default() {
        assert_eq!(NameRule::load(None), NameRule::default());
        assert_eq!(NameRule::load(Some("nonsense")), NameRule::default());
        let stored = r#"{"style":"subjectFirst"}"#;
        assert_eq!(NameRule::load(Some(stored)), rule(NameStyle::SubjectFirst));
    }
}
