//! Starting the host's own programs from inside the AppImage.
//!
//! The AppImage's `AppRun` points `LD_LIBRARY_PATH`, `PATH`, `PYTHONHOME`, and the GTK, GIO and
//! GStreamer paths into the bundle, so Forma finds its own libraries. Every program Forma starts
//! inherits that. Batocera's Python tools (`emulatorlauncher`) then cannot find their standard
//! library, and `connmand`, restarted by `batocera-wifi`, loads Forma's copies of its libraries
//! and dies, taking the network with it. [`command`] gives programs the environment they would
//! have had without the bundle.

use std::ffi::{OsStr, OsString};
use std::process::Command;

use crate::error::{Error, Result};

/// Set by the AppImage for Forma alone.
const BUNDLE_ONLY: &[&str] = &[
    "APPDIR",
    "APPIMAGE",
    "ARGV0",
    "OWD",
    "PYTHONDONTWRITEBYTECODE",
];

/// A [`Command`] for a host program, with the bundle's additions taken out of its environment.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    if let Some(env) = host_env(std::env::vars_os()) {
        command.env_clear().envs(env);
    }
    command
}

/// `vars` without the AppImage's additions. `None` outside an AppImage (no `APPDIR`): there is
/// nothing to take out.
pub fn host_env(
    vars: impl IntoIterator<Item = (OsString, OsString)>,
) -> Option<Vec<(OsString, OsString)>> {
    let vars: Vec<_> = vars.into_iter().collect();
    let appdir = vars
        .iter()
        .find(|(k, _)| k == "APPDIR")
        .and_then(|(_, v)| v.to_str())
        .map(|d| d.trim_end_matches('/').to_owned())
        .filter(|d| !d.is_empty())?;
    let inside = |entry: &str| {
        // AppRun's paths come with doubled slashes (`…/app//usr/lib`).
        let entry = entry.replace("//", "/");
        entry == appdir || entry.starts_with(&format!("{appdir}/"))
    };

    let mut out = Vec::with_capacity(vars.len());
    for (key, value) in vars {
        if key.to_str().is_some_and(|k| BUNDLE_ONLY.contains(&k)) {
            continue;
        }
        let Some(text) = value.to_str() else {
            out.push((key, value));
            continue;
        };
        if !text.contains(appdir.as_str()) {
            out.push((key, value));
            continue;
        }
        // A path list keeps the host's entries; a value that is only the bundle goes.
        let kept: Vec<&str> = text
            .split(':')
            .filter(|e| !e.is_empty() && !inside(e))
            .collect();
        if !kept.is_empty() {
            out.push((key, kept.join(":").into()));
        }
    }
    Some(out)
}

/// Runs the system's tools; a fake one in tests.
pub trait Shell: Send + Sync {
    /// Runs `program` to completion and returns its standard output.
    fn run(&self, program: &str, args: &[&str]) -> Result<String>;
}

/// Runs real programs, with the host's environment rather than the AppImage's (see
/// [`crate::host`]): `connmand`, restarted from here, must not load Forma's libraries. Errors
/// name the program, never its arguments: those can hold a Wi-Fi password.
pub struct SystemShell;

impl Shell for SystemShell {
    fn run(&self, program: &str, args: &[&str]) -> Result<String> {
        let out = command(program)
            .args(args)
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| Error::Network(format!("could not run `{program}`: {e}")))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let why = stderr.lines().next().unwrap_or("no message");
            return Err(Error::Network(format!("`{program}` failed: {why}")));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect()
    }

    fn get<'a>(env: &'a [(OsString, OsString)], key: &str) -> Option<&'a str> {
        env.iter()
            .find(|(k, _)| k == key)
            .and_then(|(_, v)| v.to_str())
    }

    #[test]
    fn takes_the_bundle_out_and_keeps_the_host() {
        let out = host_env(env(&[
            ("APPDIR", "/userdata/system/forma/app"),
            ("APPIMAGE", "/userdata/Forma.AppImage"),
            ("PYTHONDONTWRITEBYTECODE", "1"),
            (
                "PATH",
                "/userdata/system/forma/app/usr/bin/:/userdata/system/forma/app/bin/:/usr/sbin:/usr/bin",
            ),
            (
                "LD_LIBRARY_PATH",
                "/userdata/system/forma/app/usr/lib/:/userdata/system/forma/app//usr/lib/gtk",
            ),
            ("PYTHONHOME", "/userdata/system/forma/app/usr/"),
            (
                "XDG_DATA_DIRS",
                "/userdata/system/forma/app/usr/share/:/usr/share:",
            ),
            ("GIO_MODULE_DIR", "/userdata/system/forma/app//usr/lib/gio/modules"),
            ("HOME", "/userdata/system"),
            ("WAYLAND_DISPLAY", "wayland-0"),
            ("GTK_THEME", "Adwaita:light"),
        ]))
        .unwrap();

        assert_eq!(get(&out, "PATH"), Some("/usr/sbin:/usr/bin"));
        assert_eq!(get(&out, "XDG_DATA_DIRS"), Some("/usr/share"));
        for gone in [
            "APPDIR",
            "APPIMAGE",
            "PYTHONDONTWRITEBYTECODE",
            "LD_LIBRARY_PATH",
            "PYTHONHOME",
            "GIO_MODULE_DIR",
        ] {
            assert_eq!(get(&out, gone), None, "{gone}");
        }
        assert_eq!(get(&out, "HOME"), Some("/userdata/system"));
        assert_eq!(get(&out, "WAYLAND_DISPLAY"), Some("wayland-0"));
        assert_eq!(get(&out, "GTK_THEME"), Some("Adwaita:light"));
    }

    #[test]
    fn a_neighbour_with_the_same_prefix_is_not_the_bundle() {
        let out = host_env(env(&[
            ("APPDIR", "/opt/forma/app/"),
            ("PATH", "/opt/forma/app2/bin:/opt/forma/app/usr/bin"),
        ]))
        .unwrap();
        assert_eq!(get(&out, "PATH"), Some("/opt/forma/app2/bin"));
    }

    #[test]
    fn outside_an_appimage_nothing_changes() {
        assert!(host_env(env(&[("PATH", "/usr/bin")])).is_none());
        assert!(host_env(env(&[("APPDIR", "")])).is_none());
    }
}
