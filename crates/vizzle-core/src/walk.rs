//! Gitignore-aware source file discovery.

use std::path::Path;

use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;

use crate::model::Language;

fn build_globset(patterns: &[String]) -> Result<Option<GlobSet>> {
    if patterns.is_empty() {
        return Ok(None);
    }
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(Glob::new(pattern).with_context(|| format!("invalid glob `{pattern}`"))?);
    }
    Ok(Some(builder.build()?))
}

/// Collect `(relative_path, contents)` for package manifests under `root`,
/// respecting .gitignore (so vendored trees like node_modules stay out).
pub fn collect_manifests(root: &Path) -> Result<Vec<(String, String)>> {
    let mut manifests = Vec::new();
    for entry in WalkBuilder::new(root).hidden(true).build() {
        let entry = entry?;
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let Some(name) = entry.path().file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !crate::component::MANIFEST_NAMES.contains(&name) {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(root)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .replace('\\', "/");
        match std::fs::read_to_string(entry.path()) {
            Ok(contents) => manifests.push((rel, contents)),
            Err(_) => continue,
        }
    }
    manifests.sort();
    Ok(manifests)
}

/// The file-selection rules shared by every entry point: language, then
/// include globs, then exclude globs. One matcher rather than one per entry
/// point, so a glob means the same thing on `component`, `class`, `doc` and
/// `diff` — the diff learns nothing about globs of its own.
///
/// Sharing the matcher is necessary but not sufficient for that promise: the
/// commands do not all hand it the same kind of path. See [`Self::scope`].
pub struct Selector {
    include: Option<GlobSet>,
    exclude: Option<GlobSet>,
    langs: Vec<Language>,
    /// A prefix under which a path is ALSO matched with the prefix removed.
    ///
    /// Matching the repo-relative path alone is not enough to make a glob mean
    /// one thing everywhere, because the commands do not all see repo-relative
    /// paths. A walk is rooted at the path the user names, so
    /// `component harness/kikimora` yields `tests/...`. A component diff cannot
    /// be rooted there — an edge's existence depends on files the change never
    /// touched, so it collects the whole repository and yields
    /// `harness/kikimora/tests/...`. Without this, `-E 'tests/**'` filters on
    /// `component` and silently does nothing on `diff`.
    scope: Option<String>,
}

impl Selector {
    pub fn new(include: &[String], exclude: &[String], langs: &[Language]) -> Result<Self> {
        Ok(Self {
            include: build_globset(include)?,
            exclude: build_globset(exclude)?,
            langs: langs.to_vec(),
            scope: None,
        })
    }

    /// Also match paths relative to `scope`, so a glob written for
    /// `component <scope>` means the same on `diff <scope>`.
    pub fn within_scope(mut self, scope: &str) -> Self {
        let trimmed = scope.trim_matches('/');
        self.scope = (!trimmed.is_empty()).then(|| format!("{trimmed}/"));
        self
    }

    /// Every spelling of `rel` a user's glob may reasonably be written against.
    fn spellings<'a>(&'a self, rel: &'a str) -> impl Iterator<Item = &'a str> {
        let scoped = self
            .scope
            .as_deref()
            .and_then(|prefix| rel.strip_prefix(prefix));
        std::iter::once(rel).chain(scoped)
    }

    /// True when `rel` is a supported source file the rules let through.
    pub fn accepts(&self, rel: &str) -> bool {
        let Some(lang) = Language::from_path(rel) else {
            return false;
        };
        if !self.langs.is_empty() && !self.langs.contains(&lang) {
            return false;
        }
        if let Some(include) = &self.include {
            if !self.spellings(rel).any(|p| include.is_match(p)) {
                return false;
            }
        }
        if let Some(exclude) = &self.exclude {
            // Excluded under any spelling means excluded: someone who asked not
            // to see a tree should not have to work out which base we matched.
            if self.spellings(rel).any(|p| exclude.is_match(p)) {
                return false;
            }
        }
        true
    }

    /// Keep the `(relative_path, contents)` pairs the rules let through.
    pub fn filter(&self, files: &[(String, String)]) -> Vec<(String, String)> {
        files
            .iter()
            .filter(|(rel, _)| self.accepts(rel))
            .cloned()
            .collect()
    }
}

/// Collect `(relative_path, contents)` for supported source files under `root`,
/// respecting .gitignore. `include`/`exclude` are glob patterns matched against
/// the relative path; `langs` (empty = all) restricts languages.
pub fn collect_files(
    root: &Path,
    include: &[String],
    exclude: &[String],
    langs: &[Language],
) -> Result<Vec<(String, String)>> {
    let selector = Selector::new(include, exclude, langs)?;

    let mut files = Vec::new();
    for entry in WalkBuilder::new(root).hidden(true).build() {
        let entry = entry?;
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(root)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .replace('\\', "/");
        if !selector.accepts(&rel) {
            continue;
        }
        match std::fs::read_to_string(entry.path()) {
            Ok(contents) => files.push((rel, contents)),
            Err(_) => continue, // non-UTF8 or unreadable: skip
        }
    }
    files.sort();
    Ok(files)
}

#[cfg(test)]
mod scope_tests {
    use super::*;

    fn excluding(pattern: &str) -> Selector {
        Selector::new(&[], &[pattern.to_owned()], &[]).unwrap()
    }

    #[test]
    fn a_scope_relative_glob_filters_a_repo_relative_path() {
        // The acceptance criterion: the glob a user writes for
        // `component harness/kikimora` must mean the same on
        // `diff harness/kikimora`, whose paths are repo-relative.
        let s = excluding("tests/fixtures/**").within_scope("harness/kikimora");
        assert!(!s.accepts("harness/kikimora/tests/fixtures/repo/a.py"));
        assert!(s.accepts("harness/kikimora/src/a.py"));
    }

    #[test]
    fn the_repo_relative_spelling_still_works() {
        let s = excluding("harness/kikimora/tests/**").within_scope("harness/kikimora");
        assert!(!s.accepts("harness/kikimora/tests/fixtures/repo/a.py"));
    }

    #[test]
    fn a_scope_relative_glob_does_not_reach_outside_the_scope() {
        // Stripping the prefix must not turn a scoped glob into a loose one.
        let s = excluding("tests/fixtures/**").within_scope("harness/kikimora");
        assert!(s.accepts("other/tests/fixtures/repo/a.py"));
    }

    #[test]
    fn without_a_scope_only_the_repo_relative_spelling_matches() {
        let s = excluding("tests/fixtures/**").within_scope("");
        assert!(s.accepts("harness/kikimora/tests/fixtures/repo/a.py"));
        assert!(!s.accepts("tests/fixtures/repo/a.py"));
    }

    #[test]
    fn an_include_glob_gets_the_same_two_spellings() {
        let s = Selector::new(&["src/**".to_owned()], &[], &[])
            .unwrap()
            .within_scope("harness/kikimora");
        assert!(s.accepts("harness/kikimora/src/a.py"));
        assert!(!s.accepts("harness/kikimora/tests/a.py"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(items: &[&str]) -> Vec<(String, String)> {
        items
            .iter()
            .map(|p| ((*p).to_owned(), String::new()))
            .collect()
    }

    fn paths(files: &[(String, String)]) -> Vec<&str> {
        files.iter().map(|(p, _)| p.as_str()).collect()
    }

    #[test]
    fn selector_applies_lang_then_include_then_exclude() {
        let files = pairs(&[
            "apps/svc/main.ts",
            "packages/core/index.ts",
            "packages/core/tests/fixtures/repo/index.ts",
            "tools/script.py",
            "README.md",
        ]);
        let all = Selector::new(&[], &[], &[]).unwrap();
        assert_eq!(
            paths(&all.filter(&files)),
            [
                "apps/svc/main.ts",
                "packages/core/index.ts",
                "packages/core/tests/fixtures/repo/index.ts",
                "tools/script.py",
            ],
            "unsupported files drop even with no rules"
        );

        let excluded = Selector::new(&[], &["**/tests/fixtures/**".to_owned()], &[]).unwrap();
        assert_eq!(
            paths(&excluded.filter(&files)),
            [
                "apps/svc/main.ts",
                "packages/core/index.ts",
                "tools/script.py"
            ]
        );

        let included = Selector::new(&["packages/**".to_owned()], &[], &[]).unwrap();
        assert_eq!(
            paths(&included.filter(&files)),
            [
                "packages/core/index.ts",
                "packages/core/tests/fixtures/repo/index.ts"
            ]
        );

        let python = Selector::new(&[], &[], &[Language::Python]).unwrap();
        assert_eq!(paths(&python.filter(&files)), ["tools/script.py"]);
    }

    #[test]
    fn selector_rejects_an_invalid_glob() {
        assert!(Selector::new(&["[".to_owned()], &[], &[]).is_err());
    }
}
