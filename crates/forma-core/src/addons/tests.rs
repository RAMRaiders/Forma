use std::collections::BTreeMap;
use std::io::Write;
use std::sync::{Arc, Mutex};

use serde_json::json;
use sha2::{Digest, Sha256};

use super::runtime::{ScraperAddon, Script, allowed, call, stations};
use super::*;
use crate::host::Shell;
use crate::net::{HttpError, HttpRequest};
use crate::scraper::{Credentials, MediaKind, Query, Source};
use crate::themes::catalogue::{CatalogueTheme, hex};

/// Answers every request with `body`; records them.
struct Echo {
    body: &'static str,
    sent: Mutex<Vec<HttpRequest>>,
}

impl Http for Echo {
    fn get(&self, url: &str) -> std::result::Result<Vec<u8>, HttpError> {
        self.send(&HttpRequest::get(url))
    }
    fn send(&self, r: &HttpRequest) -> std::result::Result<Vec<u8>, HttpError> {
        self.sent.lock().unwrap().push(r.clone());
        if r.url.contains("/missing") {
            return Err(HttpError {
                status: Some(404),
                message: "nope".into(),
            });
        }
        Ok(self.body.as_bytes().to_vec())
    }
}

fn echo(body: &'static str) -> Arc<Echo> {
    Arc::new(Echo {
        body,
        sent: Mutex::new(Vec::new()),
    })
}

/// Records what it is asked to run; fails when told to.
#[derive(Default)]
struct Recorder {
    ran: Mutex<Vec<Vec<String>>>,
    fail: bool,
}

impl Shell for Recorder {
    fn run(&self, program: &str, args: &[&str]) -> crate::error::Result<String> {
        let mut all = vec![program.to_owned()];
        all.extend(args.iter().map(|a| (*a).to_owned()));
        self.ran.lock().unwrap().push(all);
        if self.fail {
            return Err(Error::Invalid("exit 1".into()));
        }
        Ok(format!("ran {}", args.get(1).copied().unwrap_or("")))
    }
}

struct NoShell;

impl Shell for NoShell {
    fn run(&self, _: &str, _: &[&str]) -> crate::error::Result<String> {
        panic!("nothing should run");
    }
}

const HOSTS: &[&str] = &["https://api.example.com", "https://cdn.example.com:8443"];

fn hosts() -> Vec<String> {
    HOSTS.iter().map(|h| (*h).to_owned()).collect()
}

fn run(
    code: &str,
    http: Arc<Echo>,
    input: serde_json::Value,
) -> std::result::Result<serde_json::Value, String> {
    let mut settings = Credentials::new();
    settings.insert("apiKey".into(), "k3y".into());
    call(
        "test",
        code,
        &hosts(),
        &settings,
        http,
        None,
        "lookup",
        &input,
    )
}

#[test]
fn a_script_gets_its_input_settings_and_http_to_its_hosts() {
    let http = echo(r#"{"title":"Super Metroid","cover":"https://cdn.example.com:8443/c.png"}"#);
    let code = r#"
        export function lookup(q) {
          const r = forma.http.get(`https://api.example.com/search?q=${encodeURIComponent(q.name)}`,
                                   { 'X-Key': forma.settings.get('apiKey') });
          const posted = forma.http.post('https://api.example.com/p', { a: 1 });
          const gone = forma.http.get('https://api.example.com/missing');
          const g = r.json();
          return { name: g.title, status: r.status, posted: posted.status, gone: gone.status,
                   none: forma.settings.get('nope'), sys: q.system };
        }"#;
    let out = run(
        code,
        http.clone(),
        json!({ "name": "Super Metroid", "system": "snes" }),
    )
    .unwrap();
    assert_eq!(
        out,
        json!({ "name": "Super Metroid", "status": 200, "posted": 200, "gone": 404, "none": null, "sys": "snes" })
    );
    let sent = http.sent.lock().unwrap();
    assert_eq!(
        sent[0].url,
        "https://api.example.com/search?q=Super%20Metroid"
    );
    assert!(sent[0].headers.contains(&("X-Key".into(), "k3y".into())));
    assert_eq!(sent[1].method, "POST");
    assert_eq!(sent[1].body.as_deref(), Some(&b"{\"a\":1}"[..]));
}

#[test]
fn async_functions_are_awaited() {
    let out = run(
        "export async function lookup(q) { await null; return { n: q.n * 2 }; }",
        echo(""),
        json!({ "n": 21 }),
    )
    .unwrap();
    assert_eq!(out, json!({ "n": 42 }));
}

#[test]
fn the_sandbox_holds() {
    // Another host: refused, and the script sees an error it can catch.
    let code = r#"export function lookup() {
        try { forma.http.get('https://evil.example.net/x'); return 'reached'; }
        catch (e) { return String(e.message); } }"#;
    let http = echo("");
    let out = run(code, http.clone(), json!({})).unwrap();
    assert_eq!(
        out,
        json!("https://evil.example.net/x is not one of the add-on's hosts")
    );
    assert!(http.sent.lock().unwrap().is_empty());
    // Nothing else from the host: no natives left behind, no require, no std/os modules.
    let out = run(
        "export function lookup() { return [typeof __formaRequest, typeof require, typeof std, typeof os, typeof process]; }",
        echo(""),
        json!({}),
    )
    .unwrap();
    assert_eq!(
        out,
        json!([
            "undefined",
            "undefined",
            "undefined",
            "undefined",
            "undefined"
        ])
    );
    let err = run(
        "import * as os from 'os'; export function lookup() { return 1; }",
        echo(""),
        json!({}),
    )
    .unwrap_err();
    assert!(!err.is_empty());
    // forma cannot be replaced.
    let out = run("export function lookup() { 'use strict'; try { forma.http = null; } catch (e) { return 'frozen'; } return 'changed'; }", echo(""), json!({})).unwrap();
    assert_eq!(out, json!("frozen"));
}

