// mawx spec parser — WI-4 of ψ/design/mawx-spec.md (§2.1 grammar table).
//
// Pure and deterministic: no filesystem or network access. Two consequences,
// both deliberate (spec §2.1 disambiguation rules):
// - Scheme prefixes are checked BEFORE the local-path test, so an existing
//   directory named like a scheme'd spec (e.g. `gh:acme/tools`) can never
//   shadow the scheme.
// - The local route requires an explicit prefix (`file:`, `./`, `../`, `/`,
//   `~/`); a bare token is always a verb or GitHub shorthand, never a cwd
//   probe (unlike `plugin_plan.rs`, which consults `path.exists()`).
//
// This module is standalone by design: wiring into `maw x` and
// `maw plugin install` happens in WI-8. Nothing here touches the existing
// install parsing in `plugin_plan.rs`; the few helpers it needs are mirrored
// with `// mirrors plugin_plan.rs::…` markers so WI-8 can unify them.

/// A fully parsed `maw x` / `maw plugin install` spec: the source plus the
/// optional inline `#sha256:` pin (spec §2.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XSpec {
    /// Where the plugin comes from (one variant per grammar row).
    pub source: XSource,
    /// Inline `#sha256:<64hex>` pin, normalized to `sha256:<hex>`
    /// (equivalent to `--sha256`; mismatch is fatal, `--force`-proof).
    pub sha256: Option<String>,
}

/// One variant per row of the spec §2.1 grammar table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XSource {
    /// `costs` / `costs@main` — bare verb, resolved via the registry
    /// resolution chain (spec §2.2).
    Verb {
        verb: String,
        reference: Option<XGitRef>,
    },
    /// `owner/repo[/sub][@ref]`, `owner/repo@<40hex>[/sub]`, and the
    /// canonical scheme form `gh:owner/repo[@ref][/sub]`.
    GitHub {
        owner: String,
        repo: String,
        subpath: Option<String>,
        reference: Option<XGitRef>,
    },
    /// Explicit git URL: `https://…/repo.git[@ref][#sub]` (also `git@…`).
    GitUrl {
        url: String,
        reference: Option<XGitRef>,
        subpath: Option<String>,
    },
    /// Local dev route: `file:./dir` | `./dir` | `../dir` | `/abs` | `~/dir`
    /// (no cache write).
    LocalDir { path: String },
    /// `gh-release:owner/repo@tag#asset` — parse-only in V1 (fetch lands in
    /// v1.1, spec §2.3).
    GhRelease {
        owner: String,
        repo: String,
        tag: String,
        asset: String,
    },
    /// `npm:@scope/pkg[@ver]` — parse-only in V1 (prebuilt-pinned-wasm
    /// bridge lands in v2, spec §2.3; never executes JS).
    Npm {
        package: String,
        version: Option<String>,
    },
}

/// A git `@ref`: a 40-lowercase-hex ref is a commit pin (immutable via raw
/// fetch), everything else is a branch/tag name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XGitRef {
    /// Branch or tag name (mutable — resolution must pin it later).
    Named(String),
    /// 40-lowercase-hex commit SHA — an immutable pin.
    Commit(String),
}

/// Which mawx milestone can *execute* a parsed source (everything parses in
/// V1; `gh-release:` and `npm:` are parse-only until their fetch routes land).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XSpecTier {
    V1,
    V1Dot1,
    V2,
}

impl XSpec {
    /// Render the canonical source string — the form the trust store and
    /// `plugins.lock` record. GitHub sources render in the `gh:` scheme
    /// (spec §2.1: `gh:` is the canonical form). Round-trip invariant:
    /// `parse_x_spec(spec.canonical()) == Ok(spec)`.
    #[must_use]
    pub fn canonical(&self) -> String {
        match &self.sha256 {
            Some(pin) => format!("{}#{pin}", self.source.canonical()),
            None => self.source.canonical(),
        }
    }

    /// Milestone gate for the parsed source (see [`XSpecTier`]).
    #[must_use]
    pub fn tier(&self) -> XSpecTier {
        self.source.tier()
    }
}

