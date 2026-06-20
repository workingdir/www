//! Real git repositories, materialised from the read-only [`vfs`] so that
//! `git clone` works over SSH. Each project under `projects/` becomes a git
//! repo on disk (default `./repos`, override `CWD_REPOS`); the SSH layer then
//! bridges `git-upload-pack` to the system `git` binary against these.

use crate::vfs::{self, Node};
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn base_dir() -> PathBuf {
    std::env::var("CWD_REPOS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("repos"))
}

/// Map a clone path ("/projects/helio", "projects/helio", "helio", "helio.git")
/// to a built repo dir, validating the name to keep it inside `repos/`.
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
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    let dir = base_dir().join(name);
    if dir.join(".git").is_dir() {
        std::fs::canonicalize(dir).ok()
    } else {
        None
    }
}

/// Build a git repo for every project once, at startup. Idempotent.
pub fn ensure() {
    let base = base_dir();
    if std::fs::create_dir_all(&base).is_err() {
        eprintln!("repos: could not create {}", base.display());
        return;
    }
    let root = vfs::root();
    let Some(Node::Dir(projects)) = root.child("projects") else {
        return;
    };
    for (name, node) in projects {
        let repo = base.join(name);
        if repo.join(".git").is_dir() {
            continue; // already built
        }
        let _ = std::fs::create_dir_all(&repo);
        write_tree(node, &repo);
        init_commit(&repo);
        eprintln!("repo ready → {}", repo.display());
    }
}

fn write_tree(node: &Node, at: &Path) {
    if let Node::Dir(m) = node {
        for (name, child) in m {
            let p = at.join(name);
            match child {
                Node::Dir(_) => {
                    let _ = std::fs::create_dir_all(&p);
                    write_tree(child, &p);
                }
                Node::File(c) => {
                    let _ = std::fs::write(&p, c);
                }
            }
        }
    }
}

fn init_commit(repo: &Path) {
    let git = |args: &[&str]| {
        Command::new("git")
            .current_dir(repo)
            .args(args)
            .env("GIT_AUTHOR_NAME", "cwd")
            .env("GIT_AUTHOR_EMAIL", "hello@cwd.dev")
            .env("GIT_COMMITTER_NAME", "cwd")
            .env("GIT_COMMITTER_EMAIL", "hello@cwd.dev")
            .output()
    };
    let _ = git(&["init", "-q", "-b", "main"]);
    let _ = git(&["add", "-A"]);
    let _ = git(&["commit", "-q", "-m", "import from cwd"]);
}
