use super::{Class, Command, Input, Target};
use crate::context::Captured;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Default)]
pub struct CommandFile {
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub commands: Vec<CommandDef>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct CommandDef {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub run: Option<String>,
    #[serde(default, rename = "in")]
    pub target: Option<Target>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub keep_open: bool,
    #[serde(default)]
    pub confirm: bool,
    #[serde(default)]
    pub inputs: Vec<Input>,
    #[serde(default)]
    pub disabled: bool,
}

pub struct Loaded {
    pub commands: Vec<Command>,
    pub errors: Vec<String>,
    pub files: Vec<PathBuf>,
}

pub fn load(dir: &Path, ctx: &Captured) -> Loaded {
    let mut out = Loaded { commands: vec![], errors: vec![], files: vec![] };
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    let mut paths: Vec<PathBuf> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .collect();
    paths.sort();
    for path in paths {
        out.files.push(path.clone());
        match parse(&path) {
            Ok(file) => {
                let Some(depth) = scope_depth(file.scope.as_deref(), ctx) else { continue };
                for def in file.commands {
                    out.commands.push(to_command(def, &path, depth));
                }
            }
            Err(e) => out.errors.push(format!("{}: {e:#}", path.display())),
        }
    }
    out
}

fn parse(path: &Path) -> Result<CommandFile> {
    let s = std::fs::read_to_string(path).with_context(|| "read")?;
    toml::from_str(&s).with_context(|| "parse")
}

fn to_command(def: CommandDef, path: &Path, depth: i32) -> Command {
    Command {
        title: def.title.unwrap_or_else(|| def.id.clone()),
        id: def.id,
        description: def.description,
        keywords: def.keywords,
        class: Class::User { file: path.to_path_buf(), scoped: depth > 0 },
        run: def.run.unwrap_or_default(),
        target: def.target.unwrap_or_default(),
        cwd: def.cwd,
        label: def.label,
        keep_open: def.keep_open,
        confirm: def.confirm,
        inputs: def.inputs,
        disabled: def.disabled,
        depth,
        source: path.display().to_string(),
    }
}

/// None when the scope does not cover the context. Global files have depth 0.
pub fn scope_depth(scope: Option<&str>, ctx: &Captured) -> Option<i32> {
    let Some(scope) = scope.map(str::trim).filter(|s| !s.is_empty()) else { return Some(0) };
    let scope = expand_home(scope);
    let scope = Path::new(&scope);
    let inside = ctx.dir.starts_with(scope) || ctx.alt_dir.as_ref().is_some_and(|d| d.starts_with(scope));
    inside.then(|| scope.components().count() as i32)
}

pub fn expand_home(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return format!("{}/{rest}", home.to_string_lossy());
        }
    }
    p.to_string()
}

pub fn sanitize_name(name: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in name.trim().chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            out.push(c);
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() { "commands".into() } else { out }
}

pub fn template(name: &str, scope: &Path) -> String {
    format!(
        "# Commands for {scope}\nscope = \"{scope}\"\n\n[[commands]]\nid = \"{name}\"\ntitle = \"{title}\"\nrun = \"echo hello from {name}\"\nin = \"split\"          # background | current-pane | split | tab | workspace | popup\n\n# [[commands.inputs]]\n# name = \"arg\"\n# prompt = \"Argument\"\n# kind = \"text\"        # text | choice | workspace | tab | pane | agent\n",
        scope = scope.display(),
        title = name.replace('-', " "),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(dir: &str, alt: Option<&str>) -> Captured {
        Captured { dir: PathBuf::from(dir), alt_dir: alt.map(PathBuf::from), ..Default::default() }
    }

    #[test]
    fn global_has_depth_zero() {
        assert_eq!(scope_depth(None, &ctx("/a/b", None)), Some(0));
        assert_eq!(scope_depth(Some(""), &ctx("/a/b", None)), Some(0));
    }

    #[test]
    fn scope_matches_inside_only() {
        assert_eq!(scope_depth(Some("/a"), &ctx("/a/b", None)), Some(2));
        assert_eq!(scope_depth(Some("/a/b/c"), &ctx("/a/b", None)), None);
        assert_eq!(scope_depth(Some("/ab"), &ctx("/a/b", None)), None);
    }

    #[test]
    fn scope_matches_worktree_translation() {
        let c = ctx("/home/x/.herdr/worktrees/main/feat/infra", Some("/home/x/repo/infra"));
        assert!(scope_depth(Some("/home/x/repo"), &c).is_some());
    }

    #[test]
    fn sanitizes_names() {
        assert_eq!(sanitize_name("Terraform / Plan!"), "terraform-plan");
        assert_eq!(sanitize_name("  "), "commands");
    }

    #[test]
    fn parses_example_files() {
        for f in ["global.toml", "scoped.toml"] {
            let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/commands").join(f);
            parse(&p).unwrap_or_else(|e| panic!("{f}: {e:#}"));
        }
    }
}
