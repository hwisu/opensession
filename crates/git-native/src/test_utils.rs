use std::path::Path;
use std::process::Command;

/// Run a git command in tests while isolating hook-provided git env vars.
pub fn run_git(dir: &Path, args: &[&str]) -> std::process::Output {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .output()
        .expect("git command failed to spawn");
    assert!(
        output.status.success(),
        "git {} failed with status {:?}\nstdout: {}\nstderr: {}",
        args.join(" "),
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

/// Initialize a minimal git repository for testing.
///
/// Creates a repo with an initial commit so that HEAD exists.
pub fn init_test_repo(dir: &Path) {
    run_git(dir, &["init", "--initial-branch=main"]);
    run_git(dir, &["config", "user.email", "test@test.com"]);
    run_git(dir, &["config", "user.name", "Test"]);

    std::fs::write(dir.join("README"), "test repo").unwrap();
    run_git(dir, &["add", "."]);
    run_git(dir, &["commit", "-m", "init"]);
}
