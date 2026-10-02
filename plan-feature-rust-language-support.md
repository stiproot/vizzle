# Rust Language Support — Implementation Plan

## Context

vizzle parses only Python and TypeScript; its own Rust crates are invisible to `vizzle component .`. The `docs/languages/rust.md` spec (§§1–9, Status: proposed) was reviewed and approved. One pre-work item: `tree-sitter` was bumped to `0.27` by dependabot #47, so the spec's `tree-sitter-rust = "0.24"` guidance (written for 0.26) must be re-evaluated first.

## Step 0 — Resolve tree-sitter-rust version (spec §9 trigger)

Try `cargo add tree-sitter-rust@0.24 -p vizzle-core`. If resolution fails (version requires tree-sitter `^0.26`), try `@0.23` then the latest `0.x`. Confirm with a minimal compile test:
```rust
#[test] fn grammar_loads() {
    let mut p = tree_sitter::Parser::new();
    p.set_language(&tree_sitter_rust::LANGUAGE.into()).unwrap();
    assert!(p.parse("fn main() {}", None).is_some());
}
```
Update `docs/languages/rust.md` §9 with the version chosen and this evidence.

## Files to create or modify

| File | Change |
|---|---|
| `crates/vizzle-core/Cargo.toml` | add `tree-sitter-rust = "<resolved>"` |
| `crates/vizzle-core/src/model.rs` | add `Rust` to `Language`; update `from_path` (`.rs`) and `name()` |
| `crates/vizzle-core/src/parse/rust.rs` | **new** — full Rust tree-sitter parser |
| `crates/vizzle-core/src/parse/mod.rs` | add `mod rust;` + `Language::Rust` match arm |
| `crates/vizzle-core/src/lib.rs` | add `"rust" | "rs"` to `SelectOptions::languages()` |
| `crates/vizzle-core/src/component.rs` | Rust import + Cargo path-dep edge building (§7) |
| `packages/vizzle-cli/src/vizzle_cli/cli.py` | add `"rust"` to `click.Choice` on `-l` |
| `plugins/vizzle/skills/vizzle-diagrams/SKILL.md` | document `-l rust` |
| `README.md` | update language list to include Rust |
| `docs/languages/rust.md` | `Status: v1 implemented`; update §9 with version + evidence |

## Step 1 — Language plumbing (model.rs, lib.rs, cli.py)

Add `Rust` variant. `from_path`: `"rs" => Some(Language::Rust)`. `target/` is already excluded via gitignore. `SelectOptions::languages()`: add `"rust" | "rs"` arm. CLI: add `"rust"` to `click.Choice`.

## Step 2 — Write failing tests first

Before implementing the parser, write these unit tests (must FAIL on base tree) and record failure output in `demonstrations/`:

1. `test_language_enum_box` — parse a fixture with the `Language` enum; assert `vizzle_core::model::Language` + `<<enumeration>>`
2. `test_language_merge` — assert `from_path` and `name` members appear in Language box (impl merging)  
3. `test_struct_count_names` — parse the live repo; assert each of the 26 named structs and 9 named enums appears exactly once by name
4. `test_no_trait_box` — assert zero `<<interface>>` boxes in this repo's output
5. `test_realization_edge` — fixture with `impl Tr for T`; assert `..|>` edge
6. `test_no_derive_realization` — fixture with `#[derive(Serialize)] struct T`; assert no `..|> Serializer` invented

Also a CLI integration test: `vizzle class . -l rust` produces the named boxes, and `vizzle component . -l rust` shows `vizzle-py → vizzle-core`.

## Step 3 — parse/rust.rs

**Qualified-name helper:** Formats type paths to crate-scoped form.

**Two-pass parse:**
- Pass 1 — collect type declarations: `struct_item`, `enum_item`, `union_item`, `trait_item`, `type_alias_item`
- Pass 2 — impl merging and realization edges

**Member extraction:** self receiver → instance; no receiver → static. Visibility: `pub` → `+`, `pub(crate|super|in ...)` → `#`, private → `-`.

**Relations:** field types → Association; param/return types → Dependency. Type-wrapper traversal (§4).

## Step 4 — component.rs: Rust edges (§7)

**Import-based:** add `Language::Rust` arm in `build_edges`

**Cargo path-dep edges:** add `build_cargo_path_dep_edges` for `[dependencies.*]` entries with `path = "..."`

## Step 5 — Docs

- `docs/languages/rust.md`: `Status: v1 implemented`; update §9
- `SKILL.md`: add `-l rust` documentation  
- `README.md`: update language list

## Verification gate

```sh
uv sync --reinstall-package vizzle && cargo test -p vizzle-core \
  && uv run pytest packages/vizzle-cli/tests && uv run pre-commit run --all-files
```

Then §10 acceptance commands with text assertions on box names and edge structure.
