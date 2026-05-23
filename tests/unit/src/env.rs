// Mirrors kernel/src/env.rs — keep in sync.

use std::collections::HashMap;

fn env_expand(s: &str, env: &HashMap<&str, &str>) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '$' {
            out.push(ch);
            continue;
        }
        match chars.peek() {
            Some(&'{') => {
                chars.next();
                let key: String = chars.by_ref().take_while(|&c| c != '}').collect();
                if let Some(val) = env.get(key.as_str()) { out.push_str(val); }
            }
            Some(&c) if c.is_ascii_alphanumeric() || c == '_' => {
                let _ = c;
                let mut key = String::new();
                while matches!(chars.peek(), Some(c) if c.is_ascii_alphanumeric() || *c == '_') {
                    key.push(chars.next().unwrap());
                }
                if let Some(val) = env.get(key.as_str()) { out.push_str(val); }
            }
            _ => out.push('$'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&'static str, &'static str)]) -> HashMap<&'static str, &'static str> {
        pairs.iter().cloned().collect()
    }

    #[test]
    fn expand_plain_var() {
        let e = env(&[("FOO", "bar")]);
        assert_eq!(env_expand("$FOO", &e), "bar");
    }

    #[test]
    fn expand_braced_var() {
        let e = env(&[("HOME", "/home/user")]);
        assert_eq!(env_expand("${HOME}/docs", &e), "/home/user/docs");
    }

    #[test]
    fn expand_unknown_var_is_empty() {
        let e = env(&[]);
        assert_eq!(env_expand("$NOPE", &e), "");
    }

    #[test]
    fn expand_bare_dollar_preserved() {
        let e = env(&[]);
        assert_eq!(env_expand("$", &e), "$");
        assert_eq!(env_expand("$ ", &e), "$ ");
    }

    #[test]
    fn expand_multiple_vars() {
        let e = env(&[("A", "hello"), ("B", "world")]);
        assert_eq!(env_expand("$A $B", &e), "hello world");
    }

    #[test]
    fn expand_var_adjacent_text() {
        let e = env(&[("EXT", "rs")]);
        assert_eq!(env_expand("file.${EXT}", &e), "file.rs");
    }

    #[test]
    fn expand_no_dollars() {
        let e = env(&[]);
        assert_eq!(env_expand("plain text", &e), "plain text");
    }
}
