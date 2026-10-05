use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub scope: Scope,
}

/// Which files get snapshotted, matched against repo-relative paths.
/// Patterns are globs (`**/*.md`) or, prefixed with `re:`, regexes. Gitignored files never match.
#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Scope {
    pub include: Vec<String>,
    pub exclude: Vec<String>,
}

impl Default for Scope {
    fn default() -> Scope {
        Scope {
            include: vec!["**/*.md".into()],
            exclude: vec!["**/node_modules/**".into()],
        }
    }
}

impl Config {
    /// Defaults when the file doesn't exist.
    pub fn load(path: &Path) -> Result<Config> {
        if !path.exists() {
            return Ok(Config::default());
        }
        let text = fs::read_to_string(path)?;
        toml::from_str(&text).with_context(|| format!("invalid config: {}", path.display()))
    }

    /// Write-then-rename, so a concurrent reader never sees a half-written config.
    pub fn save(&self, path: &Path) -> Result<()> {
        fs::create_dir_all(path.parent().unwrap())?;
        let tmp = path.with_extension(format!("toml.{}.tmp", std::process::id()));
        fs::write(&tmp, toml::to_string(self)?)?;
        fs::rename(tmp, path)?;
        Ok(())
    }
}

impl Scope {
    /// Literal paths every include lies under, so listing can skip the rest of the tree.
    /// `None` when some include could match anywhere (`**/*.md`, a regex).
    pub fn roots(&self) -> Option<Vec<String>> {
        self.include.iter().map(|p| literal_root(p)).collect()
    }
}

/// The longest literal directory prefix of a glob, or the whole path if it has no glob syntax.
fn literal_root(pattern: &str) -> Option<String> {
    if pattern.starts_with("re:") {
        return None;
    }
    let root = match pattern.find(['*', '?', '[', '{', '\\']) {
        None => pattern,
        Some(i) => pattern[..i].rsplit_once('/')?.0,
    };
    (!root.is_empty()).then(|| root.to_string())
}

pub struct Matcher {
    include: Patterns,
    exclude: Patterns,
    /// Directories whose every file is excluded: the `dir` of each `dir/**` exclude.
    excluded_dirs: Patterns,
}

impl Matcher {
    pub fn new(scope: &Scope) -> Result<Matcher> {
        let dirs: Vec<String> = scope
            .exclude
            .iter()
            .filter_map(|p| p.strip_suffix("/**"))
            .map(String::from)
            .collect();
        Ok(Matcher {
            include: Patterns::new(&scope.include)?,
            exclude: Patterns::new(&scope.exclude)?,
            excluded_dirs: Patterns::new(&dirs)?,
        })
    }

    pub fn matches(&self, path: &str) -> bool {
        self.include.matches(path) && !self.exclude.matches(path)
    }

    /// True when nothing under directory `dir` can match, so a walk can skip it.
    pub fn prunes_dir(&self, dir: &str) -> bool {
        self.excluded_dirs.matches(dir)
    }
}

struct Patterns {
    globs: GlobSet,
    regexes: Vec<Regex>,
}

impl Patterns {
    fn new(patterns: &[String]) -> Result<Patterns> {
        let mut globs = GlobSetBuilder::new();
        let mut regexes = vec![];
        for p in patterns {
            match p.strip_prefix("re:") {
                Some(re) => {
                    regexes.push(Regex::new(re).with_context(|| format!("bad regex: {p}"))?)
                }
                // Gitignore-like: `*` stays within a directory, `**` crosses them.
                None => {
                    globs.add(
                        GlobBuilder::new(p)
                            .literal_separator(true)
                            .build()
                            .with_context(|| format!("bad glob: {p}"))?,
                    );
                }
            }
        }
        Ok(Patterns {
            globs: globs.build()?,
            regexes,
        })
    }

    fn matches(&self, path: &str) -> bool {
        self.globs.is_match(path) || self.regexes.iter().any(|r| r.is_match(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matcher(include: &[&str], exclude: &[&str]) -> Matcher {
        Matcher::new(&Scope {
            include: include.iter().map(|s| s.to_string()).collect(),
            exclude: exclude.iter().map(|s| s.to_string()).collect(),
        })
        .unwrap()
    }

    #[test]
    fn defaults_cover_markdown_anywhere_but_node_modules() {
        let m = Matcher::new(&Scope::default()).unwrap();
        assert!(m.matches("README.md"));
        assert!(m.matches("docs/deep/plan.md"));
        assert!(!m.matches("src/main.rs"));
        assert!(!m.matches("node_modules/pkg/README.md"));
    }

    #[test]
    fn prunes_only_directories_excluded_wholesale() {
        let m = matcher(
            &["**/*.md"],
            &["skills/synced/**", "**/node_modules/**", "draft*"],
        );
        assert!(m.prunes_dir("skills/synced"));
        assert!(m.prunes_dir("a/b/node_modules"));
        assert!(!m.prunes_dir("skills"));
        assert!(
            !m.prunes_dir("drafts"),
            "a file-level exclude never prunes a directory"
        );
    }

    #[test]
    fn single_star_stays_in_one_directory() {
        let m = matcher(&["*.txt"], &[]);
        assert!(m.matches("notes.txt"));
        assert!(!m.matches("docs/notes.txt"));
    }

    #[test]
    fn regex_patterns() {
        let m = matcher(&[r"re:^skills/.*\.(md|toml)$"], &["re:draft"]);
        assert!(m.matches("skills/git/SKILL.md"));
        assert!(m.matches("skills/git/config.toml"));
        assert!(!m.matches("skills/git/draft.md"));
        assert!(!m.matches("other/SKILL.md"));
    }

    #[test]
    fn roots_are_the_literal_prefixes_of_every_include() {
        let scope = |include: &[&str]| Scope {
            include: include.iter().map(|s| s.to_string()).collect(),
            exclude: vec![],
        };
        assert_eq!(
            scope(&["CLAUDE.md", "skills/**", "docs/a/*.md"]).roots(),
            Some(vec!["CLAUDE.md".into(), "skills".into(), "docs/a".into()])
        );
        assert_eq!(scope(&["skills/**", "**/*.md"]).roots(), None);
        assert_eq!(scope(&["*.txt"]).roots(), None);
        assert_eq!(scope(&["re:^docs/"]).roots(), None);
        assert_eq!(scope(&[r"weird\[1\].txt"]).roots(), None);
    }

    #[test]
    fn rejects_bad_patterns() {
        let scope = Scope {
            include: vec!["re:(".into()],
            exclude: vec![],
        };
        assert!(Matcher::new(&scope).is_err());
    }
}
