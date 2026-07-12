use maw_schedule::{ExecMode, RunStatus};
use maw_schedule_runner::{exec::{execute, resolve_binary_in}, FireStore, StartRequest};
use std::{os::unix::fs::PermissionsExt, path::{Path, PathBuf}, sync::atomic::{AtomicU64, Ordering}};
#[rustfmt::skip]
fn root() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!("maw-schedule-exec-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&path).unwrap(); path
}
fn script(path: &Path, body: &str) {
    std::fs::write(path, body).unwrap();
    let mut permissions = path.metadata().unwrap().permissions(); permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).unwrap();
}
#[rustfmt::skip]
fn request(root: &Path, mode: ExecMode, command: &str) -> StartRequest {
    StartRequest { run_id: "run-1".into(), oracle: "odin".into(), job_id: "digest".into(),
        local_date: "2026-07-13".into(), reserved_at: 1, cadence_seconds: 60,
        boot_identity: "boot-a".into(), cap: 1, forced: false, exec: mode,
        expected_output: None, command: command.into(), cwd: root.join("repo").to_string_lossy().into(),
        log_path: root.join("job.log").to_string_lossy().into(), output_path: None,
        token_name: "t2".into(), bash_path: "/bin/bash".into(), claude_path: None, pass_path: None }
}
#[test]
#[rustfmt::skip]
fn claude_uses_absolute_pass_and_structural_prompt_then_finalizes_deliverable() {
    let root = root(); let repo = root.join("repo");
    std::fs::create_dir_all(repo.join("out/2026-07-13")).unwrap();
    let pass = root.join("pass"); script(&pass, "#!/bin/sh\nprintf 'secret-token\\n'");
    let capture = root.join("prompt"); let token = root.join("token");
    let deliverable = repo.join("out/2026-07-13/0900.md");
    let claude = root.join("claude");
    script(&claude, &format!("#!/bin/sh\nprintf '%s' \"$3\" > '{}'\nprintf '%s' \"$CLAUDE_CODE_OAUTH_TOKEN\" > '{}'\nprintf 'answer\\n'\nprintf 'digest\\n' > '{}'\n", capture.display(), token.display(), deliverable.display()));
    let command = "quotes: ' \" $ ( ) unicode=สวัสดี\n## WHO Matrix";
    let mut request = request(&root, ExecMode::ClaudeHeadless, command);
    request.expected_output = Some("out/$TODAY/$HOUR00.md".into());
    request.output_path = Some(root.join("session.log").to_string_lossy().into());
    request.claude_path = Some(claude.to_string_lossy().into()); request.pass_path = Some(pass.to_string_lossy().into());
    let store = FireStore::new(root.join("state")); store.reserve(request).unwrap();
    let run = execute(&store, "run-1", "2026-07-13", "09", None).unwrap();
    assert_eq!((run.outcome.status, run.outcome.deliverable_written), (RunStatus::Succeeded, Some(true)));
    assert!(run.output_bytes > 0); assert_eq!(std::fs::read_to_string(capture).unwrap(), command);
    assert_eq!(std::fs::read_to_string(token).unwrap(), "secret-token");
    assert_eq!(run.outcome.expected_output.as_deref(), deliverable.canonicalize().unwrap().to_str());
    let log = std::fs::read_to_string(root.join("job.log")).unwrap(); assert!(log.contains("START") && log.contains("END status=Succeeded"));
}
#[test]
#[rustfmt::skip]
fn credential_and_child_failures_finalize_and_log_before_returning() {
    let first_root = root(); std::fs::create_dir_all(first_root.join("repo")).unwrap();
    let pass = first_root.join("pass"); script(&pass, "#!/bin/sh\nexit 1\n");
    let claude = first_root.join("claude"); script(&claude, "#!/bin/sh\nexit 0\n");
    let mut first_request = request(&first_root, ExecMode::ClaudeHeadless, "never spawned");
    first_request.output_path = Some(first_root.join("session.log").to_string_lossy().into());
    first_request.claude_path = Some(claude.to_string_lossy().into()); first_request.pass_path = Some(pass.to_string_lossy().into());
    let store = FireStore::new(first_root.join("state")); store.reserve(first_request).unwrap();
    let failed = execute(&store, "run-1", "2026-07-13", "09", None).unwrap();
    assert_eq!(failed.outcome.status, RunStatus::Failed);
    let log = std::fs::read_to_string(first_root.join("job.log")).unwrap();
    assert!(log.contains("ERROR oauth token hydration failed") && log.contains("END status=Failed"));
    let shell_root = root(); std::fs::create_dir_all(shell_root.join("repo")).unwrap();
    let store = FireStore::new(shell_root.join("state")); store.reserve(request(&shell_root, ExecMode::Shell, "exit 7")).unwrap();
    assert_eq!(execute(&store, "run-1", "2026-07-13", "09", None).unwrap().outcome.status, RunStatus::Failed);
    assert!(std::fs::read_to_string(shell_root.join("job.log")).unwrap().contains("ERROR child exited 7"));
}
#[test]
#[rustfmt::skip]
fn expected_output_expands_yesterday_across_month_and_year_end() {
    for (today, yesterday) in [("2026-03-01", "2026-02-28"), ("2026-01-01", "2025-12-31")] {
        let root = root(); std::fs::create_dir_all(root.join(format!("repo/digest/{yesterday}"))).unwrap();
        let mut request = request(&root, ExecMode::Shell, "true");
        request.expected_output = Some("digest/$YESTERDAY/result.md".into());
        let store = FireStore::new(root.join("state")); store.reserve(request).unwrap();
        let run = execute(&store, "run-1", today, "00", None).unwrap();
        let expected = root.join("repo").canonicalize().unwrap().join(format!("digest/{yesterday}/result.md"));
        assert_eq!(run.outcome.expected_output.as_deref(), expected.to_str());
    }
}
#[test]
fn fixed_path_resolution_never_needs_ambient_path() {
    let root = root(); let binary = root.join("claude"); script(&binary, "#!/bin/sh\n");
    assert_eq!(resolve_binary_in("claude", std::slice::from_ref(&root)).unwrap(), binary);
    assert!(resolve_binary_in("missing", &[root]).unwrap_err().contains("fixed search path"));
}
