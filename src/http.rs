//! Minimal HTTP/1.1 server (std only). Browsers get the designed site;
//! terminal clients (curl/wget) get the plain-text shell intro: so the
//! website is reachable from the command line too.

use crate::shell::Shell;
use crate::site;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

pub fn serve(addr: &str) {
    let listener = TcpListener::bind(addr).expect("http: bind failed");
    for stream in listener.incoming().flatten() {
        std::thread::spawn(move || {
            let _ = handle(stream);
        });
    }
}

fn handle(mut stream: TcpStream) -> std::io::Result<()> {
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf)?;
    let req = String::from_utf8_lossy(&buf[..n]);
    let mut lines = req.lines();
    let request_line = lines.next().unwrap_or("");
    let path = request_line.split_whitespace().nth(1).unwrap_or("/");

    let (mut ua, mut accept) = (String::new(), String::new());
    for l in lines {
        let ll = l.to_ascii_lowercase();
        if let Some(v) = ll.strip_prefix("user-agent:") {
            ua = v.trim().to_string();
        } else if let Some(v) = ll.strip_prefix("accept:") {
            accept = v.trim().to_string();
        }
    }

    // Static assets: the background script, the local fonts, the favicon.
    if let Some((ctype, bytes)) = site::asset(path) {
        return write_resp(&mut stream, "200 OK", ctype, bytes);
    }

    let is_terminal = ua.contains("curl") || ua.contains("wget") || ua.contains("httpie");
    let wants_html = accept.contains("text/html") && !is_terminal;

    if wants_html {
        write_resp(
            &mut stream,
            "200 OK",
            "text/html; charset=utf-8",
            site::index_html().as_bytes(),
        )
    } else {
        write_resp(
            &mut stream,
            "200 OK",
            "text/plain; charset=utf-8",
            terminal_page().as_bytes(),
        )
    }
}

fn terminal_page() -> String {
    let sh = Shell::new();
    format!(
        "{}\n  you're reading the terminal edition. for the real thing, drop in:\n\n      \x1b[38;2;124;134;255mssh cwd.dev\x1b[0m\n\n",
        sh.motd()
    )
}

fn write_resp(
    stream: &mut TcpStream,
    status: &str,
    ctype: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}