impl XSource {
    /// Canonical rendering of the source without any `#sha256:` pin.
    #[must_use]
    pub fn canonical(&self) -> String {
        match self {
            XSource::Verb { verb, reference } => match reference {
                Some(reference) => format!("{verb}@{}", reference.as_str()),
                None => verb.clone(),
            },
            XSource::GitHub { owner, repo, subpath, reference } => {
                let mut out = format!("gh:{owner}/{repo}");
                if let Some(reference) = reference {
                    out.push('@');
                    out.push_str(reference.as_str());
                }
                if let Some(subpath) = subpath {
                    out.push('/');
                    out.push_str(subpath);
                }
                out
            }
            XSource::GitUrl { url, reference, subpath } => {
                let mut out = url.clone();
                if let Some(reference) = reference {
                    out.push('@');
                    out.push_str(reference.as_str());
                }
                if let Some(subpath) = subpath {
                    out.push('#');
                    out.push_str(subpath);
                }
                out
            }
            XSource::LocalDir { path } => format!("file:{path}"),
            XSource::GhRelease { owner, repo, tag, asset } => {
                format!("gh-release:{owner}/{repo}@{tag}#{asset}")
            }
            XSource::Npm { package, version } => match version {
                Some(version) => format!("npm:{package}@{version}"),
                None => format!("npm:{package}"),
            },
        }
    }

    /// Milestone gate: `gh-release:` executes from v1.1, `npm:` from v2,
    /// everything else in V1.
    #[must_use]
    pub fn tier(&self) -> XSpecTier {
        match self {
            XSource::GhRelease { .. } => XSpecTier::V1Dot1,
            XSource::Npm { .. } => XSpecTier::V2,
            XSource::Verb { .. }
            | XSource::GitHub { .. }
            | XSource::GitUrl { .. }
            | XSource::LocalDir { .. } => XSpecTier::V1,
        }
    }
}

impl XGitRef {
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            XGitRef::Named(reference) | XGitRef::Commit(reference) => reference,
        }
    }
}

/// Parse one spec string from the §2.1 grammar table into an [`XSpec`].
///
/// # Errors
///
/// Returns a user-facing message when the spec is empty, contains
/// whitespace, carries a malformed `#sha256:` pin (must be 64 lowercase hex
/// chars), has an empty `@ref`, an empty or traversing path segment, or a
/// scheme'd form that does not match its grammar row.
pub fn parse_x_spec(spec: &str) -> Result<XSpec, String> {
    if spec.is_empty() {
        return Err("x spec: spec must not be empty".to_owned());
    }
    if spec.chars().any(char::is_whitespace) {
        return Err(format!("x spec: spec must not contain whitespace: {spec:?}"));
    }
    let (body, sha256) = split_x_spec_sha256_pin(spec)?;
    if body.is_empty() {
        return Err("x spec: spec must name a source before #sha256:".to_owned());
    }
    Ok(XSpec { source: parse_x_source(body)?, sha256 })
}

/// Split a trailing inline `#sha256:<64hex>` pin off the spec body.
fn split_x_spec_sha256_pin(spec: &str) -> Result<(&str, Option<String>), String> {
    let Some(index) = spec.rfind("#sha256:") else {
        return Ok((spec, None));
    };
    let hex = &spec[index + "#sha256:".len()..];
    // mirrors plugin_plan.rs::normalize_plugin_install_sha256 — unify in WI-8
    if hex.len() == 64 && hex.chars().all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase()) {
        Ok((&spec[..index], Some(format!("sha256:{hex}"))))
    } else {
        Err("x spec: #sha256: pin must be 64 lowercase hex chars".to_owned())
    }
}

fn parse_x_source(body: &str) -> Result<XSource, String> {
    // Scheme prefixes BEFORE the local-path test (spec §2.1): an existing
    // dir named like a scheme'd spec must not shadow it.
    if let Some(rest) = body.strip_prefix("gh-release:") {
        return parse_x_gh_release(rest);
    }
    if let Some(rest) = body.strip_prefix("gh:") {
        return parse_x_github_scheme(rest);
    }
    if let Some(rest) = body.strip_prefix("npm:") {
        return parse_x_npm(rest);
    }
    if let Some(rest) = body.strip_prefix("file:") {
        return parse_x_local_dir(rest);
    }
    if body.starts_with("./")
        || body.starts_with("../")
        || body.starts_with('/')
        || body.starts_with("~/")
    {
        return parse_x_local_dir(body);
    }
    if is_x_explicit_git_source(body) {
        return parse_x_git_url(body);
    }
    if body.contains('/') {
        return parse_x_github_shorthand(body);
    }
    parse_x_bare_verb(body)
}

