//! Times identifying a generated library, then re-scanning it unchanged.
//!
//! cargo run --release -p romdex --example bench_identify -- [systems] [files-per-system] [kib]

use std::time::Instant;

use romdex::batocera::Target;
use romdex::cache::Cache;

fn main() {
    let mut args = std::env::args()
        .skip(1)
        .map(|a| a.parse::<usize>().expect("a number"));
    let systems = args.next().unwrap_or(10);
    let per_system = args.next().unwrap_or(2_000);
    let kib = args.next().unwrap_or(4);

    let dir = tempfile::tempdir().expect("temp dir");
    let mut targets = Vec::new();
    for s in 0..systems {
        let root = dir.path().join(format!("roms/sys{s}"));
        std::fs::create_dir_all(&root).expect("mkdir");
        for f in 0..per_system {
            let body: Vec<u8> = (0..kib * 1024)
                .map(|i| ((i * 7 + f * 13 + s) % 256) as u8)
                .collect();
            std::fs::write(root.join(format!("Game {f}.rom")), body).expect("write");
        }
        targets.push(Target {
            system: format!("sys{s}"),
            root,
            extensions: vec![".rom".into()],
        });
    }
    let mut cache = Cache::open(&dir.path().join("cache.db")).expect("cache");

    let t = Instant::now();
    let (_, first) = romdex::identify(&targets, &mut cache).expect("identify");
    let first_time = t.elapsed();
    let t = Instant::now();
    let (_, again) = romdex::identify(&targets, &mut cache).expect("identify");
    let again_time = t.elapsed();

    println!("{} files of {kib} KiB in {systems} systems", first.files);
    println!(
        "first scan:  {:>8.0} ms  hashed {}, read {} MiB",
        first_time.as_secs_f64() * 1000.0,
        first.hashed,
        first.bytes_read / (1024 * 1024)
    );
    println!(
        "re-scan:     {:>8.0} ms  hashed {}, from cache {}, read {} bytes",
        again_time.as_secs_f64() * 1000.0,
        again.hashed,
        again.cached,
        again.bytes_read
    );
}