#[test]
fn runaway_scripts_are_stopped() {
    let err = run(
        "export function lookup() { const a = []; for (;;) a.push(new Array(1e6).fill(1)); }",
        echo(""),
        json!({}),
    )
    .unwrap_err();
    assert!(
        err.to_lowercase().contains("memory") || err.contains("interrupted"),
        "{err}"
    );
    let err = run(
        "export function lookup() { throw new Error('broken'); }",
        echo(""),
        json!({}),
    )
    .unwrap_err();
    assert!(err.contains("broken"), "{err}");
    let err = run("export const lookup = 3;", echo(""), json!({})).unwrap_err();
    assert!(!err.is_empty());
    let err = run("export function other() {}", echo(""), json!({})).unwrap_err();
    assert_eq!(err, "it exports no `lookup` function");
}

#[test]
fn hosts_are_origins() {
    let h = hosts();
    assert!(allowed("https://api.example.com/v1/x?y", &h));
    assert!(allowed("https://API.example.com", &h));
    assert!(allowed("https://cdn.example.com:8443/c.png", &h));
    assert!(
        !allowed("https://cdn.example.com/c.png", &h),
        "another port"
    );
    assert!(!allowed("http://api.example.com/", &h));
    assert!(!allowed("https://api.example.com.evil.net/", &h));
    assert!(!allowed("https://api.example.com@evil.net/", &h));
}

fn write_addon(root: &Path, id: &str, manifest: serde_json::Value, code: &str) -> PathBuf {
    let dir = root.join(id);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("addon.json"), manifest.to_string()).unwrap();
    std::fs::write(dir.join("main.js"), code).unwrap();
    dir
}

fn manifest(id: &str) -> serde_json::Value {
    json!({
        "id": id, "name": "Example", "version": "1.0.0", "sdk": "^1",
        "provides": ["scraper"], "permissions": ["network"],
        "hosts": ["https://api.example.com", "https://cdn.example.com:8443"],
        "settings": [{ "key": "apiKey", "label": "API key", "secret": true, "required": true }],
        "scraper": { "gives": ["thumbnail", "image"], "metadata": true }
    })
}

const SCRAPER: &str = r#"
export function lookup(q) {
  const g = forma.http.get('https://api.example.com/g?k=' + forma.settings.get('apiKey')).json();
  if (!g.title) return null;
  return {
    metadata: { name: g.title, releaseDate: '1994-03-19', rating: 0.9, players: 1 },
    media: [
      { kind: 'thumbnail', url: g.cover },
      { kind: 'image', url: 'https://elsewhere.example.org/shot.png' },
      { kind: 'nonsense', url: g.cover }
    ]
  };
}"#;

