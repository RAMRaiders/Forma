//! Forma's boot picture: what Batocera shows while the box starts, after its own logo, in
//! place of its video, so the box shows Forma's screen from there until Forma is up.
//!
//! It is a picture of Forma's entry screen: its logo, with Batocera's, Forma's and the theme's
//! versions in the corner, laid out as the shell's cover lays them out, so the box goes from the
//! picture to Forma's own screen without a jump. It is drawn again (with Batocera's
//! `rsvg-convert`) whenever what it shows changes: a new version, another theme. It goes to
//! `/userdata/splash/forma.png`: Batocera shows the user's own picture from there in place of
//! its splash video (its `S28splash`), and the folder survives Batocera's updates. Then
//! `forma-standalone` keeps it on screen under Forma's see-through window until the UI is up.
//!
//! `scripts/batocera/uninstall.sh` takes it away, and Batocera's video comes back.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use crate::shell::CoverInfo;

/// Where Batocera looks for the user's own boot pictures.
const SPLASH_DIR: &str = "/userdata/splash";
const PICTURE: &str = "forma.png";
const LOGO: &str = include_str!("../boot/forma-logo.svg");

/// What was drawn last in this process, so a page load that changes nothing costs nothing.
static DRAWN: Mutex<Option<String>> = Mutex::new(None);

/// The boot picture is there, for the screen under Forma's window while it starts.
pub fn drawn() -> bool {
    Path::new(SPLASH_DIR).join(PICTURE).is_file()
}

/// Draws the boot picture for `cover` in the background, if it changed. Only on Batocera.
pub fn refresh(cover: &CoverInfo) {
    if cover.batocera.is_empty() || !Path::new(SPLASH_DIR).is_dir() {
        return;
    }
    let (width, height) = screen_size();
    let svg = picture(
        cover,
        env!("CARGO_PKG_VERSION"),
        width,
        height,
        emblem().as_deref(),
        theme_logo(cover).as_ref(),
    );
    {
        let mut drawn = DRAWN
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if drawn.as_deref() == Some(svg.as_str()) {
            return;
        }
        drawn.replace(svg.clone());
    }
    std::thread::spawn(move || {
        if let Err(e) = draw(&svg, &Path::new(SPLASH_DIR).join(PICTURE), width, height) {
            eprintln!("forma: cannot draw the boot picture: {e}");
        }
    });
}

/// Renders `svg` to `out`, unless the picture there was drawn from the same SVG (kept beside
/// Forma's data, so a restart does not redraw it).
fn draw(svg: &str, out: &Path, width: u32, height: u32) -> Result<(), String> {
    let source = PathBuf::from("/userdata/system/forma/boot-picture.svg");
    if out.is_file() && std::fs::read_to_string(&source).is_ok_and(|s| s == svg) {
        return Ok(());
    }
    if let Some(dir) = source.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&source, svg).map_err(|e| e.to_string())?;
    // Drawn aside, then moved in: Batocera never finds half a picture at boot.
    let partial = out.with_extension("png.part");
    let status = system_command("rsvg-convert")
        .args([
            "-w",
            &width.to_string(),
            "-h",
            &height.to_string(),
            "-f",
            "png",
            "-o",
        ])
        .arg(&partial)
        .arg(&source)
        .status()
        .map_err(|e| format!("rsvg-convert: {e}"))?;
    if !status.success() {
        let _ = std::fs::remove_file(&partial);
        return Err(format!("rsvg-convert: {status}"));
    }
    std::fs::rename(&partial, out).map_err(|e| e.to_string())
}

/// A command of Batocera's own, without Forma's environment: Forma's bundled libraries
/// (`LD_LIBRARY_PATH` from its AppImage) break Batocera's programs.
fn system_command(program: &str) -> Command {
    let mut command = Command::new(program);
    command
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .env("HOME", "/userdata/system");
    command
}

/// The screen's size, as Batocera's own boot image script reads it; 1920×1080 if unknown.
fn screen_size() -> (u32, u32) {
    std::fs::read_to_string("/sys/class/graphics/fb0/modes")
        .ok()
        .and_then(|modes| parse_mode(modes.lines().next()?))
        .unwrap_or((1920, 1080))
}

