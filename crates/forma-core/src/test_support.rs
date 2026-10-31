//! Shared test helpers.

use std::path::PathBuf;

use crate::paths::Paths;

/// The repository's fake Batocera install (`fixtures/`).
pub(crate) fn fixture_paths() -> Paths {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .canonicalize()
        .expect("fixtures/ exists");
    Paths {
        userdata: root.join("userdata"),
        systems_cfg: root.join("usr-share-emulationstation/es_systems.cfg"),
    }
}

/// A writable copy of the fixtures, for tests that change files on disk.
pub(crate) fn fixture_copy() -> (tempfile::TempDir, Paths) {
    let src = fixture_paths();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    copy_dir(&src.userdata, &root.join("userdata"));
    std::fs::copy(&src.systems_cfg, root.join("es_systems.cfg")).unwrap();
    let paths = Paths {
        userdata: root.join("userdata"),
        systems_cfg: root.join("es_systems.cfg"),
    };
    (dir, paths)
}

fn copy_dir(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}
