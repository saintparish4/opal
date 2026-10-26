//! Glob matching for `tsconfig.json` `include` and `package.json`
//! `sideEffects`: `*` within a segment, `**` across segments, `?` one
//! character, and `{a,b}` alternatives.

/// Whether `text` (a `/`-separated relative path) matches `pattern`.
pub fn matches(pattern: &str, text: &str) -> bool {
    expand_braces(pattern)
        .iter()
        .any(|pattern| matches_plain(pattern, text))
}

fn expand_braces(pattern: &str) -> Vec<String> {
    let Some(open) = pattern.find('{') else {
        return vec![pattern.to_string()];
    };
    let Some(close) = pattern[open..].find('}').map(|i| open + i) else {
        return vec![pattern.to_string()];
    };
    let (head, rest) = (&pattern[..open], &pattern[close + 1..]);
    pattern[open + 1..close]
        .split(',')
        .flat_map(|alternative| expand_braces(&format!("{head}{alternative}{rest}")))
        .collect()
}

fn matches_plain(pattern: &str, text: &str) -> bool {
    fn segments(pattern: &[&str], text: &[&str]) -> bool {
        match (pattern.first(), text.first()) {
            (None, None) => true,
            (Some(&"**"), _) => {
                segments(&pattern[1..], text) || (!text.is_empty() && segments(pattern, &text[1..]))
            }
            (Some(p), Some(t)) => {
                segment(p.as_bytes(), t.as_bytes()) && segments(&pattern[1..], &text[1..])
            }
            _ => false,
        }
    }
    fn segment(pattern: &[u8], text: &[u8]) -> bool {
        match (pattern.first(), text.first()) {
            (None, None) => true,
            (Some(b'*'), _) => {
                segment(&pattern[1..], text) || (!text.is_empty() && segment(pattern, &text[1..]))
            }
            (Some(b'?'), Some(_)) => segment(&pattern[1..], &text[1..]),
            (Some(p), Some(t)) if p == t => segment(&pattern[1..], &text[1..]),
            _ => false,
        }
    }
    let pattern: Vec<&str> = pattern.split('/').collect();
    let text: Vec<&str> = text.split('/').collect();
    segments(&pattern, &text)
}

#[cfg(test)]
mod tests {
    use super::matches;

    #[test]
    fn test_globs() {
        assert!(matches("**/*.css", "a/b/c.css"));
        assert!(matches("**/*.css", "c.css"));
        assert!(matches("src/*.{js,mjs}", "src/a.mjs"));
        assert!(!matches("src/*.js", "src/deep/a.js"));
        assert!(matches("src/**/polyfill?.js", "src/x/y/polyfill1.js"));
    }
}