/// `U:1920x1080p-0` → (1920, 1080).
fn parse_mode(mode: &str) -> Option<(u32, u32)> {
    let size = mode.split(':').nth(1)?;
    let (w, rest) = size.split_once('x')?;
    let h: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let (w, h) = (w.parse().ok()?, h.parse().ok()?);
    (w > 0 && h > 0).then_some((w, h))
}

/// Batocera's boot image, inline, for its emblem.
fn emblem() -> Option<String> {
    let png = std::fs::read(crate::boot::batocera_splash_file("boot-logo.png")).ok()?;
    Some(format!("data:image/png;base64,{}", base64(&png)))
}

/// The theme's logo, inline, with its width over its height; `None` without one.
struct ThemeLogo {
    src: String,
    aspect: f64,
}

/// The theme's logo file (an SVG or PNG; rsvg draws no WebP), for the boot picture.
fn theme_logo(cover: &CoverInfo) -> Option<ThemeLogo> {
    let file = cover.theme_logo_file.as_ref()?;
    let bytes = std::fs::read(file).ok()?;
    let ext = file.extension()?.to_str()?.to_ascii_lowercase();
    let (mime, aspect) = match ext.as_str() {
        "svg" => (
            "image/svg+xml",
            svg_aspect(&String::from_utf8_lossy(&bytes))?,
        ),
        "png" => ("image/png", png_aspect(&bytes)?),
        _ => return None,
    };
    Some(ThemeLogo {
        src: format!("data:{mime};base64,{}", base64(&bytes)),
        aspect,
    })
}

/// An SVG's width over its height, from its `viewBox`.
fn svg_aspect(svg: &str) -> Option<f64> {
    let start = svg.find("viewBox=\"")? + "viewBox=\"".len();
    let end = start + svg[start..].find('"')?;
    let n: Vec<f64> = svg[start..end]
        .split([' ', ','])
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect();
    match n[..] {
        [_, _, w, h] if w > 0.0 && h > 0.0 => Some(w / h),
        _ => None,
    }
}

/// A PNG's width over its height, from its header.
fn png_aspect(png: &[u8]) -> Option<f64> {
    if png.len() < 24 || &png[1..4] != b"PNG" {
        return None;
    }
    let w = u32::from_be_bytes(png[16..20].try_into().ok()?);
    let h = u32::from_be_bytes(png[20..24].try_into().ok()?);
    (w > 0 && h > 0).then(|| f64::from(w) / f64::from(h))
}

