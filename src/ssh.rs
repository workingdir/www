//! SSH transport: wraps the shared [`Shell`] in an interactive russh session
//! with a small readline-style line editor: history (↑/↓), cursor movement
//! (←/→, Home/End), Delete, and Ctrl-A/E/U/W/L/C/D. Anonymous, read-only.
//! Also handles one-shot `ssh cwd.dev "ls projects"`.

use crate::shell::Shell;
use async_trait::async_trait;
use russh::server::{self, Auth, Handler, Msg, Server as _, Session};
use russh::{Channel, ChannelId, CryptoVec, Pty};
use russh_keys::key::{KeyPair, PublicKey};
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command as Cmd;

pub fn serve(addr: &str) {
    crate::repos::ensure();
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    rt.block_on(async move {
        let key = host_key();
        let config = Arc::new(server::Config {
            keys: vec![key],
            ..Default::default()
        });
        let mut srv = AppServer;
        srv.run_on_address(config, addr)
            .await
            .expect("ssh: run_on_address");
    });
}

/// Load a persistent ed25519 host key, generating + saving one on first run so
/// clients don't see "REMOTE HOST IDENTIFICATION HAS CHANGED" across restarts.
fn host_key() -> KeyPair {
    let path = std::env::var("CWD_HOSTKEY").unwrap_or_else(|_| "cwd_host_ed25519".to_string());
    if let Ok(k) = russh_keys::load_secret_key(&path, None) {
        eprintln!("host key ← {path}");
        return k;
    }
    let k = KeyPair::generate_ed25519().expect("ed25519 host key");
    match std::fs::File::create(&path) {
        Ok(mut f) => {
            if let Err(e) = russh_keys::encode_pkcs8_pem(&k, &mut f) {
                eprintln!("warning: could not persist host key ({e}); it will change on restart");
            } else {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
                }
                eprintln!("host key → generated and saved to {path}");
            }
        }
        Err(e) => {
            eprintln!("warning: could not create {path} ({e}); host key will change on restart")
        }
    }
    k
}

/// Longest common prefix of a set of completion candidates.
fn common_prefix(items: &[String]) -> String {
    let mut p = match items.first() {
        Some(s) => s.clone(),
        None => return String::new(),
    };
    for s in &items[1..] {
        while !s.starts_with(&p) {
            p.pop();
            if p.is_empty() {
                return String::new();
            }
        }
    }
    p
}

struct AppServer;
impl server::Server for AppServer {
    type Handler = Client;
    fn new_client(&mut self, _peer: Option<std::net::SocketAddr>) -> Client {
        Client::default()
    }
}

/// A minimal line editor (ASCII; every accepted char is one byte = one column).
#[derive(Default)]
struct Editor {
    buf: String,
    pos: usize,
    hist: Vec<String>,
    idx: usize,
    draft: String,
}

impl Editor {
    fn insert(&mut self, c: char) {
        self.buf.insert(self.pos, c);
        self.pos += 1;
    }
    fn backspace(&mut self) {
        if self.pos > 0 {
            self.pos -= 1;
            self.buf.remove(self.pos);
        }
    }
    fn delete(&mut self) {
        if self.pos < self.buf.len() {
            self.buf.remove(self.pos);
        }
    }
    fn left(&mut self) {
        self.pos = self.pos.saturating_sub(1);
    }
    fn right(&mut self) {
        if self.pos < self.buf.len() {
            self.pos += 1;
        }
    }
    fn home(&mut self) {
        self.pos = 0;
    }
    fn end(&mut self) {
        self.pos = self.buf.len();
    }
    fn kill_line(&mut self) {
        self.buf.clear();
        self.pos = 0;
    }
    fn kill_word(&mut self) {
        while self.pos > 0 && self.buf.as_bytes()[self.pos - 1] == b' ' {
            self.backspace();
        }
        while self.pos > 0 && self.buf.as_bytes()[self.pos - 1] != b' ' {
            self.backspace();
        }
    }
    fn up(&mut self) {
        if self.idx > 0 {
            if self.idx == self.hist.len() {
                self.draft = self.buf.clone();
            }
            self.idx -= 1;
            self.buf = self.hist[self.idx].clone();
            self.pos = self.buf.len();
        }
    }
    fn down(&mut self) {
        if self.idx < self.hist.len() {
            self.idx += 1;
            self.buf = if self.idx == self.hist.len() {
                std::mem::take(&mut self.draft)
            } else {
                self.hist[self.idx].clone()
            };
            self.pos = self.buf.len();
        }
    }
    /// Take the current line, record it in history, reset for the next one.
    fn take(&mut self) -> String {
        let line = std::mem::take(&mut self.buf);
        self.pos = 0;
        if !line.trim().is_empty() && self.hist.last() != Some(&line) {
            self.hist.push(line.clone());
        }
        self.idx = self.hist.len();
        self.draft.clear();
        line
    }
}

