//! Where a screen export writes its frames: `$SHOTS/<folder>` in a shot run, else the view's own
//! `*_SCREEN_EXPORT` (an absolute path, or `target/<folder>` of the workspace), else nowhere.
use std::path::{Path, PathBuf};

pub fn screens(var: &str, folder: &str) -> Option<PathBuf> {
    let out = match (std::env::var_os("SHOTS"), std::env::var_os(var)) {
        (Some(shots), _) => Path::new(&shots).join(folder),
        (None, Some(dir)) if Path::new(&dir).is_absolute() => PathBuf::from(dir),
        (None, Some(_)) => Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../target")
            .join(folder),
        (None, None) => return None,
    };
    std::fs::create_dir_all(&out).unwrap();
    Some(out)
}
