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

// ── split_commands (kernel/src/shell.rs) ─────────────────────────────────────

fn split_commands(input: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut in_quote: Option<char> = None;
    let mut start = 0;
    let chars: Vec<(usize, char)> = input.char_indices().collect();
    let mut i = 0;
    while i < chars.len() {
        let (byte_pos, ch) = chars[i];
        match in_quote {
            Some(q) if ch == q => { in_quote = None; i += 1; }
            Some(_)            => { i += 1; }
            None => match ch {
                '"' | '\'' => { in_quote = Some(ch); i += 1; }
                ';' => {
                    out.push(&input[start..byte_pos]);
                    start = byte_pos + 1;
                    i += 1;
                }
                '&' if i + 1 < chars.len() && chars[i + 1].1 == '&' => {
                    out.push(&input[start..byte_pos]);
                    start = chars[i + 1].0 + 1;
                    i += 2;
                }
                _ => { i += 1; }
            }
        }
    }
    out.push(&input[start..]);
    out
}

// ── tokenize_quoted + ParsedArgs + parse_args (kernel/src/shell.rs) ──────────

fn tokenize_quoted(input: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_quote: Option<char> = None;
    for ch in input.chars() {
        match in_quote {
            Some(q) if ch == q => in_quote = None,
            Some(_)            => current.push(ch),
            None => match ch {
                '"' | '\'' => in_quote = Some(ch),
                c if c.is_whitespace() => {
                    if !current.is_empty() {
                        tokens.push(std::mem::take(&mut current));
                    }
                }
                c => current.push(c),
            },
        }
    }
    if !current.is_empty() { tokens.push(current); }
    tokens
}

struct ParsedArgs {
    flags:      Vec<char>,
    flag_vals:  Vec<(char, String)>,
    positional: Vec<String>,
}

impl ParsedArgs {
    fn has_flag(&self, c: char) -> bool {
        self.flags.contains(&c) || self.flag_vals.iter().any(|(f, _)| *f == c)
    }
    fn flag_val(&self, c: char) -> Option<&str> {
        self.flag_vals.iter().find(|(f, _)| *f == c).map(|(_, v)| v.as_str())
    }
    fn get(&self, i: usize) -> Option<&str> {
        self.positional.get(i).map(|s| s.as_str())
    }
}

