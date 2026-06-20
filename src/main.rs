//! cwd.dev: one binary that is the whole site.
//!   serve : run the website (HTTP) and, with `--features ssh`, the shell (SSH)
//!   web   : website only
//!   local : the faux shell over stdin/stdout (for development)

mod http;
#[cfg(feature = "ssh")]
mod repos;
mod shell;
#[cfg(feature = "ssh")]
mod ssh;
mod theme;
mod vfs;

use std::io::Write;

fn http_addr() -> String {
    std::env::var("CWD_HTTP").unwrap_or_else(|_| "0.0.0.0:4280".to_string())
}
#[cfg(feature = "ssh")]
fn ssh_addr() -> String {
    std::env::var("CWD_SSH").unwrap_or_else(|_| "0.0.0.0:4242".to_string())
}

fn main() {
    let mode = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "serve".to_string());
    match mode.as_str() {
        "local" => local(),
        "web" => {
            eprintln!("http → http://{}", http_addr());
            http::serve(&http_addr());
        }
        "serve" => serve(),
        other => eprintln!("usage: cwd [serve|web|local]  (got `{other}`)"),
    }
}

/// The faux shell over stdio: same engine the SSH server runs.
fn local() {
    use std::io::{self, BufRead};
    let mut sh = shell::Shell::new();
    print!("{}", sh.motd());
    let stdin = io::stdin();
    loop {
        print!("{}", sh.prompt());
        io::stdout().flush().ok();
        let mut line = String::new();
        if stdin.lock().read_line(&mut line).unwrap_or(0) == 0 {
            println!();
            break;
        }
        let o = sh.exec(&line);
        print!("{}", o.out);
        io::stdout().flush().ok();
        if o.quit {
            break;
        }
    }
}

#[cfg(feature = "ssh")]
fn serve() {
    let h = http_addr();
    std::thread::spawn(move || http::serve(&h));
    eprintln!("http → http://{}", http_addr());
    eprintln!("ssh  → {}", ssh_addr());
    ssh::serve(&ssh_addr());
}

#[cfg(not(feature = "ssh"))]
fn serve() {
    eprintln!("http → http://{}", http_addr());
    eprintln!("(ssh transport disabled — rebuild with `--features ssh`)");
    http::serve(&http_addr());
}