#[derive(Default)]
struct Client {
    shell: Shell,
    ed: Editor,
    esc: u8,                                       // 0 normal, 1 after ESC, 2 inside CSI
    param: String,                                 // CSI numeric parameter
    git_stdin: Option<tokio::process::ChildStdin>, // set while bridging git-upload-pack
}

impl Client {
    fn send(&self, session: &mut Session, channel: ChannelId, s: &str) {
        let crlf = s.replace("\r\n", "\n").replace('\n', "\r\n");
        session.data(channel, CryptoVec::from(crlf.into_bytes()));
    }

    /// Repaint the current input line in place: CR, clear, prompt, buffer, then
    /// park the cursor at `pos`.
    fn redraw(&self, session: &mut Session, channel: ChannelId) {
        let mut s = format!("\r\x1b[K{}{}", self.shell.prompt(), self.ed.buf);
        let back = self.ed.buf.len() - self.ed.pos;
        if back > 0 {
            s.push_str(&format!("\x1b[{back}D"));
        }
        session.data(channel, CryptoVec::from(s.into_bytes()));
    }
}

#[async_trait]
impl Handler for Client {
    type Error = russh::Error;

    async fn auth_none(&mut self, _user: &str) -> Result<Auth, Self::Error> {
        Ok(Auth::Accept)
    }
    async fn auth_password(&mut self, _u: &str, _p: &str) -> Result<Auth, Self::Error> {
        Ok(Auth::Accept)
    }
    async fn auth_publickey(&mut self, _u: &str, _k: &PublicKey) -> Result<Auth, Self::Error> {
        Ok(Auth::Accept)
    }

    async fn channel_open_session(
        &mut self,
        _channel: Channel<Msg>,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }

    #[allow(clippy::too_many_arguments)]
    async fn pty_request(
        &mut self,
        _channel: ChannelId,
        _term: &str,
        _cw: u32,
        _rh: u32,
        _pw: u32,
        _ph: u32,
        _modes: &[(Pty, u32)],
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        let motd = self.shell.motd();
        let prompt = self.shell.prompt();
        self.send(session, channel, &motd);
        self.send(session, channel, &prompt);
        Ok(())
    }

    // exec: `git clone` runs `git-upload-pack <path>`; otherwise one-shot shell.
    async fn exec_request(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        let line = String::from_utf8_lossy(data).to_string();
        let line = line.trim();

        // git fetch/clone: bridge to the real `git upload-pack`.
        if let Some(arg) = line
            .strip_prefix("git-upload-pack ")
            .or_else(|| line.strip_prefix("git upload-pack "))
        {
            let path = arg.trim().trim_matches('\'').trim_matches('"');
            let Some(dir) = crate::repos::resolve(path) else {
                let h = session.handle();
                let _ = h
                    .extended_data(
                        channel,
                        1,
                        CryptoVec::from(
                            format!("fatal: repository '{path}' not found\n").into_bytes(),
                        ),
                    )
                    .await;
                let _ = h.exit_status_request(channel, 128).await;
                let _ = h.eof(channel).await;
                let _ = h.close(channel).await;
                return Ok(());
            };
            match Cmd::new("git")
                .arg("upload-pack")
                .arg(&dir)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
            {
                Ok(mut child) => {
                    self.git_stdin = child.stdin.take();
                    let mut out = child.stdout.take().unwrap();
                    let mut err = child.stderr.take().unwrap();
                    let h = session.handle();
                    let ch = channel;
                    let h_out = h.clone();
                    let t_out = tokio::spawn(async move {
                        let mut buf = vec![0u8; 32768];
                        loop {
                            match out.read(&mut buf).await {
                                Ok(0) | Err(_) => break,
                                Ok(n) => {
                                    if h_out
                                        .data(ch, CryptoVec::from(buf[..n].to_vec()))
                                        .await
                                        .is_err()
                                    {
                                        break;
                                    }
                                }
                            }
                        }
                    });
                    let h_err = h.clone();
                    let t_err = tokio::spawn(async move {
                        let mut buf = vec![0u8; 8192];
                        loop {
                            match err.read(&mut buf).await {
                                Ok(0) | Err(_) => break,
                                Ok(n) => {
                                    let _ = h_err
                                        .extended_data(ch, 1, CryptoVec::from(buf[..n].to_vec()))
                                        .await;
                                }
                            }
                        }
                    });
                    tokio::spawn(async move {
                        let _ = t_out.await;
                        let _ = t_err.await;
                        let code =
                            child.wait().await.ok().and_then(|s| s.code()).unwrap_or(0) as u32;
                        let _ = h.exit_status_request(ch, code).await;
                        let _ = h.eof(ch).await;
                        let _ = h.close(ch).await;
                    });
                }
                Err(e) => {
                    let h = session.handle();
                    let _ = h
                        .extended_data(
                            channel,
                            1,
                            CryptoVec::from(format!("fatal: git unavailable ({e})\n").into_bytes()),
                        )
                        .await;
                    let _ = h.exit_status_request(channel, 128).await;
                    let _ = h.eof(channel).await;
                    let _ = h.close(channel).await;
                }
            }
            return Ok(());
        }

        // pushes are refused: this is a read-only mirror.
        if line.contains("receive-pack") {
            let h = session.handle();
            let _ = h
                .extended_data(
                    channel,
                    1,
                    CryptoVec::from("cwd is read-only — push is disabled.\n".as_bytes().to_vec()),
                )
                .await;
            let _ = h.exit_status_request(channel, 1).await;
            let _ = h.eof(channel).await;
            let _ = h.close(channel).await;
            return Ok(());
        }

        // one-shot shell, e.g. `ssh cwd.dev "ls projects"`.
        let o = self.shell.exec(line);
        self.send(session, channel, &o.out);
        session.eof(channel);
        session.close(channel);
        Ok(())
    }

