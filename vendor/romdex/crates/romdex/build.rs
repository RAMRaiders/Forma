//! Compiles the vendored rcheevos hashing (`vendor/rcheevos`): plain C, no libclang needed.

fn main() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/rcheevos");
    let rhash = root.join("src/rhash");
    let mut build = cc::Build::new();
    build
        .include(root.join("include"))
        .include(root.join("src"))
        .define("RC_STATIC", None)
        .define("RC_DISABLE_LUA", None)
        // Upstream's code, compiled as it is: its warnings are not ours to fix here.
        .warnings(false)
        .file(root.join("src/rc_compat.c"));
    for entry in std::fs::read_dir(&rhash).expect("vendor/rcheevos/src/rhash") {
        let path = entry.expect("entry").path();
        if path.extension().is_some_and(|e| e == "c") {
            build.file(&path);
        }
    }
    build.compile("rcheevos_hash");
    println!("cargo::rerun-if-changed={}", root.display());
}
