/// Normalize user-typed target names by trimming and removing trailing `/` and `/.git`.
#[must_use]
pub fn normalize_target(raw: &str) -> String {
    let mut s = raw.trim().to_owned();
    if s.is_empty() {
        return s;
    }

    loop {
        let previous = s.clone();
        while s.ends_with('/') {
            s.pop();
        }
        if s.ends_with("/.git") {
            let new_len = s.len() - "/.git".len();
            s.truncate(new_len);
        }
        if s == previous {
            break;
        }
    }

    s
}
