//! The website: an askama template rendered once, plus the static assets the
//! page pulls in (the background script, the local fonts, the favicon). All of
//! it is embedded, so the whole site ships inside the one binary.

use crate::shell::env_name;
use askama::Template;
use pulldown_cmark::{html, Options, Parser};
use std::sync::OnceLock;

#[derive(Template)]
#[template(path = "index.html")]
struct Index {
    env: String,
    body: String, // content/index.md rendered to HTML
}

/// The homepage prose, edited as markdown.
pub const INDEX_MD: &str = include_str!("../content/index.md");

/// Markdown to HTML. External links open in a new tab; in-page anchors and
/// mailto: links are left alone.
fn render_markdown(md: &str) -> String {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(md, opts);
    let mut out = String::new();
    html::push_html(&mut out, parser);
    out.replace(
        "<a href=\"http",
        "<a target=\"_blank\" rel=\"noreferrer\" href=\"http",
    )
}

/// The rendered homepage. Rendered once on first use, then handed out as a
/// borrowed string to both the HTTP layer and the in-shell `site/index.html`.
pub fn index_html() -> &'static str {
    static HTML: OnceLock<String> = OnceLock::new();
    HTML.get_or_init(|| {
        Index {
            env: env_name(),
            body: render_markdown(INDEX_MD),
        }
        .render()
        .expect("render index template")
    })
}

pub const STYLE_CSS: &str = include_str!("../assets/style.css");
pub const BG_JS: &str = include_str!("../assets/bg.js");
pub const FAVICON: &str = include_str!("../assets/favicon.svg");

/// Serve a static asset by request path. Returns its content type and bytes.
pub fn asset(path: &str) -> Option<(&'static str, &'static [u8])> {
    let woff2 = "font/woff2";
    match path {
        "/assets/style.css" => Some(("text/css; charset=utf-8", STYLE_CSS.as_bytes())),
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
