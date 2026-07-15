//maw:order 126
// Helpers for the maw more per-coder spawn phase; dispatcher wiring lands separately.

const MORE_SPAWN_CODEX_ENGINE: &str = "codex";
const MORE_SPAWN_ENGINE_MARKER: &str = ".maw-engine";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnResult {
    pub window_name: String,
    pub worktree_path: std::path::PathBuf,
    pub branch: String,
    pub engine: String,
    pub success: bool,
}

trait MoreSpawnRuntime {
    fn more_spawn_repo_root(&mut self) -> Result<std::path::PathBuf, String>;
    fn more_spawn_git(&mut self, cwd: &std::path::Path, args: &[&str]) -> Result<(), String>;
    fn more_spawn_path_exists(&self, path: &std::path::Path) -> bool;
    fn more_spawn_write_file(
        &mut self,
        path: &std::path::Path,
        contents: &str,
    ) -> Result<(), String>;
}

struct MoreSpawnSystemRuntime;

impl MoreSpawnRuntime for MoreSpawnSystemRuntime {
    fn more_spawn_repo_root(&mut self) -> Result<std::path::PathBuf, String> {
        let output = std::process::Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .output()
            .map_err(|error| format!("more spawn: failed to run git rev-parse: {error}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            return if stderr.is_empty() {
                Err("more spawn: git rev-parse failed".to_owned())
            } else {
                Err(format!("more spawn: git rev-parse failed: {stderr}"))
            };
        }
        let root = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if root.is_empty() {
            Err("more spawn: git rev-parse returned an empty repo root".to_owned())
        } else {
            Ok(std::path::PathBuf::from(root))
        }
    }

    fn more_spawn_git(&mut self, cwd: &std::path::Path, args: &[&str]) -> Result<(), String> {
        let output = std::process::Command::new("git")
            .current_dir(cwd)
            .args(args)
            .output()
            .map_err(|error| format!("more spawn: failed to run git: {error}"))?;
        if output.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        if stderr.is_empty() {
            Err("more spawn: git worktree add failed".to_owned())
        } else {
            Err(format!("more spawn: git worktree add failed: {stderr}"))
        }
    }

    fn more_spawn_path_exists(&self, path: &std::path::Path) -> bool {
        path.exists()
    }

    fn more_spawn_write_file(
        &mut self,
        path: &std::path::Path,
        contents: &str,
    ) -> Result<(), String> {
        std::fs::write(path, contents)
            .map_err(|error| format!("more spawn: write {} failed: {error}", path.display()))
    }
}

/// Create one `maw more codex` worktree and write its bare engine marker.
///
/// # Errors
///
/// Returns an error when the spawn arguments are unsafe, the repository root cannot be resolved,
/// `git worktree add` fails, the created worktree cannot be found on disk, or the `.maw-engine`
/// marker cannot be written.
pub fn more_spawn_codex(prefix: &str, n: u32, base: &str) -> Result<SpawnResult, String> {
    more_spawn_codex_engine(prefix, n, base, MORE_SPAWN_CODEX_ENGINE)
}

fn more_spawn_codex_engine(
    prefix: &str,
    n: u32,
    base: &str,
    engine: &str,
) -> Result<SpawnResult, String> {
    let mut runtime = MoreSpawnSystemRuntime;
    more_spawn_codex_with_runtime(prefix, n, base, engine, &mut runtime)
}

fn more_spawn_codex_with_runtime(
    prefix: &str,
    n: u32,
    base: &str,
    engine: &str,
    runtime: &mut impl MoreSpawnRuntime,
) -> Result<SpawnResult, String> {
    more_spawn_validate_token(prefix, "prefix")?;
    more_spawn_validate_index(n)?;
    more_spawn_validate_base(base)?;
    more_spawn_validate_token(engine, "engine")?;

    let window_name = format!("{prefix}-{MORE_SPAWN_CODEX_ENGINE}-{n}");
    let worktree_relative = std::path::PathBuf::from("agents").join(&window_name);
    let branch = format!("agents/{window_name}");
    let repo_root = runtime.more_spawn_repo_root()?;
    let worktree_path = repo_root.join(&worktree_relative);
    let worktree_arg = worktree_relative.to_string_lossy().into_owned();

    runtime.more_spawn_git(
        &repo_root,
        &["worktree", "add", &worktree_arg, "-b", &branch, base],
    )?;

    if !runtime.more_spawn_path_exists(&worktree_path) {
        return Err(format!(
            "more spawn: worktree missing after git add: {}",
            worktree_path.display()
        ));
    }

    runtime.more_spawn_write_file(
        &worktree_path.join(MORE_SPAWN_ENGINE_MARKER),
        engine,
    )?;

    Ok(SpawnResult {
        window_name,
        worktree_path,
        branch,
        engine: engine.to_owned(),
        success: true,
    })
}

fn more_spawn_validate_index(n: u32) -> Result<(), String> {
    if n == 0 {
        Err("more spawn: coder index must be greater than zero".to_owned())
    } else {
        Ok(())
    }
}

