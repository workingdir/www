//! The faux shell: shared by the SSH transport and the local stdio demo.
//! Pure logic over the read-only [`vfs`]; no I/O, so it's trivially testable.

use crate::theme::*;
use crate::vfs::{self, Node};

/// Environment name from `CWD_ENV` (default `production`). Lets one binary run
/// as both production and staging on the same host with an observable marker.
pub fn env_name() -> String {
    std::env::var("CWD_ENV").unwrap_or_else(|_| "production".to_string())
}

/// A one-line banner appended to the welcome screen on non-production instances,
/// so `curl`/`ssh` against staging is visibly distinct.
fn env_banner() -> String {
    let e = env_name();
    if e == "production" {
        String::new()
    } else {
        format!("   {BLUE}[{e}]{RESET}")
    }
}

pub struct Output {
    pub out: String,
    pub quit: bool,
}
impl Output {
    fn text(s: String) -> Self {
        Output {
            out: s,
            quit: false,
        }
    }
}

pub struct Shell {
    root: Node,
    cwd: Vec<String>,
}

impl Default for Shell {
    fn default() -> Self {
        Self::new()
    }
}

impl Shell {
    pub fn new() -> Self {
        Shell {
            root: vfs::root(),
            cwd: Vec::new(),
        }
    }

    fn abspath(&self) -> String {
        if self.cwd.is_empty() {
            String::new()
        } else {
            format!("/{}", self.cwd.join("/"))
        }
    }

    /// The shell prompt, e.g. `~/cwd/projects $`.
    pub fn prompt(&self) -> String {
        format!("{BLUE}~/cwd{}{RESET} {MUTED}${RESET} ", self.abspath())
    }

    /// Welcome screen, shown on connect and via `home`. Also what `curl cwd.dev` returns.
    pub fn motd(&self) -> String {
        format!(
"
  {BLUE}./{RESET} {BOLD}cwd{RESET} {MUTED}— current working directory{RESET}{}

  the home directory for everything i make.
  {MUTED}a read-only shell — nothing here can be changed, nothing's watching.{RESET}

  {BLUE}ls{RESET} list   {BLUE}cd{RESET} enter   {BLUE}cat{RESET} read   {BLUE}tree{RESET} map   {BLUE}open{RESET} on github   {BLUE}help{RESET} more

",
            env_banner()
        )
    }

    pub fn help(&self) -> String {
        format!(
            "  {BLUE}ls{RESET} [path]      list a directory
  {BLUE}cd{RESET} <path>      change directory ( .. up, / or ~ for root )
  {BLUE}pwd{RESET}           print where you are
  {BLUE}cat{RESET} <file>    print a file
  {BLUE}tree{RESET}          show everything from here down
  {BLUE}open{RESET} <name>   the github url for a project
  {BLUE}clone{RESET} <name>  git clone a project over ssh
  {BLUE}home{RESET}          the welcome screen
  {BLUE}whoami{RESET}        who you are here
  {BLUE}clear{RESET}         clear the screen
  {BLUE}exit{RESET}          disconnect
"
        )
    }

