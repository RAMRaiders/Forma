//! Names, matching and the small text work every source needs.

/// A name to search with: `Super Metroid (USA, Europe) [!]` → `Super Metroid`.
pub fn search_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut depth = 0usize;
    for c in name.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    let out = out.replace('_', " ");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// For comparing names: lower case, letters and digits only, a leading or trailing "the"
/// dropped (`Legend of Zelda, The`).
pub fn comparable(name: &str) -> String {
    let lower = search_name(name).to_lowercase();
    let lower = lower
        .strip_suffix(", the")
        .or_else(|| lower.strip_prefix("the "))
        .unwrap_or(&lower)
        .to_owned();
    lower.chars().filter(|c| c.is_alphanumeric()).collect()
}

/// For finding one game under its many spellings: `Streets of Rage II (USA)` and `Streets of
/// Rage 2`, `Sonic & Knuckles` and `Sonic and Knuckles`, `Legend of Zelda, The - A Link` and
/// `The Legend of Zelda: A Link`, are the same. A sequel stays another game.
pub fn title_key(name: &str) -> String {
    let name = search_name(name);
    let lower = name.to_lowercase();
    let name = match lower.find(", the ") {
        Some(at) if name.is_char_boundary(at) && lower.len() == name.len() => {
            format!("{} {}", &name[..at], &name[at + ", the ".len()..])
        }
        _ => name,
    };
    let words: Vec<&str> = name
        .split_whitespace()
        .map(|word| {
            match word
                .trim_matches(|c: char| !c.is_alphanumeric() && c != '&')
                .to_lowercase()
                .as_str()
            {
                "ii" => "2",
                "iii" => "3",
                "iv" => "4",
                "&" => "and",
                _ => word,
            }
        })
        .collect();
    comparable(&words.join(" "))
}

/// The index of the candidate whose name matches `name` best: the same once compared, else
/// one starting with it, else none.
pub fn best_match<'a>(name: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<usize> {
    let want = comparable(name);
    if want.is_empty() {
        return None;
    }
    let names: Vec<String> = candidates.into_iter().map(comparable).collect();
    names.iter().position(|n| *n == want).or_else(|| {
        names
            .iter()
            .position(|n| n.starts_with(&want) || want.starts_with(n.as_str()) && !n.is_empty())
    })
}

/// Percent-encodes `text` for a URL's path or query.
pub fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 3);
    for b in text.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Text with HTML's character codes (`&quot;`, `&#039;`, `&eacute;`…) as the characters they
/// stand for, as some sources send their text. Twice over for text escaped twice
/// (`&amp;quot;`). Anything else with `&` stays as it is.
pub fn decode_entities(text: &str) -> String {
    let once = decode_once(text);
    if once.contains('&') && once != text {
        decode_once(&once)
    } else {
        once
    }
}

