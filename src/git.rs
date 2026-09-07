use std::path::Path;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_base_prefers_remote_and_locals_follow() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let refs = base_refs(dir);
        assert_eq!(refs.first().map(String::as_str), default_base(dir).as_deref());
        assert!(refs.iter().filter(|r| r.rsplit('/').next() == Some("main")).count() <= 1);
    }
}