    async fn channel_eof(
        &mut self,
        _channel: ChannelId,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        // client finished sending: close git's stdin so upload-pack can wrap up
        self.git_stdin = None;
        Ok(())
    }

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        // While a git transfer is in flight, raw bytes are git's stdin, not keystrokes.
        if let Some(stdin) = self.git_stdin.as_mut() {
            let _ = stdin.write_all(data).await;
            let _ = stdin.flush().await;
            return Ok(());
        }
        for &b in data {
            match self.esc {
                1 => {
                    self.esc = if b == b'[' || b == b'O' { 2 } else { 0 };
                    continue;
                }
                2 => {
                    if b.is_ascii_digit() {
                        self.param.push(b as char);
                        continue;
                    }
                    match b {
                        b'A' => self.ed.up(),
                        b'B' => self.ed.down(),
                        b'C' => self.ed.right(),
                        b'D' => self.ed.left(),
                        b'H' => self.ed.home(),
                        b'F' => self.ed.end(),
                        b'~' => match self.param.as_str() {
                            "3" => self.ed.delete(),
                            "1" | "7" => self.ed.home(),
                            "4" | "8" => self.ed.end(),
                            _ => {}
                        },
                        _ => {}
                    }
                    self.esc = 0;
                    self.param.clear();
                    self.redraw(session, channel);
                    continue;
                }
                _ => {}
            }

            match b {
                0x1b => self.esc = 1,
                b'\r' | b'\n' => {
                    self.send(session, channel, "\r\n");
                    let line = self.ed.take();
                    let o = self.shell.exec(&line);
                    if !o.out.is_empty() {
                        self.send(session, channel, &o.out);
                    }
                    if o.quit {
                        session.eof(channel);
                        session.close(channel);
                        return Ok(());
                    }
                    let prompt = self.shell.prompt();
                    self.send(session, channel, &prompt);
                }
                0x03 => {
                    // Ctrl-C
                    self.send(session, channel, "^C\r\n");
                    self.ed.kill_line();
                    self.ed.idx = self.ed.hist.len();
                    let prompt = self.shell.prompt();
                    self.send(session, channel, &prompt);
                }
                0x04 => {
                    // Ctrl-D: EOF on empty line, else delete-at-cursor
                    if self.ed.buf.is_empty() {
                        self.send(session, channel, "\r\nbye.\r\n");
                        session.eof(channel);
                        session.close(channel);
                        return Ok(());
                    }
                    self.ed.delete();
                    self.redraw(session, channel);
                }
                0x0c => {
                    // Ctrl-L: clear screen
                    self.send(session, channel, "\x1b[2J\x1b[H");
                    self.redraw(session, channel);
                }
                0x01 => {
                    self.ed.home();
                    self.redraw(session, channel);
                }
                0x05 => {
                    self.ed.end();
                    self.redraw(session, channel);
                }
                0x15 => {
                    self.ed.kill_line();
                    self.redraw(session, channel);
                }
                0x17 => {
                    self.ed.kill_word();
                    self.redraw(session, channel);
                }
                0x7f | 0x08 => {
                    self.ed.backspace();
                    self.redraw(session, channel);
                }
                0x09 => {
                    // Tab completion of the last token.
                    let (cands, start) = self.shell.complete(&self.ed.buf);
                    if cands.is_empty() {
                        self.send(session, channel, "\x07"); // bell
                    } else if cands.len() == 1 {
                        let mut nl = self.ed.buf[..start].to_string();
                        nl.push_str(&cands[0]);
                        self.ed.buf = nl;
                        self.ed.pos = self.ed.buf.len();
                        self.redraw(session, channel);
                    } else {
                        let lcp = common_prefix(&cands);
                        if lcp.len() > self.ed.buf.len() - start {
                            let mut nl = self.ed.buf[..start].to_string();
                            nl.push_str(&lcp);
                            self.ed.buf = nl;
                            self.ed.pos = self.ed.buf.len();
                            self.redraw(session, channel);
                        } else {
                            // ambiguous: list options, then repaint the line
                            self.send(session, channel, &format!("\r\n{}\r\n", cands.join("   ")));
                            self.redraw(session, channel);
                        }
                    }
                }
                0x20..=0x7e => {
                    self.ed.insert(b as char);
                    self.redraw(session, channel);
                }
                _ => {}
            }
        }
        Ok(())
    }
}
