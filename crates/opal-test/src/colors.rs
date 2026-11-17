//! Whether matcher messages are colored, decided as Jest decides it.
//!
//! jest-worker gives each worker `FORCE_COLOR=1` when supports-color says the
//! parent's stdout supports color, and no `FORCE_COLOR` at all when it
//! doesn't. chalk inside the worker then colors matcher messages and diffs,
//! or not. A test that snapshots a message gets the codes only in the first
//! case. jest-extended's own suite, for one, runs with `--color=true` for
//! that reason. This is supports-color 7's decision for stdout, with
//! `--color` and `--no-color` standing for the flags it reads from argv.

use std::io::IsTerminal;

use regex::Regex;

/// What `FORCE_COLOR` and the flags force, if anything. `Garbage` is a
/// `FORCE_COLOR` that `parseInt` makes NaN of: forced, with no minimum.
#[derive(Clone, Copy, PartialEq)]
enum Force {
    Unset,
    Level(u32),
    Garbage,
}

pub fn stdout_supports_color(flag: Option<bool>) -> bool {
    let env = |name: &str| std::env::var(name).ok();
    supports_color(flag, &env, std::io::stdout().is_terminal())
}

fn supports_color(flag: Option<bool>, env: &dyn Fn(&str) -> Option<String>, is_tty: bool) -> bool {
    let mut force = match flag {
        Some(false) => Force::Level(0),
        Some(true) => Force::Level(1),
        None => Force::Unset,
    };
    if let Some(value) = env("FORCE_COLOR") {
        force = match value.as_str() {
            "true" | "" => Force::Level(1),
            "false" => Force::Level(0),
            other => {
                // parseInt: leading digits, the rest ignored.
                let digits: String = other
                    .trim_start()
                    .chars()
                    .take_while(char::is_ascii_digit)
                    .collect();
                digits
                    .parse::<u32>()
                    .map(|n| Force::Level(n.min(3)))
                    .unwrap_or(Force::Garbage)
            }
        };
    }
    if force == Force::Level(0) {
        return false;
    }
    if !is_tty && force == Force::Unset {
        return false;
    }
    let forced = matches!(force, Force::Level(n) if n > 0);
    let term = env("TERM").unwrap_or_default();
    if term == "dumb" {
        return forced;
    }
    if cfg!(windows) {
        return true;
    }
    if env("CI").is_some() {
        let known = [
            "TRAVIS",
            "CIRCLECI",
            "APPVEYOR",
            "GITLAB_CI",
            "GITHUB_ACTIONS",
            "BUILDKITE",
        ]
        .iter()
        .any(|sign| env(sign).is_some())
            || env("CI_NAME").as_deref() == Some("codeship");
        return known || forced;
    }
    if let Some(version) = env("TEAMCITY_VERSION") {
        let modern = Regex::new(r"^(9\.(0*[1-9]\d*)\.|\d{2,}\.)").expect("valid");
        return modern.is_match(&version);
    }
    if env("COLORTERM").as_deref() == Some("truecolor") {
        return true;
    }
    if let Some(program) = env("TERM_PROGRAM")
        && (program == "iTerm.app" || program == "Apple_Terminal")
    {
        return true;
    }
    let wide = Regex::new(r"(?i)-256(color)?$").expect("valid");
    let basic = Regex::new(r"(?i)^screen|^xterm|^vt100|^vt220|^rxvt|color|ansi|cygwin|linux")
        .expect("valid");
    if wide.is_match(&term) || basic.is_match(&term) {
        return true;
    }
    if env("COLORTERM").is_some() {
        return true;
    }
    forced
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decide(flag: Option<bool>, vars: &[(&str, &str)], is_tty: bool) -> bool {
        let env = |name: &str| {
            vars.iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| v.to_string())
        };
        supports_color(flag, &env, is_tty)
    }

    #[test]
    fn test_a_pipe_has_no_color_unless_forced() {
        assert!(!decide(None, &[("TERM", "xterm-256color")], false));
        assert!(decide(Some(true), &[], false));
        assert!(decide(None, &[("FORCE_COLOR", "1")], false));
        assert!(!decide(Some(true), &[("FORCE_COLOR", "0")], false));
    }

    #[test]
    fn test_a_terminal_has_color_by_its_term() {
        assert!(decide(None, &[("TERM", "xterm-256color")], true));
        assert!(!decide(None, &[("TERM", "dumb")], true));
        assert!(!decide(None, &[], true));
        assert!(!decide(Some(false), &[("TERM", "xterm")], true));
    }

    #[test]
    fn test_ci_counts_only_the_known_ones() {
        assert!(decide(None, &[("CI", "1"), ("GITHUB_ACTIONS", "1")], true));
        assert!(!decide(None, &[("CI", "1"), ("TERM", "xterm")], true));
    }
}
