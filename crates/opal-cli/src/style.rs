//! Colour for the lines a command prints once it is done.
//!
//! Decoration only: every line reads the same without it, which is what a
//! pipe, a CI log, and `NO_COLOR` get. Stdout and stderr are decided
//! separately, because `opal install > result.txt` leaves stderr on a terminal
//! and must still put no escape codes in the file.
//!
//! These are the eight-colour codes every colour terminal has, so unlike the
//! progress bar's gradient there is no depth to detect. They also follow the
//! terminal's own theme, so green is whatever the user's green is.

use std::fmt::Display;
use std::io::IsTerminal;

const RESET: &str = "\x1b[0m";

#[derive(Clone, Copy, Debug)]
pub struct Paint {
    on: bool,
}

impl Paint {
    pub fn for_stdout() -> Self {
        Self::from_env(std::io::stdout().is_terminal())
    }

    pub fn for_stderr() -> Self {
        Self::from_env(std::io::stderr().is_terminal())
    }

    fn from_env(is_terminal: bool) -> Self {
        Self {
            on: Self::decide(
                is_terminal,
                std::env::var("NO_COLOR").ok().as_deref(),
                std::env::var("TERM").ok().as_deref(),
            ),
        }
    }

    /// `NO_COLOR` counts when set to anything non-empty (no-color.org), the
    /// same reading the progress bar gives it.
    fn decide(is_terminal: bool, no_color: Option<&str>, term: Option<&str>) -> bool {
        is_terminal && !no_color.is_some_and(|value| !value.is_empty()) && term != Some("dumb")
    }

    pub fn is_on(self) -> bool {
        self.on
    }

    #[cfg(test)]
    pub const fn plain() -> Self {
        Self { on: false }
    }

    #[cfg(test)]
    pub const fn coloured() -> Self {
        Self { on: true }
    }

    fn wrap(self, code: &str, text: impl Display) -> String {
        if self.on {
            format!("\x1b[{code}m{text}{RESET}")
        } else {
            text.to_string()
        }
    }

    pub fn bold(self, text: impl Display) -> String {
        self.wrap("1", text)
    }

    pub fn dim(self, text: impl Display) -> String {
        self.wrap("2", text)
    }

    pub fn red(self, text: impl Display) -> String {
        self.wrap("31", text)
    }

    pub fn green(self, text: impl Display) -> String {
        self.wrap("32", text)
    }

    pub fn yellow(self, text: impl Display) -> String {
        self.wrap("33", text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_colour_needs_a_terminal() {
        assert!(Paint::decide(true, None, Some("xterm-256color")));
        assert!(!Paint::decide(false, None, Some("xterm-256color")));
    }

    #[test]
    fn test_no_color_and_a_dumb_terminal_turn_colour_off() {
        assert!(!Paint::decide(true, Some("1"), Some("xterm")));
        assert!(!Paint::decide(true, None, Some("dumb")));
        assert!(Paint::decide(true, Some(""), Some("xterm")));
    }

    #[test]
    fn test_plain_text_carries_no_escape_codes() {
        assert_eq!(Paint::plain().green("364"), "364");
        assert_eq!(Paint::coloured().green("364"), "\x1b[32m364\x1b[0m");
    }
}
