// Pure-logic functions duplicated from the kernel for host-side unit testing.
// These must stay in sync with their counterparts in kernel/src/.

// ── env::expand (kernel/src/env.rs) ──────────────────────────────────────────

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

// ── edit_distance (kernel/src/shell.rs) ──────────────────────────────────────

fn edit_distance(a: &[char], b: &str) -> usize {
    let bv: Vec<char> = b.chars().collect();
    let (m, n) = (a.len(), bv.len());
    let mut row: Vec<usize> = (0..=n).collect();
    for i in 1..=m {
        let mut prev = row[0];
        row[0] = i;
        for j in 1..=n {
            let old = row[j];
            row[j] = if a[i - 1] == bv[j - 1] {
                prev
            } else {
                1 + prev.min(row[j]).min(row[j - 1])
            };
            prev = old;
        }
    }
    row[n]
}

// ── split_semicolons (kernel/src/shell.rs) ───────────────────────────────────

fn split_semicolons(input: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut in_quote: Option<char> = None;
    let mut start = 0;
    for (i, ch) in input.char_indices() {
        match in_quote {
            Some(q) if ch == q => in_quote = None,
            Some(_)            => {}
            None => match ch {
                '"' | '\'' => in_quote = Some(ch),
                ';' => { out.push(&input[start..i]); start = i + 1; }
                _   => {}
            },
        }
    }
    out.push(&input[start..]);
    out
}

// ── tilde_expand (kernel/src/shell.rs) ───────────────────────────────────────

fn tilde_expand(s: &str) -> String {
    if s.starts_with('~') {
        let mut out = String::from("/");
        out.push_str(&s[1..]);
        out
    } else {
        s.to_string()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&'static str, &'static str)]) -> HashMap<&'static str, &'static str> {
        pairs.iter().cloned().collect()
    }

    // env_expand

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

    // edit_distance

    #[test]
    fn distance_identical() {
        let a: Vec<char> = "reboot".chars().collect();
        assert_eq!(edit_distance(&a, "reboot"), 0);
    }

    #[test]
    fn distance_one_insert() {
        let a: Vec<char> = "rebo".chars().collect();
        assert_eq!(edit_distance(&a, "reboot"), 2);
    }

    #[test]
    fn distance_substitution() {
        let a: Vec<char> = "helo".chars().collect();
        assert_eq!(edit_distance(&a, "help"), 1); // one substitution: o→p
    }

    #[test]
    fn distance_empty_vs_word() {
        let a: Vec<char> = vec![];
        assert_eq!(edit_distance(&a, "ls"), 2);
    }

    // split_semicolons

    #[test]
    fn split_single_cmd() {
        assert_eq!(split_semicolons("ls"), vec!["ls"]);
    }

    #[test]
    fn split_two_cmds() {
        assert_eq!(split_semicolons("ls;pwd"), vec!["ls", "pwd"]);
    }

    #[test]
    fn split_semicolon_in_quotes_ignored() {
        assert_eq!(
            split_semicolons(r#"echo "a;b""#),
            vec![r#"echo "a;b""#]
        );
    }

    #[test]
    fn split_trailing_semicolon() {
        assert_eq!(split_semicolons("ls;"), vec!["ls", ""]);
    }

    // tilde_expand

    #[test]
    fn tilde_at_start_replaced() {
        // '~' → '/', so '~/docs' → '//docs'; normalize() collapses '//' to '/'
        assert_eq!(tilde_expand("~/docs"), "//docs");
    }

    #[test]
    fn tilde_only_becomes_root() {
        assert_eq!(tilde_expand("~"), "/");
    }

    #[test]
    fn tilde_mid_string_unchanged() {
        assert_eq!(tilde_expand("path/~here"), "path/~here");
    }

    #[test]
    fn no_tilde_unchanged() {
        assert_eq!(tilde_expand("/etc/hostname"), "/etc/hostname");
    }
}
