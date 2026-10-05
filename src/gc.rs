//! Cleanup: rewrite snapshot history to the current scope, and prune what nothing references.

use anyhow::{Result, bail};
use serde::Serialize;

use crate::config::Matcher;
use crate::snapshot::{Repo, nul_join, pathspec_cmd};

#[derive(Debug, Default, PartialEq, Serialize)]
pub struct Report {
    /// Snapshots before, and how many survive.
    pub snapshots: usize,
    pub kept: usize,
    /// Paths dropped from history because they're no longer in scope, sorted.
    pub removed_paths: Vec<String>,
    pub dry_run: bool,
    /// Whether unreferenced objects were deleted now (driftr-owned stores only).
    pub pruned: bool,
}

impl Repo {
    /// Rewrites the snapshot chain so every snapshot holds only files in the current scope,
    /// dropping snapshots that no longer change anything. Messages, dates and trailers are
    /// kept, and only driftr's own ref and reflog are touched. Unreferenced objects are pruned
    /// immediately in driftr-owned stores; in the user's repo, git's normal expiry applies,
    /// because an immediate prune would also take the user's own unreachable objects.
    pub fn gc(&self, dry_run: bool) -> Result<Report> {
        let _lock = self.lock()?;
        let Some(tip) = self.last()? else {
            return Ok(Report {
                dry_run,
                ..Report::default()
            });
        };
        let matcher = Matcher::new(&self.config()?.scope)?;
        let commits: Vec<String> = self
            .git
            .run(&["rev-list", "--reverse", &tip])?
            .lines()
            .map(String::from)
            .collect();

        let scratch = self.index.with_file_name("gc-index");
        let git = self.git.with_index(&scratch);
        let mut removed = std::collections::BTreeSet::new();
        let mut parent: Option<String> = None;
        let mut parent_tree: Option<String> = None;
        let mut kept = 0;

        for commit in &commits {
            git.run(&["read-tree", commit])?;
            let listed = git.run(&["ls-files", "-z"])?;
            let stale: Vec<&str> = listed
                .split('\0')
                .filter(|p| !p.is_empty() && !matcher.matches(p))
                .collect();
            if !stale.is_empty() {
                git.run_input(
                    &pathspec_cmd(&["rm", "--cached", "-f", "-q"]),
                    &nul_join(&stale),
                )?;
                removed.extend(stale.iter().map(|s| s.to_string()));
            }
            let tree = git.run(&["write-tree"])?;
            if parent_tree.as_ref() == Some(&tree) {
                continue;
            }

            // Same message and dates; only the tree (and so the id) changes.
            let meta =
                self.git
                    .run(&["log", "-1", "--format=%an%n%ae%n%aI%n%cn%n%ce%n%cI", commit])?;
            let [an, ae, ad, cn, ce, cd] = meta.lines().collect::<Vec<_>>()[..] else {
                bail!("unexpected commit metadata for {commit}");
            };
            let message = self.git.run(&["log", "-1", "--format=%B", commit])?;
            let mut args = vec!["commit-tree", "--no-gpg-sign", &tree, "-m", &message];
            if let Some(p) = &parent {
                args.extend(["-p", p]);
            }
            let new = self
                .git
                .with_env("GIT_AUTHOR_NAME", an)
                .with_env("GIT_AUTHOR_EMAIL", ae)
                .with_env("GIT_AUTHOR_DATE", ad)
                .with_env("GIT_COMMITTER_NAME", cn)
                .with_env("GIT_COMMITTER_EMAIL", ce)
                .with_env("GIT_COMMITTER_DATE", cd)
                .run(&args)?;
            parent = Some(new);
            parent_tree = Some(tree);
            kept += 1;
        }
        let _ = std::fs::remove_file(&scratch);

        let report = Report {
            snapshots: commits.len(),
            kept,
            removed_paths: removed.into_iter().collect(),
            dry_run,
            pruned: !dry_run && self.is_external(),
        };
        if dry_run {
            return Ok(report);
        }

        let snapshot_ref = self.snapshot_ref();
        match &parent {
            // Compare-and-swap: a snapshot taken mid-rewrite fails this instead of being lost.
            Some(new_tip) => self.git.run(&[
                "update-ref",
                "-m",
                "driftr: gc --rescope",
                snapshot_ref,
                new_tip,
                &tip,
            ])?,
            None => self.git.run(&["update-ref", "-d", snapshot_ref, &tip])?,
        };
        // Only driftr's reflog: the user's branch reflogs are theirs.
        self.git
            .run(&["reflog", "expire", "--expire=now", snapshot_ref])?;
        if report.pruned {
            self.git.run(&["gc", "-q", "--prune=now"])?;
            self.forget_index();
        }
        Ok(report)
    }
}