fn parse_args(input: &str) -> ParsedArgs {
    let mut flags:      Vec<char>           = Vec::new();
    let mut flag_vals:  Vec<(char, String)> = Vec::new();
    let mut positional: Vec<String>         = Vec::new();

    let tokens: Vec<String> = tokenize_quoted(input);
    let mut i = 0;
    let mut stop = false;

    while i < tokens.len() {
        let tok = tokens[i].as_str();
        if stop || !tok.starts_with('-') || tok == "-" {
            positional.push(tilde_expand(tok));
            i += 1;
            continue;
        }
        if tok == "--" {
            stop = true;
            i += 1;
            continue;
        }

        let chars: Vec<char> = tok[1..].chars().collect();
        let mut j = 0;
        while j < chars.len() {
            let flag = chars[j];
            j += 1;
            if j < chars.len() {
                if chars[j].is_ascii_digit() {
                    let val: String = chars[j..].iter().collect();
                    flag_vals.push((flag, val));
                    break;
                }
                flags.push(flag);
            } else {
                let next_is_num = tokens.get(i + 1)
                    .map(|t| t.starts_with(|c: char| c.is_ascii_digit()))
                    .unwrap_or(false);
                if next_is_num {
                    flag_vals.push((flag, tokens[i + 1].clone()));
                    i += 1;
                } else {
                    flags.push(flag);
                }
            }
        }
        i += 1;
    }

    ParsedArgs { flags, flag_vals, positional }
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

    // split_commands

    #[test]
    fn split_single_cmd() {
        assert_eq!(split_commands("ls"), vec!["ls"]);
    }

    #[test]
    fn split_two_cmds_semicolon() {
        assert_eq!(split_commands("ls;pwd"), vec!["ls", "pwd"]);
    }

    #[test]
    fn split_semicolon_in_quotes_ignored() {
        assert_eq!(
            split_commands(r#"echo "a;b""#),
            vec![r#"echo "a;b""#]
        );
    }

    #[test]
    fn split_trailing_semicolon() {
        assert_eq!(split_commands("ls;"), vec!["ls", ""]);
    }

    #[test]
    fn split_two_cmds_and_and() {
        assert_eq!(split_commands("ls&&pwd"), vec!["ls", "pwd"]);
    }

    #[test]
    fn split_and_and_with_spaces() {
        assert_eq!(split_commands("cd /sys && pwd"), vec!["cd /sys ", " pwd"]);
    }

    #[test]
    fn split_and_and_in_quotes_ignored() {
        assert_eq!(
            split_commands(r#"echo "a&&b""#),
            vec![r#"echo "a&&b""#]
        );
    }

    #[test]
    fn split_single_ampersand_not_split() {
        assert_eq!(split_commands("echo a&b"), vec!["echo a&b"]);
    }

    #[test]
    fn split_mixed_separators() {
        assert_eq!(
            split_commands("echo a ; echo b && echo c"),
            vec!["echo a ", " echo b ", " echo c"]
        );
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

    // parse_args

    #[test]
    fn parse_single_bool_flag() {
        let p = parse_args("-l");
        assert!(p.has_flag('l'));
        assert!(p.flag_vals.is_empty());
        assert!(p.positional.is_empty());
    }

    #[test]
    fn parse_bool_flag_with_positional() {
        // wc -l /path  →  flag 'l', positional[0] = "/path"
        let p = parse_args("-l /etc/version");
        assert!(p.has_flag('l'));
        assert!(p.flag_vals.is_empty());
        assert_eq!(p.get(0), Some("/etc/version"));
    }

    #[test]
    fn parse_flag_val_space_separated() {
        // head -n 3 /path  →  flag_val('n') = "3", positional[0] = "/path"
        let p = parse_args("-n 3 /sys/welcome");
        assert_eq!(p.flag_val('n'), Some("3"));
        assert!(p.has_flag('n'));  // has_flag returns true for value-flags too
        assert_eq!(p.get(0), Some("/sys/welcome"));
    }

    #[test]
    fn parse_flag_val_inline_digit() {
        // -n3 (no space)  →  same result as -n 3
        let p = parse_args("-n3 /sys/welcome");
        assert_eq!(p.flag_val('n'), Some("3"));
        assert_eq!(p.get(0), Some("/sys/welcome"));
    }

    #[test]
    fn parse_combined_bool_flags() {
        // grep -ic pattern /path  →  both 'i' and 'c' set
        let p = parse_args("-ic pattern /path");
        assert!(p.has_flag('i'));
        assert!(p.has_flag('c'));
        assert_eq!(p.get(0), Some("pattern"));
        assert_eq!(p.get(1), Some("/path"));
    }

    #[test]
    fn parse_grep_style() {
        // grep -i amaterasu /sys/welcome
        let p = parse_args("-i amaterasu /sys/welcome");
        assert!(p.has_flag('i'));
        assert_eq!(p.get(0), Some("amaterasu"));
        assert_eq!(p.get(1), Some("/sys/welcome"));
    }

    #[test]
    fn parse_no_flags_all_positional() {
        // wc with no flags  →  flags/flag_vals empty ("all" mode)
        let p = parse_args("/sys/welcome");
        assert!(p.flags.is_empty());
        assert!(p.flag_vals.is_empty());
        assert_eq!(p.get(0), Some("/sys/welcome"));
    }

    #[test]
    fn parse_end_of_options_marker() {
        // -- stops flag parsing; -notaflag treated as positional
        let p = parse_args("-- -notaflag");
        assert!(p.flags.is_empty());
        assert_eq!(p.get(0), Some("-notaflag"));
    }

    #[test]
    fn parse_empty_input() {
        let p = parse_args("");
        assert!(p.flags.is_empty());
        assert!(p.flag_vals.is_empty());
        assert!(p.positional.is_empty());
    }

    #[test]
    fn parse_quoted_path_with_spaces() {
        let p = parse_args(r#"-l "my file""#);
        assert!(p.has_flag('l'));
        assert_eq!(p.get(0), Some("my file"));
    }

    // tokenize_quoted

    #[test]
    fn tokenize_simple() {
        assert_eq!(tokenize_quoted("-n 3 /path"), vec!["-n", "3", "/path"]);
    }

    #[test]
    fn tokenize_quoted_span_preserves_spaces() {
        assert_eq!(tokenize_quoted(r#""hello world""#), vec!["hello world"]);
    }

    #[test]
    fn tokenize_single_quotes() {
        assert_eq!(tokenize_quoted("'a b' c"), vec!["a b", "c"]);
    }

    #[test]
    fn tokenize_empty() {
        assert!(tokenize_quoted("").is_empty());
    }
}
