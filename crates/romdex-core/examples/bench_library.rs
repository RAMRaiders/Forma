//! Times the library against a generated collection.
//!
//! cargo run --release -p romdex-core --example bench_library -- [systems] [games-per-system]
//!     [--keep DIR]
//!
//! `--keep DIR` writes the generated `/userdata` and `es_systems.cfg` to DIR and leaves them, to
//! copy to a device for the Batocera spike (#39); the paths inside are written for DIR.
//!
//! Writes empty ROM files and a full `gamelist.xml` per system into a temporary `/userdata`,
//! then measures what a user feels: the first import, an unchanged refresh, cold boot to the
//! first rendered list, and search.

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use romdex_core::{Library, Paths};

const GENRES: &[&str] = &[
    "Platform",
    "Shooter",
    "Puzzle",
    "Racing",
    "Role Playing Game",
];
const WORDS: &[&str] = &[
    "Super", "Mega", "Dragon", "Quest", "Star", "Fighter", "Legend", "Turbo", "Ninja", "Castle",
];

fn main() {
    let mut keep: Option<std::path::PathBuf> = None;
    let mut numbers = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--keep" {
            keep = Some(args.next().expect("--keep needs a folder").into());
        } else {
            numbers.push(arg.parse::<usize>().expect("a number"));
        }
    }
    let systems = numbers.first().copied().unwrap_or(10);
    let per_system = numbers.get(1).copied().unwrap_or(2_000);

    let temp = tempfile::tempdir().expect("temp dir");
    let root = match &keep {
        Some(dir) => {
            std::fs::create_dir_all(dir).expect("create --keep folder");
            dir.canonicalize().expect("--keep folder")
        }
        None => temp.path().to_path_buf(),
    };
    let paths = Paths {
        userdata: root.join("userdata"),
        systems_cfg: root.join("es_systems.cfg"),
    };
    let generated = Instant::now();
    generate(&paths, systems, per_system);
    println!(
        "generated {} games in {systems} systems ({:.1?})\n",
        systems * per_system,
        generated.elapsed()
    );

    let db = paths.database();
    let mut lib = Library::open(&db).expect("open");
    let (report, first) = time(|| lib.refresh(&paths).expect("refresh"));
    assert_eq!(report.games, systems * per_system);
    row("first import (scan + parse every gamelist)", first);

    let (_, again) = time(|| lib.refresh(&paths).expect("refresh"));
    row("refresh, nothing changed (scan only)", again);
    drop(lib);

    let (count, boot) = time(|| {
        let lib = Library::open(&db).expect("open");
        let first_system = lib.systems().expect("systems")[0].name.clone();
        lib.games(&first_system).expect("games").len()
    });
    assert_eq!(count, per_system);
    row(&format!("cold boot to first list ({count} games)"), boot);

    let lib = Library::open(&db).expect("open");
    if keep.is_some() {
        println!(
            "\nkept: ROMDEX_USERDATA={} ROMDEX_SYSTEMS_CFG={}",
            paths.userdata.display(),
            paths.systems_cfg.display()
        );
    }

    for query in ["dragon", "super star", "ninja castle quest"] {
        let runs = 50;
        let (hits, total) = time(|| {
            (0..runs)
                .map(|_| lib.search(query, 50).expect("search").len())
                .last()
                .unwrap_or(0)
        });
        row(
            &format!("search `{query}` ({hits} hits, top 50)"),
            total / runs,
        );
    }
}

fn time<T>(f: impl FnOnce() -> T) -> (T, Duration) {
    let start = Instant::now();
    let out = f();
    (out, start.elapsed())
}

fn row(label: &str, d: Duration) {
    println!("{label:<52} {:>9.2} ms", d.as_secs_f64() * 1000.0);
}

fn generate(paths: &Paths, systems: usize, per_system: usize) {
    let mut cfg = String::from("<systemList>");
    for s in 0..systems {
        let name = format!("sys{s}");
        let dir = paths.roms().join(&name);
        std::fs::create_dir_all(&dir).expect("mkdir");
        write!(
            cfg,
            "<system><name>{name}</name><fullname>System {s}</fullname>\
             <manufacturer>Maker {}</manufacturer><path>/userdata/roms/{name}</path>\
             <extension>.rom</extension></system>",
            s % 3
        )
        .unwrap();

        let mut list = String::from("<?xml version=\"1.0\"?>\n<gameList>\n");
        for g in 0..per_system {
            let title = format!(
                "{} {} {} {g}",
                WORDS[g % WORDS.len()],
                WORDS[(g / 7) % WORDS.len()],
                WORDS[(g / 13) % WORDS.len()]
            );
            let file = format!("{title}.rom");
            std::fs::write(dir.join(&file), b"").expect("rom");
            writeln!(
                list,
                "<game><path>./{file}</path><name>{title}</name>\
                 <desc>A generated game about {} and {}, number {g}.</desc>\
                 <image>./images/{title}.png</image><rating>0.{}</rating>\
                 <releasedate>19{:02}0101T000000</releasedate><developer>Studio {}</developer>\
                 <publisher>Publisher {}</publisher><genre>{}</genre><players>1-{}</players>\
                 </game>",
                WORDS[(g / 3) % WORDS.len()],
                WORDS[(g / 5) % WORDS.len()],
                g % 10,
                80 + g % 20,
                g % 50,
                g % 20,
                GENRES[g % GENRES.len()],
                1 + g % 4
            )
            .unwrap();
        }
        list.push_str("</gameList>\n");
        std::fs::write(dir.join("gamelist.xml"), list).expect("gamelist");
    }
    cfg.push_str("</systemList>");
    std::fs::write(&paths.systems_cfg, cfg).expect("cfg");
}