fn decode_once(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let decoded = after
            .find(';')
            .filter(|&end| end > 0 && end <= 10)
            .and_then(|end| entity(&after[..end]).map(|c| (c, end)));
        match decoded {
            Some((c, end)) => {
                out.push(c);
                rest = &after[end + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The character `&name;` stands for.
fn entity(name: &str) -> Option<char> {
    if let Some(number) = name.strip_prefix('#') {
        let code = match number.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => number.parse().ok()?,
        };
        return char::from_u32(code).filter(|c| !c.is_control() || *c == '\n');
    }
    Some(match name {
        "quot" => '"',
        "amp" => '&',
        "apos" => '\'',
        "lt" => '<',
        "gt" => '>',
        "nbsp" => ' ',
        "hellip" => '…',
        "mdash" => '—',
        "ndash" => '–',
        "lsquo" => '‘',
        "rsquo" => '’',
        "ldquo" => '“',
        "rdquo" => '”',
        "laquo" => '«',
        "raquo" => '»',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        "deg" => '°',
        "eacute" => 'é',
        "egrave" => 'è',
        "ecirc" => 'ê',
        "euml" => 'ë',
        "aacute" => 'á',
        "agrave" => 'à',
        "acirc" => 'â',
        "auml" => 'ä',
        "atilde" => 'ã',
        "aring" => 'å',
        "iacute" => 'í',
        "icirc" => 'î',
        "iuml" => 'ï',
        "oacute" => 'ó',
        "ocirc" => 'ô',
        "ouml" => 'ö',
        "otilde" => 'õ',
        "uacute" => 'ú',
        "ucirc" => 'û',
        "uuml" => 'ü',
        "ccedil" => 'ç',
        "ntilde" => 'ñ',
        "szlig" => 'ß',
        "Eacute" => 'É',
        "Agrave" => 'À',
        "Ccedil" => 'Ç',
        "Ouml" => 'Ö',
        "Uuml" => 'Ü',
        "Auml" => 'Ä',
        _ => return None,
    })
}

/// Text from HTML: tags dropped, paragraphs and breaks as new lines, entities decoded.
pub fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(lt) = rest.find('<') {
        out.push_str(&rest[..lt]);
        let Some(gt) = rest[lt..].find('>') else {
            rest = "";
            break;
        };
        let tag = rest[lt + 1..lt + gt].trim_start_matches('/').to_lowercase();
        if tag.starts_with("br") || tag.starts_with('p') || tag.starts_with("li") {
            out.push('\n');
        }
        rest = &rest[lt + gt + 1..];
    }
    out.push_str(rest);
    let out = decode_entities(&out);
    let lines: Vec<&str> = out.lines().map(str::trim).collect();
    let mut joined = lines.join("\n");
    while joined.contains("\n\n\n") {
        joined = joined.replace("\n\n\n", "\n\n");
    }
    joined.trim().to_owned()
}

/// A date as a source gives it (`1994-03-19`, `1994`, `19940319`…) as `YYYY[-MM[-DD]]`, or
/// `None` if it is not one.
pub fn date(text: &str) -> Option<String> {
    let digits: String = text.chars().filter(char::is_ascii_digit).collect();
    let year = digits.get(..4)?;
    if !(1950..=2100).contains(&year.parse::<u32>().ok()?) {
        return None;
    }
    let month = digits.get(4..6).filter(|m| ("01"..="12").contains(m));
    let day = digits.get(6..8).filter(|d| ("01"..="31").contains(d));
    Some(match (month, day) {
        (Some(m), Some(d)) => format!("{year}-{m}-{d}"),
        (Some(m), None) => format!("{year}-{m}"),
        _ => year.to_owned(),
    })
}

/// Seconds since 1970 as `YYYY-MM-DD`.
pub fn date_from_unix(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// A JSON value as text: strings as they are, numbers written out, anything else `None`.
/// Empty text is `None` too.
pub fn json_text(v: Option<&serde_json::Value>) -> Option<String> {
    let text = match v? {
        serde_json::Value::String(s) => s.trim().to_owned(),
        serde_json::Value::Number(n) => n.to_string(),
        _ => return None,
    };
    (!text.is_empty()).then_some(text)
}

/// A JSON number, or a number in a string.
pub fn json_number(v: Option<&serde_json::Value>) -> Option<f64> {
    match v? {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// The boards whose ratings Forma keeps, in the order a game's are listed.
const AGE_BOARDS: &[&str] = &[
    "ESRB", "PEGI", "CERO", "USK", "ACB", "GRAC", "CLASSIND", "BBFC",
];

/// Age ratings as `(board, rating)` pairs, from any source, as Forma keeps them: `ESRB:M,PEGI:18`.
/// Known boards only, each once, in [`AGE_BOARDS`] order; a rating as its short form (`M` from
/// TheGamesDB's `M - Mature 17+`). Pending or missing ratings are left out. `None` for none.
pub(crate) fn age_ratings<'a>(
    pairs: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Option<String> {
    let mut kept: Vec<(usize, String)> = Vec::new();
    for (board, rating) in pairs {
        let board = board
            .trim()
            .to_ascii_uppercase()
            .replace([' ', '-', '_'], "");
        let board = if board == "CLASSINDBR" {
            "CLASSIND".to_owned()
        } else {
            board
        };
        let Some(at) = AGE_BOARDS.iter().position(|b| *b == board) else {
            continue;
        };
        let short = rating
            .split(" - ")
            .next()
            .unwrap_or("")
            .trim()
            .trim_start_matches(&format!("{board} "))
            .to_ascii_uppercase();
        let pending = ["", "RP", "NR", "NOT RATED", "N/A", "PENDING"];
        if pending.contains(&short.as_str())
            || short.len() > 6
            || kept.iter().any(|(b, _)| *b == at)
        {
            continue;
        }
        kept.push((at, format!("{}:{short}", AGE_BOARDS[at])));
    }
    kept.sort_by_key(|(at, _)| *at);
    (!kept.is_empty()).then(|| {
        kept.into_iter()
            .map(|(_, r)| r)
            .collect::<Vec<_>>()
            .join(",")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_codes_become_the_characters_they_stand_for() {
        assert_eq!(
            decode_entities("a &quot;Family Game&quot; mode, Sega&#039;s &amp; Namco&#x27;s"),
            "a \"Family Game\" mode, Sega's & Namco's"
        );
        assert_eq!(decode_entities("Pok&eacute;mon &hellip;"), "Pokémon …");
        // Escaped twice by a source.
        assert_eq!(decode_entities("&amp;quot;Hi&amp;quot;"), "\"Hi\"");
        // Not a code: left alone.
        assert_eq!(
            decode_entities("Tom & Jerry; R&D &unknown; &"),
            "Tom & Jerry; R&D &unknown; &"
        );
        assert_eq!(decode_entities("plain"), "plain");
    }

    #[test]
    fn names_lose_their_tags_and_match_loosely() {
        assert_eq!(
            search_name("Super Metroid (USA, Europe) [!]"),
            "Super Metroid"
        );
        assert_eq!(search_name("sonic_the_hedgehog"), "sonic the hedgehog");
        assert_eq!(
            comparable("Legend of Zelda, The - A Link to the Past"),
            "legendofzeldathealinktothepast"
        );
        assert_eq!(comparable("Legend of Zelda, The"), "legendofzelda");
        assert_eq!(
            best_match(
                "Street Fighter II",
                ["Street Fighter", "Street Fighter II", "SF II"]
            ),
            Some(1)
        );
        assert_eq!(
            best_match("Contra", ["Contra III: The Alien Wars"]),
            Some(0)
        );
        assert_eq!(best_match("Contra", ["Gradius"]), None);
    }

    #[test]
    fn dates_html_and_urls() {
        assert_eq!(date("1994-03-19").as_deref(), Some("1994-03-19"));
        assert_eq!(date("1991").as_deref(), Some("1991"));
        assert_eq!(date("19910201T000000").as_deref(), Some("1991-02-01"));
        assert_eq!(date("unknown"), None);
        assert_eq!(date_from_unix(765_158_400), "1994-04-01");
        assert_eq!(
            strip_html("<p>One &amp; two</p><p>Three<br>four</p>"),
            "One & two\n\nThree\nfour"
        );
        assert_eq!(
            encode("Mario & Luigi: 1/2"),
            "Mario%20%26%20Luigi%3A%201%2F2"
        );
    }
}