    /// Resolve `arg` to a list of path segments (does not validate existence/type).
    fn resolve(&self, arg: &str) -> Vec<String> {
        let mut path = if arg.starts_with('/') || arg == "~" || arg.starts_with("~/") {
            Vec::new()
        } else {
            self.cwd.clone()
        };
        let a = arg.trim_start_matches('~').trim_start_matches('/');
        for part in a.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    path.pop();
                }
                p => path.push(p.to_string()),
            }
        }
        path
    }

    /// Tab-completion candidates for the last token of `line`, plus the byte
    /// offset where that token begins (so the caller can splice in a choice).
    pub fn complete(&self, line: &str) -> (Vec<String>, usize) {
        let start = line.rfind(char::is_whitespace).map(|i| i + 1).unwrap_or(0);
        let token = &line[start..];
        let completing_command = line[..start].trim().is_empty();
        if completing_command {
            const CMDS: &[&str] = &[
                "ls", "cd", "pwd", "cat", "tree", "open", "whoami", "home", "clear", "help", "exit",
            ];
            let cands = CMDS
                .iter()
                .filter(|c| c.starts_with(token))
                .map(|c| c.to_string())
                .collect();
            (cands, start)
        } else {
            let (dirpart, leaf) = match token.rfind('/') {
                Some(i) => (&token[..=i], &token[i + 1..]),
                None => ("", token),
            };
            let segs = self.resolve(dirpart);
            let cands = match self.root.resolve(&segs) {
                Some(node @ Node::Dir(_)) => node
                    .entries()
                    .into_iter()
                    .filter(|(name, _)| {
                        name.starts_with(leaf) && (leaf.starts_with('.') || !name.starts_with('.'))
                    })
                    .map(|(name, is_dir)| {
                        format!("{dirpart}{name}{}", if is_dir { "/" } else { "" })
                    })
                    .collect(),
                _ => Vec::new(),
            };
            (cands, start)
        }
    }

    pub fn exec(&mut self, line: &str) -> Output {
        let line = line.trim();
        if line.is_empty() {
            return Output::text(String::new());
        }
        let mut it = line.split_whitespace();
        let cmd = it.next().unwrap_or("");
        let args: Vec<&str> = it.collect();
        let arg = args.first().copied().unwrap_or("");
        match cmd {
            "help" | "?" | "man" => Output::text(self.help()),
            "ls" | "dir" => self.ls(&args),
            "cd" => self.cd(arg),
            "pwd" => Output::text(format!("/cwd{}\n", self.abspath())),
            "cat" | "less" | "more" | "read" => self.cat(arg),
            "tree" => Output::text(self.tree()),
            "open" => self.open(arg),
            "clone" => self.clone_hint(arg),
            "whoami" => Output::text(format!("{}\n", muted("guest@cwd — a read-only visitor"))),
            "home" | "motd" => Output::text(self.motd()),
            "clear" => Output {
                out: CLEAR.to_string(),
                quit: false,
            },
            "exit" | "quit" | "logout" | "q" => Output {
                out: muted("bye.\n"),
                quit: true,
            },
            other => Output::text(format!(
                "{}: command not found — try {}\n",
                other,
                blue("help")
            )),
        }
    }

    fn ls(&self, args: &[&str]) -> Output {
        let show_all = args.iter().any(|a| a.starts_with('-') && a.contains('a'));
        let patharg = args
            .iter()
            .find(|a| !a.starts_with('-'))
            .copied()
            .unwrap_or("");
        let path = if patharg.is_empty() {
            self.cwd.clone()
        } else {
            self.resolve(patharg)
        };
        match self.root.resolve(&path) {
            Some(node @ Node::Dir(_)) => {
                let items: Vec<String> = node
                    .entries()
                    .into_iter()
                    .filter(|(name, _)| show_all || !name.starts_with('.'))
                    .map(|(name, is_dir)| {
                        if is_dir {
                            format!("{BLUE}{name}/{RESET}")
                        } else {
                            name
                        }
                    })
                    .collect();
                if items.is_empty() {
                    Output::text(muted("(empty)\n"))
                } else {
                    Output::text(format!("{}\n", items.join("   ")))
                }
            }
            Some(Node::File(_)) => Output::text(format!("{patharg}\n")),
            None => Output::text(format!("ls: {patharg}: no such directory\n")),
        }
    }

    fn cd(&mut self, arg: &str) -> Output {
        if arg.is_empty() || arg == "~" || arg == "/" {
            self.cwd.clear();
            return Output::text(String::new());
        }
        let path = self.resolve(arg);
        match self.root.resolve(&path) {
            Some(Node::Dir(_)) => {
                self.cwd = path;
                Output::text(String::new())
            }
            Some(Node::File(_)) => Output::text(format!("cd: {}: not a directory\n", arg)),
            None => Output::text(format!("cd: {}: no such directory\n", arg)),
        }
    }

    fn cat(&self, arg: &str) -> Output {
        if arg.is_empty() {
            return Output::text("cat: missing file\n".to_string());
        }
        let path = self.resolve(arg);
        match self.root.resolve(&path) {
            Some(Node::File(c)) => {
                let mut c = c.clone();
                if !c.ends_with('\n') {
                    c.push('\n');
                }
                Output::text(c)
            }
            Some(Node::Dir(_)) => Output::text(format!("cat: {}: is a directory\n", arg)),
            None => Output::text(format!("cat: {}: no such file\n", arg)),
        }
    }

    fn open(&self, arg: &str) -> Output {
        let name = if arg.is_empty() {
            self.cwd.last().cloned().unwrap_or_default()
        } else {
            self.resolve(arg).last().cloned().unwrap_or_default()
        };
        if name.is_empty() {
            return Output::text("open: name a project, e.g. `open helio`\n".to_string());
        }
        Output::text(format!(
            "{}\n",
            blue(&format!("https://github.com/workingdir/{name}"))
        ))
    }

    fn clone_hint(&self, arg: &str) -> Output {
        let name = if arg.is_empty() {
            self.cwd.last().cloned().unwrap_or_default()
        } else {
            self.resolve(arg).last().cloned().unwrap_or_default()
        };
        if name.is_empty() {
            return Output::text("clone: name a project, e.g. `clone helio`\n".to_string());
        }
        Output::text(format!(
            "{}\n",
            blue(&format!("git clone ssh://cwd.dev/projects/{name}"))
        ))
    }

    fn tree(&self) -> String {
        let start = self.root.resolve(&self.cwd).unwrap_or(&self.root);
        let mut out = String::from(".\n");
        fn walk(node: &Node, prefix: &str, out: &mut String) {
            if let Node::Dir(_) = node {
                let entries: Vec<(String, bool)> = node
                    .entries()
                    .into_iter()
                    .filter(|(name, _)| !name.starts_with('.'))
                    .collect();
                let n = entries.len();
                for (i, (name, is_dir)) in entries.into_iter().enumerate() {
                    let last = i + 1 == n;
                    let branch = if last { "└─ " } else { "├─ " };
                    let label = if is_dir {
                        format!("{BLUE}{name}{RESET}")
                    } else {
                        name.clone()
                    };
                    out.push_str(&format!("{prefix}{branch}{label}\n"));
                    if is_dir {
                        let child = node.child(&name).unwrap();
                        let next = format!("{prefix}{}", if last { "   " } else { "│  " });
                        walk(child, &next, out);
                    }
                }
            }
        }
        walk(start, "", &mut out);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip(s: &str) -> String {
        // crude ANSI strip for assertions
        let mut out = String::new();
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                while let Some(&n) = chars.peek() {
                    chars.next();
                    if n == 'm' {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    #[test]
    fn lists_root() {
        let mut sh = Shell::new();
        let o = strip(&sh.exec("ls").out);
        assert!(o.contains("projects"));
        assert!(o.contains("README.md"));
    }

    #[test]
    fn navigates_and_reads() {
        let mut sh = Shell::new();
        sh.exec("cd projects");
        assert!(strip(&sh.exec("ls").out).contains("helio"));
        sh.exec("cd helio");
        assert_eq!(strip(&sh.prompt()).trim(), "~/cwd/projects/helio $");
        assert!(strip(&sh.exec("cat README.md").out).contains("helio"));
        sh.exec("cd ..");
        sh.exec("cd ..");
        assert_eq!(strip(&sh.prompt()).trim(), "~/cwd $");
    }

    #[test]
    fn cat_errors() {
        let mut sh = Shell::new();
        assert!(strip(&sh.exec("cat nope").out).contains("no such file"));
        assert!(strip(&sh.exec("cat projects").out).contains("is a directory"));
    }

    #[test]
    fn open_builds_url() {
        let mut sh = Shell::new();
        assert!(sh
            .exec("open helio")
            .out
            .contains("github.com/workingdir/helio"));
    }

    #[test]
    fn quit_sets_flag() {
        let mut sh = Shell::new();
        assert!(sh.exec("exit").quit);
    }
}
