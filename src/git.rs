use std::path::{Path, PathBuf};
use std::process::Command;

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).current_dir(dir).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The remote default branch ("origin/main"), falling back to a local main/master.
pub fn default_base(dir: &Path) -> Option<String> {
    if let Some(head) = git(dir, &["symbolic-ref", "-q", "--short", "refs/remotes/origin/HEAD"]) {
        if !head.is_empty() {
            return Some(head);
        }
    }
    ["origin/main", "origin/master", "main", "master"]
        .into_iter()
        .find(|r| git(dir, &["rev-parse", "-q", "--verify", &format!("{r}^{{commit}}")]).is_some())
        .map(str::to_string)
}

/// The main checkout of `dir`'s repo; Herdr refuses to create a worktree from a linked one.
pub fn main_checkout(dir: &Path) -> Option<PathBuf> {
    let common = git(dir, &["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
    let common = Path::new(&common);
    (common.file_name()? == ".git").then(|| common.parent().map(Path::to_path_buf))?
}

/// Base refs for a new worktree: the remote default first, then local branches by recency.
pub fn base_refs(dir: &Path) -> Vec<String> {
    let default = default_base(dir);
    let default_name = default.as_deref().map(|d| d.rsplit('/').next().unwrap_or(d).to_string());
    let mut out: Vec<String> = default.into_iter().collect();
    if let Some(list) = git(dir, &["for-each-ref", "--sort=-committerdate", "--format=%(refname:short)", "refs/heads/"]) {
        out.extend(list.lines().map(str::trim).filter(|b| !b.is_empty() && Some(*b) != default_name.as_deref()).map(str::to_string));
    }
    out
}

/// A branch name git accepts: lowercase, `/` kept for grouping, everything else
/// git rejects (spaces, `..`, `~^:?*[\\`, `@{`, leading `-`, `.lock`) turned into `-`.
pub fn branch_slug(raw: &str) -> String {
    let mut out = String::new();
    for c in raw.trim().chars().map(|c| c.to_ascii_lowercase()) {
        let keep = c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/');
        if keep {
            if (c == '.' && out.ends_with('.')) || (c == '/' && out.ends_with('/')) {
                continue;
            }
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let mut out = out.replace("@{", "-");
    while out.ends_with(".lock") {
        out.truncate(out.len() - 5);
    }
    out.split('/')
        .map(|seg| seg.trim_matches(|c| c == '-' || c == '.'))
        .filter(|seg| !seg.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    #[test]
    fn main_checkout_resolves_from_a_linked_worktree() {
        let tmp = std::env::temp_dir().join(format!("raycast-wt-{}", std::process::id()));
        let main = tmp.join("main");
        std::fs::create_dir_all(&main).unwrap();
        let run = |dir: &std::path::Path, args: &[&str]| {
            assert!(std::process::Command::new("git").args(args).current_dir(dir).output().unwrap().status.success());
        };
        run(&main, &["init", "-q"]);
        run(&main, &["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-q", "--allow-empty", "-m", "x"]);
        run(&main, &["worktree", "add", "-q", "../linked"]);
        let main = main.canonicalize().unwrap();
        assert_eq!(super::main_checkout(&tmp.join("linked")), Some(main.clone()));
        assert_eq!(super::main_checkout(&main), Some(main));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    use super::*;

    #[test]
    fn branch_slugs_pass_git_check() {
        assert_eq!(branch_slug("Fix 502 Errors!"), "fix-502-errors");
        assert_eq!(branch_slug("feature/ATL-12: thing"), "feature/atl-12-thing");
        assert_eq!(branch_slug("-bad..name.lock"), "bad.name");
        assert_eq!(branch_slug("  "), "");
        for raw in ["Fix 502 Errors!", "feature/ATL-12: thing", "-bad..name.lock", "a//b", "x@{1}"] {
            let b = branch_slug(raw);
            let ok = Command::new("git").args(["check-ref-format", "--branch", &b]).output().unwrap().status.success();
            assert!(ok, "{raw:?} -> {b:?}");
        }
    }

    #[test]
    fn default_base_prefers_remote_and_locals_follow() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let refs = base_refs(dir);
        assert_eq!(refs.first().map(String::as_str), default_base(dir).as_deref());
        assert!(refs.iter().filter(|r| r.rsplit('/').next() == Some("main")).count() <= 1);
    }
}