// An EXPLICIT git URL requires a real URL shape. A bare token — no '/', no
// "://", no ':' — is ALWAYS a verb (or verb@ref): verbs win the tie against
// URL-ish names like `httpx`, `http-server`, `foo.git`, `git@main`. Three
// accepted shapes:
//   - a scheme carrying "://"   (http://, https://, ssh://, git://)
//   - scp-style `git@host:path` (needs BOTH '@' and ':')
//   - a path-ish `…/repo.git`   (contains '/' AND ends in ".git")
// Deliberately stricter than plugin_plan.rs::is_explicit_git_install_source
// (which treats any `http*`/`git@`/`*.git` token as a URL and so misroutes
// bare verbs); WI-8 reconciles the two.
fn is_x_explicit_git_source(value: &str) -> bool {
    value.contains("://")
        || (value.starts_with("git@") && value.contains(':'))
        || (value.contains('/') && value.ends_with(".git"))
}

/// `gh:owner/repo[@ref][/sub]` — the canonical scheme form.
fn parse_x_github_scheme(rest: &str) -> Result<XSource, String> {
    let mut parts = rest.splitn(3, '/');
    let owner = parts.next().unwrap_or_default();
    let Some(repo_and_ref) = parts.next() else {
        return Err(format!("x spec: gh: spec must be gh:owner/repo[@ref][/sub], got 'gh:{rest}'"));
    };
    let subpath = parts.next().map(normalize_x_subpath).transpose()?;
    let (repo, reference) = split_x_ref(repo_and_ref)?;
    validate_x_owner_repo(owner, repo)?;
    Ok(XSource::GitHub {
        owner: owner.to_owned(),
        repo: repo.to_owned(),
        subpath,
        reference,
    })
}

/// `owner/repo[/sub][@ref]` (trailing ref, spec table) and
/// `owner/repo@ref[/sub]` (repo-attached ref — today's install grammar,
/// mirrors `plugin_plan.rs::parse_github_shorthand_install_source`), plus the
/// commit-pinned `owner/repo@<40hex>[/sub]` form.
fn parse_x_github_shorthand(body: &str) -> Result<XSource, String> {
    let segments: Vec<&str> = body.split('/').collect();
    if segments.iter().any(|segment| segment.is_empty()) {
        return Err(format!("x spec: empty path segment in '{body}'"));
    }
    let owner = segments[0];
    // An '@' in the owner segment means the whole spec is really a slash-bearing
    // ref (`costs@release/2026` → verb `costs` @ ref `release/2026`), which is
    // indistinguishable from `owner/repo` shorthand. Reject rather than silently
    // parse `costs@release` as an owner (verifier-proven misroute).
    if owner.contains('@') {
        return Err(format!(
            "x spec: '@' cannot appear in the owner segment of '{body}' — a \
             slash-bearing ref like 'costs@release/2026' is ambiguous in \
             shorthand; use gh:owner/repo@ref, an explicit git URL, or a \
             slash-free bare verb ref like 'costs@release'"
        ));
    }
    let (repo, mut reference) = split_x_ref(segments[1])?;
    let mut tail: Vec<&str> = segments[2..].to_vec();
    if let Some(last) = tail.last_mut() {
        if last.contains('@') {
            if reference.is_some() {
                return Err(format!(
                    "x spec: use either owner/repo@ref or a trailing @ref, not both: '{body}'"
                ));
            }
            let (stripped, trailing_reference) = split_x_ref(last)?;
            if stripped.is_empty() {
                return Err(format!("x spec: empty path segment in '{body}'"));
            }
            *last = stripped;
            reference = trailing_reference;
        }
    }
    validate_x_owner_repo(owner, repo)?;
    let subpath = if tail.is_empty() {
        None
    } else {
        Some(normalize_x_subpath(&tail.join("/"))?)
    };
    Ok(XSource::GitHub {
        owner: owner.to_owned(),
        repo: repo.to_owned(),
        subpath,
        reference,
    })
}

/// `https://…/repo.git[@ref][#sub]` and other explicit git URLs. A `@ref`
/// is only recognized after `.git` so user/host `@`s stay part of the URL.
fn parse_x_git_url(body: &str) -> Result<XSource, String> {
    let (url_and_ref, subpath) = match body.split_once('#') {
        Some((head, fragment)) => (head, Some(normalize_x_subpath(fragment)?)),
        None => (body, None),
    };
    let (url, reference) = match url_and_ref.rfind(".git@") {
        Some(index) => {
            let reference = &url_and_ref[index + ".git@".len()..];
            if reference.is_empty() {
                return Err(format!("x spec: @ref must not be empty in '{body}'"));
            }
            (&url_and_ref[..index + ".git".len()], Some(classify_x_git_ref(reference)))
        }
        None => (url_and_ref, None),
    };
    if url.is_empty() {
        return Err(format!("x spec: git url must not be empty in '{body}'"));
    }
    Ok(XSource::GitUrl {
        url: url.to_owned(),
        reference,
        subpath,
    })
}

