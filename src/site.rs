//! The website: an askama template rendered once, plus the static assets the
//! page pulls in (the background script, the local fonts, the favicon). All of
//! it is embedded, so the whole site ships inside the one binary.

use crate::shell::env_name;
use askama::Template;
use std::sync::OnceLock;

#[derive(Template)]
#[template(path = "index.html")]
struct Index {
    env: String,
    domain: String,
    github_url: String,
    x_url: String,
    email: String,
}

/// The rendered homepage. Rendered once on first use, then handed out as a
/// borrowed string to both the HTTP layer and the in-shell `site/index.html`.
pub fn index_html() -> &'static str {
    static HTML: OnceLock<String> = OnceLock::new();
    HTML.get_or_init(|| {
        Index {
            env: env_name(),
            domain: "cwd.dev".into(),
            github_url: "https://github.com/workingdir".into(),
            x_url: "https://x.com/kierandrewett".into(),
            email: "hello@cwd.dev".into(),
        }
        .render()
        .expect("render index template")
    })
}

pub const BG_JS: &str = include_str!("../assets/bg.js");
pub const FAVICON: &str = include_str!("../assets/favicon.svg");

/// Serve a static asset by request path. Returns its content type and bytes.
pub fn asset(path: &str) -> Option<(&'static str, &'static [u8])> {
    let woff2 = "font/woff2";
    match path {
        "/assets/bg.js" => Some(("text/javascript; charset=utf-8", BG_JS.as_bytes())),
        "/assets/favicon.svg" | "/favicon.svg" | "/favicon.ico" => {
            Some(("image/svg+xml", FAVICON.as_bytes()))
        }
        "/assets/fonts/newsreader-400.woff2" => Some((
            woff2,
            include_bytes!("../assets/fonts/newsreader-400.woff2"),
        )),
        "/assets/fonts/newsreader-500.woff2" => Some((
            woff2,
            include_bytes!("../assets/fonts/newsreader-500.woff2"),
        )),
        "/assets/fonts/newsreader-italic-400.woff2" => Some((
            woff2,
            include_bytes!("../assets/fonts/newsreader-italic-400.woff2"),
        )),
        "/assets/fonts/spacemono-400.woff2" => {
            Some((woff2, include_bytes!("../assets/fonts/spacemono-400.woff2")))
        }
        "/assets/fonts/spacemono-700.woff2" => {
            Some((woff2, include_bytes!("../assets/fonts/spacemono-700.woff2")))
        }
        _ => None,
    }
}
