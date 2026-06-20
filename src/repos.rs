//! Resolve a clone path to the bare mirror that [`sync`] keeps on disk, so the
//! SSH layer can bridge `git-upload-pack` to it. The mirrors themselves are
//! created and refreshed by [`sync`]; this is just the lookup.

use crate::sync;
use std::path::PathBuf;

/// Map a clone path ("/projects/www", "projects/www", "www", "www.git") to its
/// bare mirror dir, validating the name so it stays inside the repos dir.
pub fn resolve(path: &str) -> Option<PathBuf> {
    let p = path
        .trim()
        .trim_matches('\'')
        .trim_matches('"')
        .trim_start_matches('/');
    let name = p.rsplit('/').next().unwrap_or("");
    let name = name.strip_suffix(".git").unwrap_or(name);
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return None;
    }
    let dir = sync::repos_dir().join(format!("{name}.git"));
    if dir.join("HEAD").exists() {
        std::fs::canonicalize(dir).ok()
    } else {
        None
    }
}