fn parse_x_local_dir(path: &str) -> Result<XSource, String> {
    if path.is_empty() {
        return Err("x spec: file: path must not be empty".to_owned());
    }
    Ok(XSource::LocalDir { path: path.to_owned() })
}

/// `gh-release:owner/repo@tag#asset` — every part is required (v1.1).
fn parse_x_gh_release(rest: &str) -> Result<XSource, String> {
    let usage =
        || format!("x spec: gh-release: spec must be gh-release:owner/repo@tag#asset, got 'gh-release:{rest}'");
    let Some((head, asset)) = rest.split_once('#') else {
        return Err(usage());
    };
    let Some((owner_repo, tag)) = head.split_once('@') else {
        return Err(usage());
    };
    let Some((owner, repo)) = owner_repo.split_once('/') else {
        return Err(usage());
    };
    if owner.is_empty() || repo.is_empty() || repo.contains('/') || tag.is_empty() || asset.is_empty() {
        return Err(usage());
    }
    Ok(XSource::GhRelease {
        owner: owner.to_owned(),
        repo: repo.to_owned(),
        tag: tag.to_owned(),
        asset: asset.to_owned(),
    })
}

/// `npm:@scope/pkg[@ver]` (unscoped `npm:pkg[@ver]` also accepted) — v2.
fn parse_x_npm(rest: &str) -> Result<XSource, String> {
    // The version `@` is any `@` past position 0 (position 0 is the scope).
    let (package, version) = match rest.rfind('@') {
        Some(index) if index > 0 => (&rest[..index], Some(rest[index + 1..].to_owned())),
        _ => (rest, None),
    };
    if package.is_empty() {
        return Err("x spec: npm: package must not be empty".to_owned());
    }
    if version.as_deref() == Some("") {
        return Err(format!("x spec: npm: version after '@' must not be empty in 'npm:{rest}'"));
    }
    let name_part = package.strip_prefix('@');
    if let Some(scoped) = name_part {
        let valid_scope = scoped
            .split_once('/')
            .is_some_and(|(scope, name)| !scope.is_empty() && !name.is_empty() && !name.contains('/'));
        if !valid_scope {
            return Err(format!("x spec: npm: scoped package must be npm:@scope/pkg, got 'npm:{rest}'"));
        }
    } else if package.contains('/') {
        return Err(format!("x spec: npm: unscoped package must not contain '/', got 'npm:{rest}'"));
    }
    Ok(XSource::Npm { package: package.to_owned(), version })
}

fn parse_x_bare_verb(body: &str) -> Result<XSource, String> {
    let (verb, reference) = split_x_ref(body)?;
    let valid = !verb.is_empty()
        && verb
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'));
    if !valid {
        return Err(format!(
            "x spec: '{verb}' is not a valid verb (ascii alphanumeric, '-', '_', '.')"
        ));
    }
    Ok(XSource::Verb { verb: verb.to_owned(), reference })
}

/// Split an optional `@ref` off a segment; empty refs are an error.
fn split_x_ref(value: &str) -> Result<(&str, Option<XGitRef>), String> {
    let Some((head, reference)) = value.split_once('@') else {
        return Ok((value, None));
    };
    if reference.is_empty() {
        return Err(format!("x spec: @ref must not be empty in '{value}'"));
    }
    Ok((head, Some(classify_x_git_ref(reference))))
}

/// 40 lowercase hex = commit pin; anything else is a branch/tag name.
fn classify_x_git_ref(reference: &str) -> XGitRef {
    if reference.len() == 40
        && reference
            .chars()
            .all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase())
    {
        XGitRef::Commit(reference.to_owned())
    } else {
        XGitRef::Named(reference.to_owned())
    }
}

fn validate_x_owner_repo(owner: &str, repo: &str) -> Result<(), String> {
    let valid = |part: &str| !part.is_empty() && part != "." && part != ".." && !part.contains('\\');
    if valid(owner) && valid(repo) {
        Ok(())
    } else {
        Err(format!("x spec: owner and repo must be non-empty path segments, got '{owner}/{repo}'"))
    }
}

