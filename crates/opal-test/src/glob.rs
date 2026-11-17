//! Jest's `testMatch` globs, compiled to regular expressions.
//!
//! Jest matches globs with micromatch against absolute paths. This covers the
//! syntax its defaults and real configs use: `**` across directories, `*` and
//! `?` within one, `[...]` classes, `{a,b}` alternatives, the extglobs `?(...)`,
//! `*(...)`, `+(...)` and `@(...)`, and a leading `!` that negates the whole
//! pattern. `!(...)` inside a pattern has no regular-expression equivalent
//! without lookahead and is refused rather than approximated.

use regex::Regex;

#[derive(Debug)]
pub struct Glob {
    regex: Regex,
    negated: bool,
}

#[derive(Debug, thiserror::Error)]
#[error("unsupported glob `{pattern}`: {reason}")]
pub struct GlobError {
    pub pattern: String,
    pub reason: String,
}

impl Glob {
    pub fn new(pattern: &str) -> Result<Self, GlobError> {
        let (negated, body) = match pattern.strip_prefix('!') {
            Some(rest) if !rest.starts_with('(') => (true, rest),
            _ => (false, pattern),
        };
        let mut out = String::from("^");
        translate(body, &mut out).map_err(|reason| GlobError {
            pattern: pattern.to_string(),
            reason,
        })?;
        out.push('$');
        let regex = Regex::new(&out).map_err(|error| GlobError {
            pattern: pattern.to_string(),
            reason: error.to_string(),
        })?;
        Ok(Glob { regex, negated })
    }

    pub fn is_negated(&self) -> bool {
        self.negated
    }

    /// Whether the pattern, ignoring a leading `!`, matches `path`.
    pub fn matches(&self, path: &str) -> bool {
        self.regex.is_match(path)
    }
}

/// Whether `path` passes a list of globs the way Jest applies `testMatch`: it
/// has to match a positive pattern and no negated one.
pub fn matches_any(globs: &[Glob], path: &str) -> bool {
    let mut matched = false;
    for glob in globs {
        if glob.negated {
            if glob.matches(path) {
                return false;
            }
        } else if glob.matches(path) {
            matched = true;
        }
    }
    matched
}

fn translate(pattern: &str, out: &mut String) -> Result<(), String> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut index = 0;
    translate_until(&chars, &mut index, out, None)
}

/// Translates until `stop` (a closing character for an enclosing group), or
/// to the end.
fn translate_until(
    chars: &[char],
    index: &mut usize,
    out: &mut String,
    stop: Option<char>,
) -> Result<(), String> {
    while *index < chars.len() {
        let c = chars[*index];
        if Some(c) == stop {
            return Ok(());
        }
        if stop == Some(')') && c == '|' {
            out.push('|');
            *index += 1;
            continue;
        }
        if stop == Some('}') && c == ',' {
            out.push('|');
            *index += 1;
            continue;
        }
        let next = chars.get(*index + 1).copied();
        match c {
            '*' if next == Some('*') => {
                // `**` is a whole segment: `a/**/b` matches `a/b` as well.
                let at_start = *index == 0 || chars[*index - 1] == '/';
                let after = chars.get(*index + 2).copied();
                if at_start && after == Some('/') {
                    out.push_str("(?:[^/]*(?:/[^/]*)*/)?");
                    *index += 3;
                } else if at_start && after.is_none() {
                    out.push_str(".*");
                    *index += 2;
                } else {
                    out.push_str("[^/]*");
                    *index += 2;
                }
            }
            '*' | '?' | '+' | '@' | '!' if next == Some('(') => {
                if c == '!' {
                    return Err("`!(...)` needs lookahead".to_string());
                }
                *index += 2;
                let mut inner = String::new();
                translate_until(chars, index, &mut inner, Some(')'))?;
                if chars.get(*index) != Some(&')') {
                    return Err("unclosed `(`".to_string());
                }
                *index += 1;
                let quantifier = match c {
                    '*' => "*",
                    '?' => "?",
                    '+' => "+",
                    _ => "",
                };
                out.push_str(&format!("(?:{inner}){quantifier}"));
            }
            '*' => {
                out.push_str("[^/]*");
                *index += 1;
            }
            '?' => {
                out.push_str("[^/]");
                *index += 1;
            }
            '[' => {
                let close = chars[*index + 1..]
                    .iter()
                    .position(|&ch| ch == ']')
                    .map(|offset| *index + 1 + offset)
                    .ok_or_else(|| "unclosed `[`".to_string())?;
                let mut class: String = chars[*index + 1..close].iter().collect();
                if let Some(rest) = class.strip_prefix('!') {
                    class = format!("^{rest}");
                }
                out.push('[');
                out.push_str(&class.replace('\\', "\\\\"));
                out.push(']');
                *index = close + 1;
            }
            '{' => {
                *index += 1;
                out.push_str("(?:");
                translate_until(chars, index, out, Some('}'))?;
                if chars.get(*index) != Some(&'}') {
                    return Err("unclosed `{`".to_string());
                }
                out.push(')');
                *index += 1;
            }
            '\\' => {
                if let Some(escaped) = next {
                    out.push_str(&regex::escape(&escaped.to_string()));
                }
                *index += 2;
            }
            _ => {
                out.push_str(&regex::escape(&c.to_string()));
                *index += 1;
            }
        }
    }
    if stop.is_some() {
        return Ok(());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const JEST_DEFAULTS: [&str; 2] = [
        "**/__tests__/**/*.?([mc])[jt]s?(x)",
        "**/?(*.)+(spec|test).?([mc])[jt]s?(x)",
    ];

    fn defaults() -> Vec<Glob> {
        JEST_DEFAULTS
            .iter()
            .map(|g| Glob::new(g).unwrap())
            .collect()
    }

    #[test]
    fn test_jest_default_test_match() {
        let globs = defaults();
        for path in [
            "/p/src/__tests__/a.js",
            "/p/__tests__/deep/b.tsx",
            "/p/src/a.test.js",
            "/p/src/a.spec.mts",
            "/p/test.js",
            "/p/src/a.b.test.cjs",
        ] {
            assert!(matches_any(&globs, path), "{path}");
        }
        for path in [
            "/p/src/a.js",
            "/p/src/a.test.json",
            "/p/src/atest.js",
            "/p/__tests__/a.json",
        ] {
            assert!(!matches_any(&globs, path), "{path}");
        }
    }

    #[test]
    fn test_braces_classes_and_negation() {
        let globs = vec![
            Glob::new("**/*.{test,spec}.[jt]s").unwrap(),
            Glob::new("!**/fixtures/**").unwrap(),
        ];
        assert!(matches_any(&globs, "/p/a.test.ts"));
        assert!(matches_any(&globs, "/p/b.spec.js"));
        assert!(!matches_any(&globs, "/p/fixtures/a.test.ts"));
        assert!(!matches_any(&globs, "/p/a.test.tsx"));
    }

    #[test]
    fn test_double_star_matches_zero_directories() {
        let glob = Glob::new("/p/**/x.js").unwrap();
        assert!(glob.matches("/p/x.js"));
        assert!(glob.matches("/p/a/b/x.js"));
        assert!(!glob.matches("/q/x.js"));
    }

    #[test]
    fn test_negative_extglob_is_refused() {
        assert!(Glob::new("**/!(x).js").is_err());
    }
}