#[test]
fn a_scraper_addon_is_a_source() {
    let dir = tempfile::tempdir().unwrap();
    let addon = load(&write_addon(
        dir.path(),
        "example",
        manifest("example"),
        SCRAPER,
    ));
    assert!(addon.usable(), "{:?}", addon.problem);
    let http = echo(r#"{"title":"Super Metroid","cover":"https://cdn.example.com:8443/c.png"}"#);
    let script = Script::new(&addon, http, Arc::new(NoShell)).unwrap();
    let source = ScraperAddon::new(&addon, script).unwrap();
    assert_eq!(source.id(), "addon:example");
    assert_eq!(source.needs()[0].key, "apiKey");
    assert!(!source.ready(&BTreeMap::new()));
    let mut creds = Credentials::new();
    creds.insert("apiKey".into(), "k".into());
    assert!(source.ready(&creds));
    let wanted = [MediaKind::Thumbnail];
    let q = Query {
        system: "snes",
        platforms: &[],
        file_name: "a.sfc",
        stem: "a",
        name: "a",
        size: 1,
        hashes: None,
        wanted: &wanted,
        wants_metadata: true,
    };
    let found = source.lookup(&*echo(""), &creds, &q).unwrap().unwrap();
    assert_eq!(found.metadata.name.as_deref(), Some("Super Metroid"));
    assert_eq!(found.metadata.release_date.as_deref(), Some("1994-03-19"));
    assert_eq!(found.metadata.players.as_deref(), Some("1"));
    // Only media on its own hosts, of kinds that exist.
    assert_eq!(
        found.media,
        [(
            MediaKind::Thumbnail,
            "https://cdn.example.com:8443/c.png".to_owned()
        )]
    );
}

#[test]
fn manifests_are_checked() {
    let dir = tempfile::tempdir().unwrap();
    let cases: Vec<(&str, serde_json::Value, &str)> = vec![
        (
            "Bad_Id",
            {
                let mut m = manifest("Bad_Id");
                m["id"] = json!("Bad_Id");
                m
            },
            "kebab-case",
        ),
        (
            "future",
            {
                let mut m = manifest("future");
                m["sdk"] = json!("^9");
                m
            },
            "needs Forma API ^9",
        ),
        (
            "other",
            {
                let mut m = manifest("other");
                m["provides"] = json!(["theme-engine"]);
                m
            },
            "as `theme-engine`",
        ),
        (
            "noscraper",
            {
                let mut m = manifest("noscraper");
                m.as_object_mut().unwrap().remove("scraper");
                m
            },
            "`scraper` section",
        ),
        (
            "nonet",
            {
                let mut m = manifest("nonet");
                m["permissions"] = json!([]);
                m
            },
            "without the `network` permission",
        ),
        (
            "badhost",
            {
                let mut m = manifest("badhost");
                m["hosts"] = json!(["https://x.com/path"]);
                m
            },
            "not an https:// origin",
        ),
        (
            "escape",
            {
                let mut m = manifest("escape");
                m["entry"] = json!("../x.js");
                m
            },
            "inside the add-on",
        ),
        (
            "perm",
            {
                let mut m = manifest("perm");
                m["permissions"] = json!(["network", "files"]);
                m
            },
            "unknown permission `files`",
        ),
    ];
    for (id, m, want) in cases {
        let addon = load(&write_addon(dir.path(), id, m, SCRAPER));
        let problem = addon.problem.unwrap_or_default();
        assert!(problem.contains(want), "{id}: {problem}");
    }
    let addon = load(&write_addon(
        dir.path(),
        "elsewhere",
        manifest("named"),
        SCRAPER,
    ));
    assert!(addon.problem.unwrap().contains("not named `named`"));
}

fn zip(files: &[(&str, &str)]) -> Vec<u8> {
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, body) in files {
        out.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        out.write_all(body.as_bytes()).unwrap();
    }
    out.finish().unwrap().into_inner()
}

struct Package(Vec<u8>);

impl Http for Package {
    fn get(&self, _: &str) -> std::result::Result<Vec<u8>, HttpError> {
        Ok(self.0.clone())
    }
}

fn entry(package: &[u8], version: &str) -> CatalogueTheme {
    CatalogueTheme {
        id: "example".into(),
        name: "Example".into(),
        version: version.into(),
        author: None,
        description: None,
        sdk: "^1".into(),
        permissions: vec!["network".into()],
        previews: vec![],
        download: "https://example.com/example.zip".into(),
        sha256: hex(&Sha256::digest(package)),
    }
}