// mirrors plugin_plan.rs::normalize_plugin_install_subpath — unify in WI-8
// (string-based so the parser stays pure; identical component rules).
fn normalize_x_subpath(subpath: &str) -> Result<String, String> {
    let valid = !subpath.is_empty()
        && !subpath.starts_with('/')
        && !subpath.contains('\\')
        && subpath
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..");
    if valid {
        Ok(subpath.to_owned())
    } else {
        Err(format!(
            "x spec: subpath must be a non-empty relative directory without '.' or '..', got '{subpath}'"
        ))
    }
}

#[cfg(test)]
mod x_spec_parser_tests {
    use super::*;

    const HEX40: &str = "0123456789abcdef0123456789abcdef01234567";
    const HEX64: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn parsed(spec: &str) -> XSpec {
        parse_x_spec(spec).unwrap_or_else(|error| panic!("spec '{spec}' should parse: {error}"))
    }

    fn github(owner: &str, repo: &str, subpath: Option<&str>, reference: Option<XGitRef>) -> XSource {
        XSource::GitHub {
            owner: owner.to_owned(),
            repo: repo.to_owned(),
            subpath: subpath.map(str::to_owned),
            reference,
        }
    }

    // ── grammar rows (§2.1 table, one test per row) ────────────────────

    #[test]
    fn parses_bare_verb() {
        assert_eq!(
            parsed("costs"),
            XSpec { source: XSource::Verb { verb: "costs".to_owned(), reference: None }, sha256: None }
        );
    }

    #[test]
    fn parses_bare_verb_with_ref() {
        assert_eq!(
            parsed("costs@main").source,
            XSource::Verb { verb: "costs".to_owned(), reference: Some(XGitRef::Named("main".to_owned())) }
        );
        assert_eq!(
            parsed("costs@v26.7.15").source,
            XSource::Verb {
                verb: "costs".to_owned(),
                reference: Some(XGitRef::Named("v26.7.15".to_owned())),
            }
        );
    }

    #[test]
    fn parses_github_shorthand() {
        assert_eq!(parsed("acme/maw-tools").source, github("acme", "maw-tools", None, None));
        assert_eq!(
            parsed("acme/maw-tools/packages/20-costs").source,
            github("acme", "maw-tools", Some("packages/20-costs"), None)
        );
    }

    #[test]
    fn parses_github_shorthand_trailing_ref() {
        // spec table form: owner/repo[/sub/path][@ref]
        assert_eq!(
            parsed("acme/maw-tools/packages/costs@main").source,
            github("acme", "maw-tools", Some("packages/costs"), Some(XGitRef::Named("main".to_owned())))
        );
    }

    #[test]
    fn parses_github_shorthand_repo_attached_ref() {
        // today's install grammar (mirrors plugin_plan.rs): owner/repo@ref/sub
        assert_eq!(
            parsed("acme/maw-tools@main/packages/costs").source,
            github("acme", "maw-tools", Some("packages/costs"), Some(XGitRef::Named("main".to_owned())))
        );
        assert_eq!(
            parsed("acme/maw-tools@main").source,
            github("acme", "maw-tools", None, Some(XGitRef::Named("main".to_owned())))
        );
    }

    #[test]
    fn parses_commit_pinned_shorthand() {
        // owner/repo@<40hex>[/sub] — a 40-hex @ref is a commit pin
        assert_eq!(
            parsed(&format!("acme/maw-tools@{HEX40}/packages/costs")).source,
            github(
                "acme",
                "maw-tools",
                Some("packages/costs"),
                Some(XGitRef::Commit(HEX40.to_owned())),
            )
        );
    }

    #[test]
    fn parses_inline_sha256_pin() {
        let spec = parsed(&format!("costs#sha256:{HEX64}"));
        assert_eq!(spec.sha256.as_deref(), Some(format!("sha256:{HEX64}").as_str()));
        assert_eq!(spec.source, XSource::Verb { verb: "costs".to_owned(), reference: None });
        let spec = parsed(&format!("gh:acme/maw-tools#sha256:{HEX64}"));
        assert_eq!(spec.sha256.as_deref(), Some(format!("sha256:{HEX64}").as_str()));
        assert_eq!(spec.source, github("acme", "maw-tools", None, None));
    }

