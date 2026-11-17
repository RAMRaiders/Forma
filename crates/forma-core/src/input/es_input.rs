//! Controllers EmulationStation knows, for the gamepad reader: `es_input.cfg` turned into SDL
//! mappings.
//!
//! gilrs ships SDL's controller database; a pad it does not list moved nothing in Forma, though
//! it worked in ES. Batocera's own `es_input.cfg` and the one the user's ES configuration writes
//! fill that gap. They are added before gilrs' database, which wins for a pad it lists: the
//! standard layout stays for known pads, and ES's for the others. (Overriding a known pad would
//! need SDL's environment variable set, which Forma's no-`unsafe` rule leaves out.)
//!
//! ES names buttons by their place on a Nintendo pad (`a` on the right, `b` at the bottom), SDL
//! by an Xbox pad's (`a` at the bottom): `a`/`b` and `x`/`y` swap. Checked against SDL's own
//! entry for the Xbox 360 pad, which Batocera's file also lists.

use std::path::Path;

use crate::xml;

/// SDL's name for each ES input, `None` where it has none.
fn sdl_name(es: &str) -> Option<&'static str> {
    Some(match es {
        "a" => "b",
        "b" => "a",
        "x" => "y",
        "y" => "x",
        "start" => "start",
        "select" => "back",
        "hotkey" => "guide",
        "pageup" => "leftshoulder",
        "pagedown" => "rightshoulder",
        "l2" => "lefttrigger",
        "r2" => "righttrigger",
        "l3" => "leftstick",
        "r3" => "rightstick",
        "up" => "dpup",
        "down" => "dpdown",
        "left" => "dpleft",
        "right" => "dpright",
        "joystick1left" => "leftx",
        "joystick1up" => "lefty",
        "joystick2left" => "rightx",
        "joystick2up" => "righty",
        _ => return None,
    })
}

/// One input's SDL binding: `b3`, `h0.4`, `a1`, `a1~` (an axis the other way round), `-a7` (half
/// an axis, for a d-pad direction on one).
fn binding(es: &str, kind: &str, id: &str, value: &str) -> Option<String> {
    let id: u32 = id.parse().ok()?;
    let value: i32 = value.parse().ok()?;
    match kind {
        "button" => Some(format!("b{id}")),
        "hat" => Some(format!("h{id}.{value}")),
        "axis" => Some(match es {
            // A stick's left or up end: negative on most pads, positive where it is turned.
            n if n.starts_with("joystick") => {
                if value < 0 {
                    format!("a{id}")
                } else {
                    format!("a{id}~")
                }
            }
            "up" | "down" | "left" | "right" => {
                format!("{}a{id}", if value < 0 { '-' } else { '+' })
            }
            _ => format!("a{id}"),
        }),
        _ => None,
    }
}

/// The SDL mapping lines for every joystick in an `es_input.cfg`. A keyboard, an entry without a
/// GUID, or one missing its `a` or `b` is left out with a warning.
pub fn sdl_mappings(cfg: &str) -> Vec<String> {
    let parsed = xml::parse(cfg);
    if let Some(error) = parsed.error {
        eprintln!("forma: es_input.cfg read up to an error: {error}");
    }
    let Some(root) = parsed.root else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    for pad in root.children_named("inputConfig") {
        if pad.attr("type") != Some("joystick") {
            continue;
        }
        let name = pad.attr("deviceName").unwrap_or("").replace(',', " ");
        let Some(guid) = pad
            .attr("deviceGUID")
            .filter(|g| g.len() == 32 && g.chars().all(|c| c.is_ascii_hexdigit()))
        else {
            eprintln!("forma: es_input.cfg: {name} has no usable GUID, skipped");
            continue;
        };
        let inputs: Vec<(&str, &str, &str, &str)> = pad
            .children_named("input")
            .filter_map(|i| {
                Some((
                    i.attr("name")?,
                    i.attr("type")?,
                    i.attr("id")?,
                    i.attr("value").unwrap_or("1"),
                ))
            })
            .collect();
        let select = inputs.iter().find(|i| i.0 == "select").map(|i| (i.1, i.2));
        let mut parts = Vec::new();
        for (es, kind, id, value) in &inputs {
            // Batocera's hotkey is often select itself: no guide button then.
            if *es == "hotkey" && select == Some((*kind, *id)) {
                continue;
            }
            if let (Some(sdl), Some(bound)) = (sdl_name(es), binding(es, kind, id, value)) {
                parts.push(format!("{sdl}:{bound}"));
            }
        }
        if !parts.iter().any(|p| p.starts_with("a:")) || !parts.iter().any(|p| p.starts_with("b:"))
        {
            eprintln!("forma: es_input.cfg: {name} has no a or b button, skipped");
            continue;
        }
        parts.sort();
        lines.push(format!(
            "{},{name},{},platform:Linux,",
            guid.to_lowercase(),
            parts.join(",")
        ));
    }
    lines
}

