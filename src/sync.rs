//! Mirror the public repos of the GitHub org and keep an in-memory list of them.
//! `git clone ssh://cwd.dev/projects/<repo>` serves these mirrors, and the shell
//! listing comes from the same snapshot. Read-only: we only ever fetch.
//!
//! It shells out to curl + jq for the org listing and git for the mirroring,
//! matching how the rest of the app orchestrates CLI tools. Everything is
//! best-effort: if the network or a tool is missing, the previous snapshot
//! stands and the site still runs.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::RwLock;
use std::time::Duration;

#[derive(Clone)]
pub struct Repo {
    pub name: String,
    pub description: String,
}

static SNAPSHOT: RwLock<Vec<Repo>> = RwLock::new(Vec::new());

/// The repos known right now. Cheap; safe to call per request.
pub fn current() -> Vec<Repo> {
    SNAPSHOT.read().map(|s| s.clone()).unwrap_or_default()
}

fn org() -> String {
    std::env::var("CWD_ORG").unwrap_or_else(|_| "workingdir".to_string())
}

pub fn repos_dir() -> PathBuf {
    std::env::var("CWD_REPOS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("repos"))
}

fn interval_secs() -> u64 {
    std::env::var("CWD_SYNC_INTERVAL")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(900)
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

/// Start syncing in the background: an immediate pass, then every interval.
pub fn start() {
    std::thread::spawn(|| loop {
        refresh();
        std::thread::sleep(Duration::from_secs(interval_secs()));
    });
}

/// One sync pass: list the org's public repos, mirror or update each, publish.
pub fn refresh() {
    let list = match fetch_repos() {
        Some(l) => l,
        None => return,
    };
    let dir = repos_dir();
    let _ = std::fs::create_dir_all(&dir);
    for r in &list {
        mirror(&dir, &r.name);
    }
    if let Ok(mut s) = SNAPSHOT.write() {
        *s = list;
    }
}

fn fetch_repos() -> Option<Vec<Repo>> {
    let auth = match std::env::var("CWD_GITHUB_TOKEN") {
        Ok(t) if !t.is_empty() => format!("-H 'Authorization: Bearer {t}'"),
        _ => String::new(),
    };
    let url = format!(
        "https://api.github.com/orgs/{}/repos?per_page=100&type=public",
        org()
    );
    // One shell pipeline: curl the listing, jq it to "name<TAB>description" lines.
    let script = format!(
        "curl -fsSL -H 'Accept: application/vnd.github+json' -H 'User-Agent: cwd' {auth} '{url}' \
         | jq -r '.[] | select(.archived|not) | [.name, (.description//\"\")] | @tsv'"
    );
    let out = Command::new("sh").arg("-c").arg(&script).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut repos = Vec::new();
    for line in text.lines() {
        let mut it = line.splitn(2, '\t');
        let name = it.next().unwrap_or("").trim().to_string();
        let description = it.next().unwrap_or("").trim().to_string();
        if valid_name(&name) {
            repos.push(Repo { name, description });
        }
    }
    if repos.is_empty() {
        None
    } else {
        Some(repos)
    }
}

fn mirror(dir: &Path, name: &str) {
    if !valid_name(name) {
        return;
    }
    let path = dir.join(format!("{name}.git"));
    let url = format!("https://github.com/{}/{}.git", org(), name);
    if path.join("HEAD").exists() {
        let _ = Command::new("git")
            .arg("--git-dir")
            .arg(&path)
            .args(["remote", "update", "--prune"])
            .output();
    } else {
        let _ = Command::new("git")
            .args(["clone", "--mirror", &url])
            .arg(&path)
            .output();
    }
}