    #[test]
    fn parses_gh_scheme() {
        assert_eq!(parsed("gh:acme/maw-tools").source, github("acme", "maw-tools", None, None));
        assert_eq!(
            parsed("gh:acme/maw-tools@main/packages/costs").source,
            github("acme", "maw-tools", Some("packages/costs"), Some(XGitRef::Named("main".to_owned())))
        );
        assert_eq!(
            parsed(&format!("gh:acme/maw-tools@{HEX40}")).source,
            github("acme", "maw-tools", None, Some(XGitRef::Commit(HEX40.to_owned())))
        );
    }

    #[test]
    fn parses_https_git_url() {
        assert_eq!(
            parsed("https://example.com/acme/maw-tools.git").source,
            XSource::GitUrl {
                url: "https://example.com/acme/maw-tools.git".to_owned(),
                reference: None,
                subpath: None,
            }
        );
        assert_eq!(
            parsed("https://example.com/acme/maw-tools.git@main#packages/costs").source,
            XSource::GitUrl {
                url: "https://example.com/acme/maw-tools.git".to_owned(),
                reference: Some(XGitRef::Named("main".to_owned())),
                subpath: Some("packages/costs".to_owned()),
            }
        );
    }

    #[test]
    fn parses_git_ssh_url() {
        assert_eq!(
            parsed("git@github.com:acme/maw-tools.git@main").source,
            XSource::GitUrl {
                url: "git@github.com:acme/maw-tools.git".to_owned(),
                reference: Some(XGitRef::Named("main".to_owned())),
                subpath: None,
            }
        );
    }

    #[test]
    fn parses_local_dirs() {
        assert_eq!(parsed("file:./plugins/costs").source, XSource::LocalDir { path: "./plugins/costs".to_owned() });
        assert_eq!(parsed("./plugins/costs").source, XSource::LocalDir { path: "./plugins/costs".to_owned() });
        assert_eq!(parsed("../plugins/costs").source, XSource::LocalDir { path: "../plugins/costs".to_owned() });
        assert_eq!(parsed("/abs/plugins/costs").source, XSource::LocalDir { path: "/abs/plugins/costs".to_owned() });
        assert_eq!(parsed("~/plugins/costs").source, XSource::LocalDir { path: "~/plugins/costs".to_owned() });
    }

    #[test]
    fn parses_gh_release_parse_only_v1_1() {
        let spec = parsed("gh-release:acme/maw-tools@v1.2.3#costs.tar.gz");
        assert_eq!(
            spec.source,
            XSource::GhRelease {
                owner: "acme".to_owned(),
                repo: "maw-tools".to_owned(),
                tag: "v1.2.3".to_owned(),
                asset: "costs.tar.gz".to_owned(),
            }
        );
        assert_eq!(spec.tier(), XSpecTier::V1Dot1);
    }

    #[test]
    fn parses_npm_parse_only_v2() {
        let spec = parsed("npm:@maw-rs/costs");
        assert_eq!(
            spec.source,
            XSource::Npm { package: "@maw-rs/costs".to_owned(), version: None }
        );
        assert_eq!(spec.tier(), XSpecTier::V2);
        assert_eq!(
            parsed("npm:@maw-rs/costs@1.2.3").source,
            XSource::Npm { package: "@maw-rs/costs".to_owned(), version: Some("1.2.3".to_owned()) }
        );
        assert_eq!(
            parsed("npm:costs@1.2.3").source,
            XSource::Npm { package: "costs".to_owned(), version: Some("1.2.3".to_owned()) }
        );
    }

    #[test]
    fn v1_sources_report_v1_tier() {
        for spec in ["costs", "acme/maw-tools", "gh:acme/maw-tools", "./dir", "https://example.com/r.git"] {
            assert_eq!(parsed(spec).tier(), XSpecTier::V1, "tier of '{spec}'");
        }
    }

    // ── disambiguation (spec §2.1 rules) ───────────────────────────────

    #[test]
    fn scheme_prefix_wins_over_local_path_test() {
        // The parser never consults the filesystem: a dir literally named
        // "gh:acme" or "npm:@scope" can never shadow the scheme.
        assert!(matches!(parsed("gh:acme/maw-tools").source, XSource::GitHub { .. }));
        assert!(matches!(parsed("npm:@scope/pkg").source, XSource::Npm { .. }));
        assert!(matches!(parsed("gh-release:a/b@t#x").source, XSource::GhRelease { .. }));
        assert!(matches!(parsed("file:gh:not-a-scheme").source, XSource::LocalDir { .. }));
    }