/// The cover, as an SVG `width`×`height`: sizes in the cover's vh, the versions in its grid.
/// With the theme's logo, its row shows the logo, then the version, as the cover does.
fn picture(
    cover: &CoverInfo,
    forma: &str,
    width: u32,
    height: u32,
    emblem: Option<&str>,
    logo: Option<&ThemeLogo>,
) -> String {
    let (w, h) = (f64::from(width), f64::from(height));
    let vh = h / 100.0;
    // The logo: 24vh high, centred with the loader 5vh under it (the cover's grid).
    let (logo_w, logo_h) = (24.0 * vh * 96.0 / 130.0, 24.0 * vh);
    let top = (h - (24.0 + 5.0 + 0.5) * vh) / 2.0;
    let logo_x = (w - logo_w) / 2.0;
    let bar_y = top + logo_h + 5.0 * vh;
    let bar_w = 26.0 * vh;

    // The versions: three columns (emblem 3vh, name, version), 1.4vh apart, rows 1.2vh apart,
    // text 2vh, the block 4vh (and the TV margin) from the bottom right.
    let font = 2.0 * vh;
    let margin = (4.0 + f64::from(cover.margins)) * vh;
    let mut lines: Vec<(&str, &str, &str)> = Vec::new();
    if !cover.batocera.is_empty() {
        lines.push(("batocera", "Batocera", &cover.batocera));
    }
    lines.push(("forma", "Forma", forma));
    // With a logo, the theme's row gives its version only: the logo goes before it.
    let logo = logo.filter(|_| !cover.theme.is_empty() && !cover.theme_version.is_empty());
    if !cover.theme.is_empty() {
        let shown = if logo.is_some() {
            &cover.theme_version
        } else {
            &cover.theme
        };
        lines.push(("theme", "Theme", shown));
    }
    // The logo as the cover draws it: 2.6vh high, 1vh before the version.
    let theme_logo_h = 2.6 * vh;
    let logo_lead = logo.map_or(0.0, |l| theme_logo_h * l.aspect + vh);
    // DejaVu Sans widths of the names, in em; DejaVu Sans Mono is 0.602em a character.
    let name_em = |n: &str| match n {
        "Batocera" => 4.42,
        "Theme" => 3.42,
        _ => 3.07,
    };
    let name_w = lines.iter().map(|l| name_em(l.1)).fold(0.0, f64::max) * font;
    let version_w = lines
        .iter()
        .map(|l| {
            let lead = if l.0 == "theme" { logo_lead } else { 0.0 };
            l.2.chars().count() as f64 * 0.602 * font + lead
        })
        .fold(0.0, f64::max);
    let icon = 3.0 * vh;
    let gap = 1.4 * vh;
    let row = icon.max(font * 1.35);
    let block_h = row * lines.len() as f64 + 1.2 * vh * (lines.len() as f64 - 1.0);
    let x_icon = w - margin - version_w - gap - name_w - gap - icon;
    let x_name = x_icon + icon + gap;
    let x_version = x_name + name_w + gap;
    let mut versions = String::new();
    for (i, (kind, name, version)) in lines.iter().enumerate() {
        let y = h - margin - block_h + i as f64 * (row + 1.2 * vh);
        let mid = y + row / 2.0;
        versions += &match (*kind, emblem) {
            // The emblem cut from the boot image, as the cover's CSS crops it.
            ("batocera", Some(src)) => {
                let iw = icon * 2.6667;
                let ih = iw * 1080.0 / 1920.0;
                format!(
                    r#"<svg x="{x:.1}" y="{y:.1}" width="{icon:.1}" height="{icon:.1}"><image href="{src}" x="{ix:.1}" y="{iy:.1}" width="{iw:.1}" height="{ih:.1}" preserveAspectRatio="none"/></svg>"#,
                    x = x_icon,
                    y = mid - icon / 2.0,
                    ix = (icon - iw) * 0.5,
                    iy = (icon - ih) * 0.375,
                )
            }
            ("batocera", None) => String::new(),
            ("forma", _) => format!(
                r#"<svg x="{x:.1}" y="{y:.1}" width="{icon:.1}" height="{icon:.1}" viewBox="0 0 96 130">{logo}</svg>"#,
                x = x_icon,
                y = mid - icon / 2.0,
                logo = logo_body(),
            ),
            _ => format!(
                r#"<rect x="{x:.1}" y="{y:.1}" width="{s:.1}" height="{s:.1}" rx="{r:.1}" fill="url(#accent)"/>"#,
                s = 1.2 * vh,
                x = x_icon + (icon - 1.2 * vh) / 2.0,
                y = mid - 0.6 * vh,
                r = 0.3 * vh,
            ),
        };
        let mut x_text = x_version;
        if let (true, Some(l)) = (*kind == "theme", logo) {
            versions += &format!(
                r#"<image href="{src}" x="{x:.1}" y="{y:.1}" width="{lw:.1}" height="{lh:.1}"/>"#,
                src = l.src,
                x = x_version,
                y = mid - theme_logo_h / 2.0,
                lw = theme_logo_h * l.aspect,
                lh = theme_logo_h,
            );
            x_text += logo_lead;
        }
        versions += &format!(
            r##"<text x="{x_name:.1}" y="{b:.1}" fill="#8aa0b6" font-family="DejaVu Sans" font-size="{font:.1}">{name}</text><text x="{x_text:.1}" y="{b:.1}" fill="#e6f7ff" font-family="DejaVu Sans Mono" font-size="{font:.1}">{version}</text>"##,
            b = mid + font * 0.36,
            version = escape(version),
        );
    }

    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">
<defs>
<radialGradient id="bg" cx="50%" cy="45%" r="70%"><stop offset="0" stop-color="#0a1a2e"/><stop offset="1" stop-color="#04060c"/></radialGradient>
<linearGradient id="accent" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#ff2e88"/><stop offset="1" stop-color="#b23cff"/></linearGradient>
</defs>
<rect width="{width}" height="{height}" fill="#04060c"/>
<rect width="{width}" height="{height}" fill="url(#bg)"/>
<svg x="{logo_x:.1}" y="{top:.1}" width="{logo_w:.1}" height="{logo_h:.1}" viewBox="0 0 96 130">{logo}</svg>
<rect x="{bar_x:.1}" y="{bar_y:.1}" width="{bar_w:.1}" height="{bar_h:.1}" rx="{bar_h:.1}" fill="#22e4ff" fill-opacity=".15"/>
{versions}
</svg>
"##,
        logo = logo_body(),
        bar_x = (w - bar_w) / 2.0,
        bar_h = 0.5 * vh,
    )
}

