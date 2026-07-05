fn strip_numeric_prefix(value: &str) -> &str {
    let Some((prefix, rest)) = value.split_once('-') else {
        return value;
    };
    if !prefix.is_empty() && prefix.bytes().all(|byte| byte.is_ascii_digit()) {
        rest
    } else {
        value
    }
}

fn strip_oracle_suffix(value: &str) -> &str {
    value.strip_suffix("-oracle").unwrap_or(value)
}

fn path_basename(path: &str) -> Option<&str> {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|part| !part.is_empty())
}

fn empty_to_none(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn unique_preserve_order(values: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for value in values {
        if !out.iter().any(|existing| existing == &value) {
            out.push(value);
        }
    }
    out
}

#[cfg(test)]
mod coverage_gap_tests {
include!("../coverage_gap_tests_impl/helper_live_state_edges.rs");
}