    #[test]
    fn forty_hex_ref_is_commit_everything_else_named() {
        assert_eq!(
            parsed(&format!("costs@{HEX40}")).source,
            XSource::Verb { verb: "costs".to_owned(), reference: Some(XGitRef::Commit(HEX40.to_owned())) }
        );
        // 39 hex chars → branch/tag name
        let hex39 = &HEX40[..39];
        assert!(matches!(
            parsed(&format!("acme/repo@{hex39}")).source,
            XSource::GitHub { reference: Some(XGitRef::Named(_)), .. }
        ));
        // 40 chars but not hex → named
        let not_hex = format!("{}g", &HEX40[..39]);
        assert!(matches!(
            parsed(&format!("acme/repo@{not_hex}")).source,
            XSource::GitHub { reference: Some(XGitRef::Named(_)), .. }
        ));
        // uppercase hex is not a canonical commit pin → named
        let upper = HEX40.to_uppercase();
        assert!(matches!(
            parsed(&format!("acme/repo@{upper}")).source,
            XSource::GitHub { reference: Some(XGitRef::Named(_)), .. }
        ));
    }

    #[test]
    fn bare_verb_vs_explicit_local_path() {
        assert!(matches!(parsed("costs").source, XSource::Verb { .. }));
        assert!(matches!(parsed("./costs").source, XSource::LocalDir { .. }));
    }

    #[test]
    fn verbs_win_over_url_ish_bare_tokens() {
        // A bare token (no '/', no "://", no ':') is ALWAYS a verb, even when it
        // looks URL-ish. Verbs win the tie against `is_x_explicit_git_source`.
        assert_eq!(
            parsed("httpx").source,
            XSource::Verb { verb: "httpx".to_owned(), reference: None }
        );
        assert_eq!(
            parsed("http-server").source,
            XSource::Verb { verb: "http-server".to_owned(), reference: None }
        );
        // `git@main` is verb "git" @ ref "main", NOT a git@host: URL (no ':').
        assert_eq!(
            parsed("git@main").source,
            XSource::Verb { verb: "git".to_owned(), reference: Some(XGitRef::Named("main".to_owned())) }
        );
        // Real URL shapes still route to GitUrl.
        assert!(matches!(parsed("https://x/y.git").source, XSource::GitUrl { .. }));
        assert!(matches!(parsed("git@host:x/y.git").source, XSource::GitUrl { .. }));
    }

    // ── rejects ────────────────────────────────────────────────────────

    #[test]
    fn rejects_bad_sha256_pins() {
        for bad in [
            format!("costs#sha256:{}", &HEX64[..63]),
            format!("costs#sha256:{HEX64}0"),
            format!("costs#sha256:{}", HEX64.to_uppercase()),
            "costs#sha256:abc".to_owned(),
            "costs#sha256:".to_owned(),
        ] {
            let error = parse_x_spec(&bad).unwrap_err();
            assert!(
                error.contains("64 lowercase hex chars"),
                "'{bad}' should reject with the hex-length message, got: {error}"
            );
        }
    }

    #[test]
    fn rejects_empty_and_ref_only() {
        assert!(parse_x_spec("").is_err());
        assert!(parse_x_spec("@main").is_err());
        assert!(parse_x_spec("costs@").is_err());
        assert!(parse_x_spec(&format!("#sha256:{HEX64}")).is_err());
        assert!(parse_x_spec("file:").is_err());
        assert!(parse_x_spec("npm:").is_err());
        assert!(parse_x_spec("gh:only-owner").is_err());
    }

    #[test]
    fn rejects_whitespace() {
        for bad in [" costs", "costs ", "costs @main", "acme/my repo", "costs\tmain", "a\nb"] {
            assert!(parse_x_spec(bad).is_err(), "'{}' should reject", bad.escape_default());
        }
    }

    #[test]
    fn rejects_empty_or_traversing_segments() {
        assert!(parse_x_spec("acme//costs").is_err());
        assert!(parse_x_spec("acme/").is_err());
        assert!(parse_x_spec("acme/repo/../escape").is_err());
        assert!(parse_x_spec("gh:acme/repo/./x").is_err());
        assert!(parse_x_spec("gh:./repo").is_err());
        assert!(parse_x_spec("https://example.com/r.git#../escape").is_err());
    }

    #[test]
    fn rejects_double_ref() {
        let error = parse_x_spec("acme/repo@main/sub@dev").unwrap_err();
        assert!(error.contains("not both"), "unexpected message: {error}");
    }