fn more_spawn_validate_token(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("more spawn: {label} is empty"));
    }
    if value.starts_with('-') {
        return Err(format!(
            "more spawn: invalid {label} '{value}': leading dash rejected"
        ));
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
    {
        return Err(format!(
            "more spawn: invalid {label} '{value}': use only ASCII letters, digits, '.', '_' or '-'"
        ));
    }
    Ok(())
}

fn more_spawn_validate_base(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.starts_with('-')
        || value.chars().any(char::is_whitespace)
        || value.chars().any(char::is_control)
    {
        return Err("more spawn: base must be a non-option git ref without whitespace".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod more_spawn_tests {
    use super::*;
    use std::collections::BTreeSet;

    #[derive(Debug, Default)]
    struct FakeMoreSpawnRuntime {
        repo_root: std::path::PathBuf,
        git_calls: Vec<(std::path::PathBuf, Vec<String>)>,
        existing_paths: BTreeSet<std::path::PathBuf>,
        writes: Vec<(std::path::PathBuf, String)>,
        create_worktree_after_git: bool,
    }

    impl MoreSpawnRuntime for FakeMoreSpawnRuntime {
        fn more_spawn_repo_root(&mut self) -> Result<std::path::PathBuf, String> {
            Ok(self.repo_root.clone())
        }

        fn more_spawn_git(&mut self, cwd: &std::path::Path, args: &[&str]) -> Result<(), String> {
            self.git_calls.push((cwd.to_path_buf(), strings(args)));
            if self.create_worktree_after_git {
                if let Some(path) = args.get(2) {
                    self.existing_paths.insert(cwd.join(path));
                }
            }
            Ok(())
        }

        fn more_spawn_path_exists(&self, path: &std::path::Path) -> bool {
            self.existing_paths.contains(path)
        }

        fn more_spawn_write_file(
            &mut self,
            path: &std::path::Path,
            contents: &str,
        ) -> Result<(), String> {
            self.writes.push((path.to_path_buf(), contents.to_owned()));
            Ok(())
        }
    }

    #[test]
    fn codex_spawn_adds_worktree_and_writes_bare_engine_marker() {
        let mut runtime = FakeMoreSpawnRuntime {
            repo_root: std::path::PathBuf::from("/repo"),
            create_worktree_after_git: true,
            ..FakeMoreSpawnRuntime::default()
        };

        let result =
            more_spawn_codex_with_runtime("mawjs", 2, "origin/alpha", "omx-3", &mut runtime)
                .expect("codex spawn succeeds");

        let worktree_path = std::path::PathBuf::from("/repo/agents/mawjs-codex-2");
        assert_eq!(
            result,
            SpawnResult {
                window_name: "mawjs-codex-2".to_owned(),
                worktree_path: worktree_path.clone(),
                branch: "agents/mawjs-codex-2".to_owned(),
                engine: "omx-3".to_owned(),
                success: true,
            }
        );
        assert_eq!(
            runtime.git_calls,
            vec![(
                std::path::PathBuf::from("/repo"),
                strings(&[
                    "worktree",
                    "add",
                    "agents/mawjs-codex-2",
                    "-b",
                    "agents/mawjs-codex-2",
                    "origin/alpha",
                ]),
            )]
        );
        assert_eq!(
            runtime.writes,
            vec![(worktree_path.join(".maw-engine"), "omx-3".to_owned())]
        );
    }

    #[test]
    fn codex_spawn_refuses_missing_worktree_after_git_add() {
        let mut runtime = FakeMoreSpawnRuntime {
            repo_root: std::path::PathBuf::from("/repo"),
            ..FakeMoreSpawnRuntime::default()
        };

        let error = more_spawn_codex_with_runtime("mawjs", 1, "alpha", "codex", &mut runtime)
            .expect_err("missing worktree is an error");

        assert!(error.contains("worktree missing after git add"), "{error}");
        assert!(runtime.writes.is_empty());
    }

    #[test]
    fn codex_spawn_rejects_unsafe_inputs_before_git() {
        let mut runtime = FakeMoreSpawnRuntime::default();

        let prefix_error = more_spawn_codex_with_runtime("../bad", 1, "alpha", "codex", &mut runtime)
            .expect_err("bad prefix rejected");
        let index_error = more_spawn_codex_with_runtime("mawjs", 0, "alpha", "codex", &mut runtime)
            .expect_err("bad index rejected");
        let base_error =
            more_spawn_codex_with_runtime("mawjs", 1, "--upload-pack=sh", "codex", &mut runtime)
                .expect_err("bad base rejected");
        let engine_error =
            more_spawn_codex_with_runtime("mawjs", 1, "alpha", "bad/engine", &mut runtime)
                .expect_err("bad engine rejected");

        assert!(prefix_error.contains("invalid prefix"), "{prefix_error}");
        assert!(index_error.contains("index"), "{index_error}");
        assert!(base_error.contains("base"), "{base_error}");
        assert!(engine_error.contains("invalid engine"), "{engine_error}");
        assert!(runtime.git_calls.is_empty());
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }
}
