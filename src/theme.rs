//! ANSI styling for the faux shell: the cwd palette, tuned for dark terminals.

pub const RESET: &str = "\x1b[0m";
pub const BOLD: &str = "\x1b[1m";

// Electric blue, lifted for legibility on a dark terminal (the brand's dark-mode accent).
pub const BLUE: &str = "\x1b[38;2;124;134;255m";
// Muted warm grey.
pub const MUTED: &str = "\x1b[38;2;142;135;122m";

pub const CLEAR: &str = "\x1b[2J\x1b[H";

pub fn blue(s: &str) -> String {
    format!("{BLUE}{s}{RESET}")
}
pub fn muted(s: &str) -> String {
    format!("{MUTED}{s}{RESET}")
}
