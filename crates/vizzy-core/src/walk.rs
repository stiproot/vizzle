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

/// Collect `(relative_path, contents)` for supported source files under `root`,
/// respecting .gitignore. `include`/`exclude` are glob patterns matched against
/// the relative path; `langs` (empty = all) restricts languages.
pub fn collect_files(
    root: &Path,
    include: &[String],
    exclude: &[String],
    langs: &[Language],
) -> Result<Vec<(String, String)>> {
    let include = build_globset(include)?;
    let exclude = build_globset(exclude)?;

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
        let Some(lang) = Language::from_path(&rel) else {
            continue;
        };
        if !langs.is_empty() && !langs.contains(&lang) {
            continue;
        }
        if let Some(include) = &include {
            if !include.is_match(&rel) {
                continue;
            }
        }
        if let Some(exclude) = &exclude {
            if exclude.is_match(&rel) {
                continue;
            }
        }
        match std::fs::read_to_string(entry.path()) {
            Ok(contents) => files.push((rel, contents)),
            Err(_) => continue, // non-UTF8 or unreadable: skip
        }
    }
    files.sort();
    Ok(files)
}
