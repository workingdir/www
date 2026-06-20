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
    pub default_branch: String,
    /// File paths in the repo (from the GitHub tree API), used to build the
    /// virtual listing under `projects/<name>`. Empty until the first sync.
    pub tree: Vec<String>,
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

/// A repo-relative path is safe to descend into: no absolute, empty, `.`, `..`
/// or control-char segments. Tree paths from GitHub are already well-formed;
/// this guards the value we pass to `git cat-file` regardless.
fn safe_path(p: &str) -> bool {
    !p.is_empty()
        && !p.starts_with('/')
        && !p.contains(['\n', '\0'])
        && !p.split('/').any(|s| s.is_empty() || s == "." || s == "..")
}

/// `-H 'Authorization: ...'` if a token is set, else empty. Optional: raises the
/// GitHub rate limit and reaches private repos when present.
fn auth_header() -> String {
    match std::env::var("CWD_GITHUB_TOKEN") {
        Ok(t) if !t.is_empty() => format!("-H 'Authorization: Bearer {t}'"),
        _ => String::new(),
    }
}

/// Start syncing in the background: an immediate pass, then every interval.
pub fn start() {
    std::thread::spawn(|| loop {
        refresh();
        std::thread::sleep(Duration::from_secs(interval_secs()));
    });
}

/// One sync pass: list the org's public repos, mirror or update each, fetch its
/// file tree for the virtual listing, then publish the snapshot.
pub fn refresh() {
    let mut list = match fetch_repos() {
        Some(l) => l,
        None => return,
    };
    let dir = repos_dir();
    let _ = std::fs::create_dir_all(&dir);
    for r in &mut list {
        mirror(&dir, &r.name);
        r.tree = fetch_tree(&r.name, &r.default_branch).unwrap_or_default();
    }
    if let Ok(mut s) = SNAPSHOT.write() {
        *s = list;
    }
}

fn fetch_repos() -> Option<Vec<Repo>> {
    let auth = auth_header();
    let url = format!(
        "https://api.github.com/orgs/{}/repos?per_page=100&type=public",
        org()
    );
    // One shell pipeline: curl the listing, jq it to "name<TAB>desc<TAB>branch".
    let script = format!(
        "curl -fsSL -H 'Accept: application/vnd.github+json' -H 'User-Agent: cwd' {auth} '{url}' \
         | jq -r '.[] | select(.archived|not) | [.name, (.description//\"\"), (.default_branch//\"main\")] | @tsv'"
    );
    let out = Command::new("sh").arg("-c").arg(&script).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut repos = Vec::new();
    for line in text.lines() {
        let mut it = line.splitn(3, '\t');
        let name = it.next().unwrap_or("").trim().to_string();
        let description = it.next().unwrap_or("").trim().to_string();
        let branch = it.next().unwrap_or("").trim();
        let default_branch = if branch.is_empty() {
            "main".to_string()
        } else {
            branch.to_string()
        };
        if valid_name(&name) {
            repos.push(Repo {
                name,
                description,
                default_branch,
                tree: Vec::new(),
            });
        }
    }
    if repos.is_empty() {
        None
    } else {
        Some(repos)
    }
}

/// The repo's file paths, from the GitHub tree API (recursive, files only).
/// Cached in the snapshot so the per-connection listing costs nothing.
fn fetch_tree(name: &str, branch: &str) -> Option<Vec<String>> {
    if !valid_name(name) {
        return None;
    }
    let auth = auth_header();
    let url = format!(
        "https://api.github.com/repos/{}/{}/git/trees/{}?recursive=1",
        org(),
        name,
        branch
    );
    let script = format!(
        "curl -fsSL -H 'Accept: application/vnd.github+json' -H 'User-Agent: cwd' {auth} '{url}' \
         | jq -r '.tree[] | select(.type==\"blob\") | .path'"
    );
    let out = Command::new("sh").arg("-c").arg(&script).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut paths: Vec<String> = text
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|p| safe_path(p))
        .collect();
    paths.sort();
    if paths.is_empty() {
        None
    } else {
        Some(paths)
    }
}

/// Read one file's contents from the local mirror (the cloned GitHub repo).
/// Lazy: called the first time a file is `cat`-ed, not at listing time. Returns
/// a friendly note for binary or oversized files rather than dumping bytes.
pub fn blob(name: &str, path: &str) -> Option<String> {
    if !valid_name(name) || !safe_path(path) {
        return None;
    }
    let dir = repos_dir().join(format!("{name}.git"));
    if !dir.join("HEAD").exists() {
        return None;
    }
    let out = Command::new("git")
        .arg("--git-dir")
        .arg(&dir)
        .args(["cat-file", "blob"])
        .arg(format!("HEAD:{path}"))
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    const MAX: usize = 256 * 1024;
    match String::from_utf8(out.stdout) {
        Ok(s) if s.len() <= MAX => Some(s),
        Ok(s) => {
            let mut end = MAX;
            while !s.is_char_boundary(end) {
                end -= 1;
            }
            Some(format!(
                "{}\n[truncated — {} bytes total]\n",
                &s[..end],
                s.len()
            ))
        }
        Err(e) => Some(format!("[binary file — {} bytes]\n", e.as_bytes().len())),
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