    #[test]
    fn rejects_malformed_gh_release_and_npm() {
        assert!(parse_x_spec("gh-release:acme/repo@v1").is_err()); // missing #asset
        assert!(parse_x_spec("gh-release:acme@v1#asset").is_err()); // missing /repo
        assert!(parse_x_spec("gh-release:acme/repo#asset").is_err()); // missing @tag
        assert!(parse_x_spec("npm:@scope").is_err()); // scoped without /pkg
        assert!(parse_x_spec("npm:costs@").is_err()); // empty version
        assert!(parse_x_spec("npm:a/b").is_err()); // unscoped with '/'
    }

    #[test]
    fn rejects_at_in_owner_segment() {
        // `costs@release/2026` must not silently parse as owner "costs@release"
        // /repo "2026" — a slash-bearing ref is ambiguous in shorthand, so we
        // reject and name the ambiguity (spec §2.1 disambiguation).
        let error = parse_x_spec("costs@release/2026").unwrap_err();
        assert!(
            error.contains("owner segment") && error.contains("ambiguous"),
            "unexpected message: {error}"
        );
    }

    #[test]
    fn rejects_invalid_verb_characters() {
        assert!(parse_x_spec("co$ts").is_err());
        assert!(parse_x_spec("costs!").is_err());
    }

    // ── canonical round-trip (every variant) ───────────────────────────

    #[test]
    fn canonical_form_is_gh_scheme() {
        // spec §2.1: gh: is the canonical form the trust store/lock records
        assert_eq!(parsed("acme/maw-tools@main").canonical(), "gh:acme/maw-tools@main");
        assert_eq!(
            parsed("acme/maw-tools/packages/costs@main").canonical(),
            "gh:acme/maw-tools@main/packages/costs"
        );
        assert_eq!(parsed("./dir").canonical(), "file:./dir");
    }

    #[test]
    fn canonical_round_trips_every_variant() {
        let canonical_specs = [
            "costs".to_owned(),
            "costs@main".to_owned(),
            format!("costs@{HEX40}"),
            format!("costs#sha256:{HEX64}"),
            "gh:acme/maw-tools".to_owned(),
            "gh:acme/maw-tools@main".to_owned(),
            "gh:acme/maw-tools@main/packages/costs".to_owned(),
            format!("gh:acme/maw-tools@{HEX40}/packages/costs"),
            format!("gh:acme/maw-tools#sha256:{HEX64}"),
            "https://example.com/acme/maw-tools.git".to_owned(),
            "https://example.com/acme/maw-tools.git@main#packages/costs".to_owned(),
            format!("https://example.com/acme/maw-tools.git@main#packages/costs#sha256:{HEX64}"),
            "git@github.com:acme/maw-tools.git".to_owned(),
            "file:./plugins/costs".to_owned(),
            "file:../plugins/costs".to_owned(),
            "file:/abs/plugins/costs".to_owned(),
            "gh-release:acme/maw-tools@v1.2.3#costs.tar.gz".to_owned(),
            "npm:@maw-rs/costs".to_owned(),
            "npm:@maw-rs/costs@1.2.3".to_owned(),
            "npm:costs@1.2.3".to_owned(),
        ];
        for spec in &canonical_specs {
            let first = parsed(spec);
            // canonical of a canonical spec is itself (idempotent)…
            assert_eq!(&first.canonical(), spec, "canonical('{spec}') should be stable");
            // …and reparsing the canonical form yields the same value.
            assert_eq!(parsed(&first.canonical()), first, "round-trip of '{spec}'");
        }
    }

    #[test]
    fn non_canonical_inputs_round_trip_through_canonical() {
        // shorthand / trailing-ref / bare-dir inputs normalize to the gh: /
        // file: canonical forms and stay stable from there.
        for spec in [
            "acme/maw-tools".to_owned(),
            "acme/maw-tools@main/packages/costs".to_owned(),
            "acme/maw-tools/packages/costs@main".to_owned(),
            format!("acme/maw-tools@{HEX40}"),
            "./plugins/costs".to_owned(),
            "~/plugins/costs".to_owned(),
            format!("acme/maw-tools@main#sha256:{HEX64}"),
        ] {
            let first = parsed(&spec);
            let canonical = first.canonical();
            assert_eq!(parsed(&canonical), first, "reparse of canonical('{spec}')");
            assert_eq!(parsed(&canonical).canonical(), canonical, "idempotent for '{spec}'");
        }
    }
}