/// Batocera's `es_input.cfg`, then the user's (which wins for a pad both list), as SDL mapping
/// lines; empty where neither exists.
pub fn mappings_from(system: &Path, user: &Path) -> String {
    let mut out = String::new();
    for file in [system, user] {
        if let Ok(cfg) = std::fs::read_to_string(file) {
            for line in sdl_mappings(&cfg) {
                out.push_str(&line);
                out.push('\n');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const XBOX: &str = r#"<?xml version="1.0"?>
<inputList>
	<inputConfig type="keyboard" deviceName="Keyboard" deviceGUID="-1">
		<input name="a" type="key" id="13" value="1" />
	</inputConfig>
	<inputConfig type="joystick" deviceName="Xbox 360 Wireless Receiver" deviceGUID="030000005e0400001907000000010000">
		<input name="a" type="button" id="1" value="1" code="305" />
		<input name="b" type="button" id="0" value="1" code="304" />
		<input name="down" type="hat" id="0" value="4" code="16" />
		<input name="hotkey" type="button" id="8" value="1" code="316" />
		<input name="joystick1left" type="axis" id="0" value="-1" code="0" />
		<input name="joystick1up" type="axis" id="1" value="-1" code="1" />
		<input name="l2" type="axis" id="2" value="1" code="2" />
		<input name="select" type="button" id="6" value="1" code="314" />
		<input name="start" type="button" id="7" value="1" code="315" />
		<input name="x" type="button" id="3" value="1" code="308" />
		<input name="y" type="button" id="2" value="1" code="307" />
	</inputConfig>
	<inputConfig type="joystick" deviceName="Odd Pad, Mk 2" deviceGUID="03000000aaaa0000bbbb000000010000">
		<input name="a" type="button" id="2" value="1" />
		<input name="b" type="button" id="1" value="1" />
		<input name="hotkey" type="button" id="9" value="1" />
		<input name="select" type="button" id="9" value="1" />
		<input name="left" type="axis" id="6" value="-1" />
		<input name="joystick1up" type="axis" id="1" value="1" />
	</inputConfig>
	<inputConfig type="joystick" deviceName="Half Pad" deviceGUID="03000000cccc0000dddd000000010000">
		<input name="start" type="button" id="0" value="1" />
	</inputConfig>
</inputList>"#;

    #[test]
    fn es_places_become_sdls_as_its_own_database_says() {
        let lines = sdl_mappings(XBOX);
        assert_eq!(
            lines.len(),
            2,
            "the keyboard and the pad with no a or b left out"
        );
        // SDL's database for this pad: a:b0, b:b1, x:b2, y:b3, guide:b8, leftx:a0, lefty:a1,
        // lefttrigger:a2, dpdown on the hat.
        assert_eq!(
            lines[0],
            "030000005e0400001907000000010000,Xbox 360 Wireless Receiver,a:b0,b:b1,back:b6,\
             dpdown:h0.4,guide:b8,lefttrigger:a2,leftx:a0,lefty:a1,start:b7,x:b2,y:b3,\
             platform:Linux,"
        );
    }

    #[test]
    fn a_hotkey_that_is_select_a_turned_stick_and_a_dpad_on_an_axis() {
        let lines = sdl_mappings(XBOX);
        let odd = &lines[1];
        assert!(odd.starts_with("03000000aaaa0000bbbb000000010000,Odd Pad  Mk 2,"));
        assert!(odd.contains("a:b1,b:b2"));
        assert!(odd.contains("back:b9") && !odd.contains("guide"));
        assert!(odd.contains("dpleft:-a6"));
        assert!(odd.contains("lefty:a1~"));
    }

    #[test]
    fn the_users_file_comes_after_batoceras() {
        let dir = tempfile::tempdir().unwrap();
        let system = dir.path().join("system.cfg");
        let user = dir.path().join("user.cfg");
        std::fs::write(&system, XBOX).unwrap();
        std::fs::write(
            &user,
            XBOX.replace(
                "id=\"1\" value=\"1\" code=\"305\"",
                "id=\"5\" value=\"1\" code=\"305\"",
            ),
        )
        .unwrap();
        let all = mappings_from(&system, &user);
        let xbox: Vec<&str> = all
            .lines()
            .filter(|l| l.starts_with("030000005e04"))
            .collect();
        assert_eq!(xbox.len(), 2);
        assert!(
            xbox[1].contains("b:b5"),
            "the user's last, so it is the one kept"
        );
        assert_eq!(
            mappings_from(&dir.path().join("none"), &dir.path().join("none")),
            ""
        );
    }
}
