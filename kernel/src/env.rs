use spin::Mutex;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

static ENV: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());

pub fn init() {
    let mut e = ENV.lock();
    e.insert("HOME".into(),  "/".into());
    e.insert("SHELL".into(), "sh".into());
    e.insert("PWD".into(),   "/".into());
}

pub fn get(key: &str) -> Option<String> {
    ENV.lock().get(key).cloned()
}

pub fn set(key: &str, val: &str) {
    ENV.lock().insert(key.into(), val.into());
}

pub fn remove(key: &str) {
    ENV.lock().remove(key);
}

pub fn list() -> Vec<(String, String)> {
    ENV.lock().iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

/// Expand `$VAR` and `${VAR}` in `s`. Unknown variables expand to `""`.
pub fn expand(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '$' {
            out.push(ch);
            continue;
        }
        match chars.peek() {
            Some(&'{') => {
                chars.next(); // consume '{'
                let key: String = chars.by_ref().take_while(|&c| c != '}').collect();
                if let Some(val) = get(&key) { out.push_str(&val); }
            }
            Some(&c) if c.is_ascii_alphanumeric() || c == '_' => {
                let _ = c; // suppress unused warning; presence confirmed by peek
                let mut key = String::new();
                while matches!(chars.peek(), Some(c) if c.is_ascii_alphanumeric() || *c == '_') {
                    key.push(chars.next().unwrap());
                }
                if let Some(val) = get(&key) { out.push_str(&val); }
            }
            _ => out.push('$'),
        }
    }
    out
}