#[test]
fn installs_lists_disables_and_removes() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths {
        userdata: dir.path().to_path_buf(),
        systems_cfg: dir.path().join("x"),
    };
    let package = zip(&[
        ("example/addon.json", &manifest("example").to_string()),
        ("example/main.js", SCRAPER),
    ]);
    let installed = install(
        &Package(package.clone()),
        &NoShell,
        &paths,
        &entry(&package, "1.0.0"),
        true,
    )
    .unwrap();
    assert!(installed.usable(), "{:?}", installed.problem);
    assert_eq!(discover(&paths).len(), 1);
    assert!(addons_dir(&paths).join("example/main.js").is_file());

    // A hash that does not match, or another version inside: refused, the old one kept.
    let mut wrong = entry(&package, "1.0.0");
    wrong.sha256 = "0".repeat(64);
    assert!(
        install(&Package(package.clone()), &NoShell, &paths, &wrong, true)
            .unwrap_err()
            .to_string()
            .contains("does not match")
    );
    let err = install(
        &Package(package.clone()),
        &NoShell,
        &paths,
        &entry(&package, "2.0.0"),
        true,
    )
    .unwrap_err();
    assert!(err.to_string().contains("not example 2.0.0"), "{err}");
    let bad = zip(&[("addon.json", &manifest("example").to_string())]);
    assert!(
        install(
            &Package(bad.clone()),
            &NoShell,
            &paths,
            &entry(&bad, "1.0.0"),
            true
        )
        .unwrap_err()
        .to_string()
        .contains("no `main.js`")
    );
    assert!(discover(&paths)[0].usable());
    assert_eq!(
        std::fs::read_dir(addons_dir(&paths)).unwrap().count(),
        1,
        "no leftovers"
    );

    assert!(disabled(Some(r#"["example"]"#)).contains("example"));
    assert!(disabled(None).is_empty());
    remove(&NoShell, &paths, "example").unwrap();
    assert!(discover(&paths).is_empty());
    assert!(remove(&NoShell, &paths, "example").is_err());
    assert!(remove(&NoShell, &paths, "../x").is_err());
}

fn system_manifest(id: &str) -> serde_json::Value {
    json!({
        "id": id, "name": "Radio box", "version": "1.0.0", "sdk": "^1",
        "provides": ["music", "menu"], "permissions": ["network", "system"],
        "hosts": ["https://api.example.com"],
        "menu": { "label": "Radio box", "description": "Stations and a tuner" },
        "system": { "install": "system/install.sh", "remove": "system/remove.sh",
                    "commands": { "tune": "system/tune.sh" } }
    })
}

const RADIO: &str = r#"
export function stations() {
  return [
    { name: 'Chip', url: 'https://radio.example.com/chip' },
    { name: 'Not audio', url: 'file:///etc/passwd' },
    { url: 'http://radio.example.com/plain' }
  ];
}
export function page(input) {
  return { rows: [{ label: forma.system.run('tune', ['99.1', 'fm']) }] };
}"#;

fn system_package(id: &str) -> Vec<u8> {
    zip(&[
        (
            &format!("{id}/addon.json"),
            &system_manifest(id).to_string(),
        ),
        (&format!("{id}/main.js"), RADIO),
        (&format!("{id}/system/install.sh"), "echo installing"),
        (&format!("{id}/system/remove.sh"), "echo removing"),
        (&format!("{id}/system/tune.sh"), "echo tuned"),
    ])
}

fn radio_entry(package: &[u8]) -> CatalogueTheme {
    CatalogueTheme {
        id: "radio-box".into(),
        name: "Radio box".into(),
        permissions: vec!["network".into(), "system".into()],
        ..entry(package, "1.0.0")
    }
}

#[test]
fn a_system_part_runs_only_from_the_official_catalogue() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths {
        userdata: dir.path().to_path_buf(),
        systems_cfg: dir.path().join("x"),
    };
    let package = system_package("radio-box");

    // From another catalogue: refused, nothing run.
    let err = install(
        &Package(package.clone()),
        &NoShell,
        &paths,
        &radio_entry(&package),
        false,
    )
    .unwrap_err();
    assert!(
        err.to_string()
            .contains("only add-ons from Forma's own catalogue"),
        "{err}"
    );
    assert!(discover(&paths).is_empty());

    // Copied in by hand: listed, but it does not run.
    let by_hand = write_addon(
        &addons_dir(&paths),
        "radio-box",
        system_manifest("radio-box"),
        RADIO,
    );
    std::fs::create_dir_all(by_hand.join("system")).unwrap();
    for f in ["install.sh", "remove.sh", "tune.sh"] {
        std::fs::write(by_hand.join("system").join(f), "echo").unwrap();
    }
    let listed = load(&by_hand);
    assert!(!listed.usable());
    assert!(
        listed
            .problem
            .unwrap()
            .contains("runs only when installed from Forma's catalogue")
    );
    assert!(Script::new(&load(&by_hand), echo(""), Arc::new(NoShell)).is_none());
    std::fs::remove_dir_all(&by_hand).unwrap();

    // From Forma's catalogue: installed, its install script run from its folder.
    let shell = Arc::new(Recorder::default());
    let addon = install(
        &Package(package.clone()),
        &*shell,
        &paths,
        &radio_entry(&package),
        true,
    )
    .unwrap();
    assert!(addon.usable() && addon.verified, "{:?}", addon.problem);
    let ran = shell.ran.lock().unwrap().clone();
    assert_eq!(ran.len(), 1);
    assert_eq!(ran[0][0], "sh");
    assert!(
        ran[0][1].ends_with("radio-box/system/install.sh"),
        "{ran:?}"
    );

    // Its stations: streams only.
    let script = Script::new(&addon, echo(""), shell.clone()).unwrap();
    let radios = stations(&script, &Credentials::new()).unwrap();
    let urls: Vec<&str> = radios.iter().map(|r| r.url.as_str()).collect();
    assert_eq!(
        urls,
        [
            "https://radio.example.com/chip",
            "http://radio.example.com/plain"
        ]
    );
    assert_eq!(radios[1].name, "http://radio.example.com/plain");

    // Its pages may run its declared commands, with arguments.
    let page = script
        .call(&Credentials::new(), "page", &json!({}))
        .unwrap();
    assert_eq!(page["rows"][0]["label"], "ran 99.1");
    assert_eq!(
        shell.ran.lock().unwrap().last().unwrap()[2..],
        ["99.1".to_owned(), "fm".to_owned()]
    );
    let err = script
        .call(&Credentials::new(), "page", &json!({}))
        .map(|_| ());
    assert!(err.is_ok());
    let undeclared = r#"export function page() { try { forma.system.run('rm', ['-rf', '/']); } catch (e) { return e.message; } }"#;
    let dir2 = addons_dir(&paths).join("radio-box");
    std::fs::write(dir2.join("main.js"), undeclared).unwrap();
    let script = Script::new(&load(&dir2), echo(""), shell.clone()).unwrap();
    assert_eq!(
        script
            .call(&Credentials::new(), "page", &json!({}))
            .unwrap(),
        json!("it has no command `rm`")
    );

    // Removing runs its remove script first.
    remove(&*shell, &paths, "radio-box").unwrap();
    assert!(shell.ran.lock().unwrap().last().unwrap()[1].ends_with("system/remove.sh"));
    assert!(discover(&paths).is_empty());

    // A failing install script leaves nothing behind.
    let failing = Recorder {
        fail: true,
        ..Recorder::default()
    };
    let err = install(
        &Package(package.clone()),
        &failing,
        &paths,
        &radio_entry(&package),
        true,
    )
    .unwrap_err();
    assert!(err.to_string().contains("install script failed"), "{err}");
    assert!(discover(&paths).is_empty());
}

#[test]
fn a_package_cannot_mark_itself_verified() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths {
        userdata: dir.path().to_path_buf(),
        systems_cfg: dir.path().join("x"),
    };
    let manifest = json!({ "id": "sly", "name": "Sly", "version": "1.0.0", "sdk": "^1", "provides": ["scraper"], "scraper": { "gives": ["thumbnail"] } });
    let package = zip(&[
        ("sly/addon.json", &manifest.to_string()),
        ("sly/main.js", "export function lookup() { return null; }"),
        ("sly/.forma-verified", "trust me"),
    ]);
    let entry = CatalogueTheme {
        id: "sly".into(),
        name: "Sly".into(),
        ..entry(&package, "1.0.0")
    };
    let addon = install(&Package(package.clone()), &NoShell, &paths, &entry, false).unwrap();
    assert!(!addon.verified);
    assert!(!addon.dir.join(".forma-verified").exists());
}

#[test]
fn a_plain_addon_has_no_system() {
    let out = run("export function lookup() { try { forma.system.run('x'); } catch (e) { return e.message; } }", echo(""), json!({})).unwrap();
    assert_eq!(out, json!("this add-on has no system part"));
}