/// The logo's drawing, without its own `<svg>` tag, to nest at any size.
fn logo_body() -> &'static str {
    let start = LOGO.find('>').map_or(0, |i| i + 1);
    let end = LOGO.rfind("</svg>").unwrap_or(LOGO.len());
    &LOGO[start..end]
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ABC[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_screen_mode() {
        assert_eq!(parse_mode("U:1920x1080p-0"), Some((1920, 1080)));
        assert_eq!(parse_mode("U:1280x1024p-60"), Some((1280, 1024)));
        assert_eq!(parse_mode("nonsense"), None);
    }

    #[test]
    fn encodes_base64() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn draws_the_logo_and_the_three_versions() {
        let cover = CoverInfo {
            batocera: "43.1 2026/05/29".into(),
            theme: "Cyber <0.7>".into(),
            margins: 0,
            see_through: false,
            ..Default::default()
        };
        let svg = picture(
            &cover,
            "0.1.0",
            1920,
            1080,
            Some("data:image/png;base64,AA=="),
            None,
        );
        assert!(svg.contains(r#"width="1920" height="1080""#));
        for text in [
            "Batocera",
            "43.1 2026/05/29",
            "Forma",
            "0.1.0",
            "Theme",
            "Cyber &lt;0.7&gt;",
        ] {
            assert!(svg.contains(&format!(">{text}</text>")), "{text}");
        }
        assert!(svg.contains("data:image/png;base64,AA=="));
        assert!(!logo_body().contains("<svg"));
    }

    #[test]
    fn draws_the_theme_logo_before_its_version() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("logo.svg");
        std::fs::write(&file, r#"<svg viewBox="0 0 254 72"></svg>"#).unwrap();
        let cover = CoverInfo {
            batocera: "43.1".into(),
            theme: "Cyber 0.7.12".into(),
            theme_version: "0.7.12".into(),
            theme_logo_file: Some(file),
            ..Default::default()
        };
        let logo = theme_logo(&cover).unwrap();
        assert!((logo.aspect - 254.0 / 72.0).abs() < 1e-9);
        let svg = picture(&cover, "0.1.0", 1920, 1080, None, Some(&logo));
        assert!(svg.contains(r#"<image href="data:image/svg+xml;base64,"#));
        assert!(
            svg.contains(">0.7.12</text>"),
            "the version, without the name"
        );
        assert!(!svg.contains(">Cyber 0.7.12</text>"));
        // Forma's own logo, in the middle, keeps its 24vh whatever the theme's logo.
        assert!(svg.contains(r#"width="191.4" height="259.2" viewBox="0 0 96 130""#));
    }

    #[test]
    fn reads_a_png_logo_size() {
        let mut png = vec![0x89, b'P', b'N', b'G', 13, 10, 26, 10, 0, 0, 0, 13];
        png.extend(b"IHDR");
        png.extend(400u32.to_be_bytes());
        png.extend(100u32.to_be_bytes());
        assert_eq!(png_aspect(&png), Some(4.0));
        assert_eq!(png_aspect(b"nope"), None);
    }
}
