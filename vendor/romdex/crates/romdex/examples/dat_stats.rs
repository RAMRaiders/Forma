//! Reads DATs and says how many dumps each lists, and how long it took:
//! `cargo run --release -p romdex --example dat_stats -- <file.dat>...`
use std::time::Instant;

fn main() {
    for path in std::env::args().skip(1) {
        let text = std::fs::read_to_string(&path).expect("readable");
        let start = Instant::now();
        let dat = romdex::dat::parse(&text).expect("a DAT");
        let mut index = romdex::dat::Index::new();
        let count = dat.entries.len();
        let with_sha1 = dat.entries.iter().filter(|e| e.rom.sha1.is_some()).count();
        index.add(romdex::dat::Catalogue::NoIntro, dat);
        println!(
            "{path}: {count} dumps, {with_sha1} with SHA1, read and indexed in {:?}",
            start.elapsed()
        );
    }
}
