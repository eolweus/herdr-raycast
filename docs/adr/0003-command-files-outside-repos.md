---
status: accepted
---

# Command Files live outside repositories and apply by directory Scope

The obvious design is a committed `.herdr/` file in each repo, as herdr-plus does. We keep every Command File in the plugin's config directory (`commands/*.toml`) and let each file declare a `scope` path; a Command is visible when the Context Directory, or its worktree-to-repo-root translation, lies inside that Scope. This keeps experimental Commands out of shared repos while the plugin is unproven, lets one repo carry several Scopes (a `terraform/` subfolder can have its own file), and works for directories that are not git repositories at all. The narrowest Scope wins on id collision, and `disabled = true` hides a Command of any class.

## Considered Options

- Committed `.herdr/<plugin>.toml` per repo: visible to teammates and travels with worktrees, but publishes half-finished Commands and cannot scope below the repo root without a second convention.
- Both, layered: rejected for v1 to avoid defining merge semantics across two sources before the format has settled. Can be added later as one more Scope source.

## Consequences

- Command Files are per machine. Installing on another host means copying or recreating them.
- Worktree checkouts under `~/.herdr/worktrees/` must be translated to the repo root before Scope matching, using the worktree info Herdr attaches to the workspace.
