# The hybrid pass table moves into the core

**Status:** proposed, 2026-11-19. Revised after an independent review (§8). Awaiting the operator's review of §5.
**Established:** 2026-11-19
**Follows:** `docs/plans/impl/hybrid.md` B15 (kept for later when `vizzle hybrid` shipped in 0.15.0).

## 1. Why

`vizzle hybrid` builds architecture pictures from hand-written specs (`docs/diagram-types/hybrid.md`). Each spec
names the **passes** it reconciles against: `vizzle component` runs whose module graphs the picture is checked
against. The passes are defined in a small file beside the specs, `vizzle-passes.json`, the **pass table**:

```json
{ "passes": { "self": { "args": ["-I", "crates/**", "-I", "packages/**"] } } }
```

Almost all of `hybrid` lives in the Rust core: reading the spec, reconciling it, layout, routing, lint, SVG. Two rules
about passes live in the Python CLI instead (`packages/vizzle-cli/src/vizzle_cli/cli.py`):

| Rule | Where it is today | Also in the core? |
|---|---|---|
| The pass table's format: an object with `passes` (name → `{"args": [strings]}`), `$comment` and the old `vizzle` pin allowed, any other field refused, the pin accepted with a note | `_hybrid_passes` (`cli.py:980`) | No |
| Which passes a spec needs: its `vizzle` field, each name defined in the table | `hybrid_command` reads it with its own `json.loads` (`cli.py:1102`); a `vizzle` that is not a list is skipped silently there | **Yes.** `hybrid::build` parses the same field (`spec.rs:65`) and refuses an undefined pass (`mod.rs:60`) with different wording |

vizzle's `CLAUDE.md` asks for each rule to live once. The second row is a real duplicate: two parsers of one field,
with two error messages for one mistake. The first row is a format rule in Python while every other hybrid format
rule (the spec's closed enums, unknown fields refused, `$comment` allowed: `hybrid.md` B5–B8) is in Rust.

## 2. What stays in the CLI, and why

A pass's **arguments** keep going through Click's `component` parser (`_run_hybrid_pass`), and the CLI keeps refusing
drawing options (`_DRAW_ONLY`, `hybrid.md` B2). Click is the one definition of what `vizzle component` accepts;
re-implementing it in Rust would make a second one. The CLI also keeps all file I/O: finding the spec directory,
reading files, answering `exists`. The core takes text, as `hybrid_svg` already does.

So the line is: **the core owns what the files mean; the CLI owns the filesystem and Click.**

## 3. Design

Two functions in `crates/vizzle-core/src/hybrid/passes.rs`, re-exported through `lib.rs` and bound in
`crates/vizzle-py/src/lib.rs`:

```rust
/// A pass table, read. `notes` are warnings the CLI prints (today: the ignored version pin).
pub struct PassTable {
    pub passes: BTreeMap<String, Vec<String>>,
    pub notes: Vec<String>,
}

/// Parse `vizzle-passes.json`'s text. An error names the first problem (§5 D2 for wording).
pub fn pass_table(json: &str) -> Result<PassTable>;

/// The passes a spec reconciles against, in the order it names them, each checked
/// against the table. A concept spec (`"vizzle": []`) gives an empty list; a spec with
/// no `vizzle` field is refused by the spec parser, as `build` refuses it today.
pub fn spec_passes(spec_json: &str, table: &PassTable) -> Result<Vec<String>>;
```

How the table is read:

- The top level is a struct with `deny_unknown_fields`: `passes` optional (`#[serde(default)]`, so `{}` is an empty
  table, as `table.get("passes", {})` allows today), `$comment` as `#[serde(rename = "$comment", default)]` like
  `Spec`'s, and the pin as `vizzle: Option<serde_json::Value>`, since the table accepts any JSON there today.
- `passes` is read by a hand-written map visitor, modelled on `ordered_map` (`spec.rs:357`) but refusing a repeated
  key (D4); `ordered_map` keeps duplicates on purpose, so it is copied, not reused. Each value is deserialized as an
  `Entry { args: Vec<String>, $comment }` with `deny_unknown_fields`, and any error there is replaced by
  `pass "<name>" must be {"args": [strings]}`, the wording the CLI uses today (D2). Without the wrapper serde would
  say `` unknown field `argz`, expected `args` `` with no pass name.
- A repeated field inside a struct (two `passes`, two `args`) is already refused by serde ("duplicate field"); a test
  pins it.
- The pin's note: `pins vizzle <v>; the pin is ignored (the passes run in this vizzle) and will be refused in a later
  minor — drop it`, where `<v>` is the string itself when the pin is a string (so `0.13.0`, not `"0.13.0"`) and its
  JSON otherwise.

`spec_passes` reads the field with `spec::parse`, the parser `build` uses, so there is one reader of `vizzle`. An
undefined pass gives `spec names vizzle pass "<name>", which the pass table does not define`, the string `build`
already uses (`mod.rs:61`). The core does not name the file: `vizzle-passes.json` is a CLI constant
(`HYBRID_PASSES`). `build` keeps its check, with that same string, for callers that skip `spec_passes`.

Bindings:

```python
_core.hybrid_pass_table(text: str) -> tuple[dict[str, list[str]], list[str]]   # passes, notes
_core.hybrid_spec_passes(spec_text: str, table_text: str | None) -> list[str]
```

`hybrid_spec_passes` takes the table's text rather than a parsed object, so Python never holds the table's rules.
Parsing the table once per spec is a few microseconds on h's eight specs. Both raise `ValueError` through
`to_py_err` (`vizzle-py/src/lib.rs:25`). The CLI calls `hybrid_pass_table` first, so a bad table stops there; a
`ValueError` from `hybrid_spec_passes` can then only be about the spec.

The CLI after the change:

```python
table_path = spec_dir / HYBRID_PASSES
table_text = _read_input(table_path, "pass table") if table_path.exists() else None
try:
    passes, notes = _core.hybrid_pass_table(table_text) if table_text is not None else ({}, [])
except ValueError as err:
    raise _CouldNotRun(f"{table_path}: {err}") from err
for note in notes:
    click.echo(f"note: {table_path} {note}", err=True)
...
try:
    wanted = _core.hybrid_spec_passes(text, table_text)   # replaces json.loads(text).get("vizzle", [])
except ValueError as err:
    raise _CouldNotRun(f"[{name}] {err}") from err
```

`_hybrid_passes` is deleted. `_run_hybrid_pass`'s own messages (a Click error, a drawing option) name the pass as
`pass "{name}"` (an f-string with literal double quotes; `!r` would give single quotes), matching the core.

## 4. Behaviour changes

Nothing a correct setup would notice. h's eight pictures stay byte-identical (§6 A5). The visible changes:

- **Error wording.** The pass name is quoted `"ws"` everywhere (D2). Two messages change words, not only quotes:
  an undefined pass now says `the pass table does not define` (was `vizzle-passes.json does not define`), and an
  unknown top-level field is serde's `` unknown field `passez` `` with the expected fields listed (was
  `unknown field 'passez'`).
- **The whole spec is checked before any pass runs.** `spec::parse` also runs `validate` (duplicate node ids,
  unknown refs; `spec.rs:380-392`). A spec with such a mistake used to run its passes and then stop in `build`; now it
  stops first, which is faster and gives the same exit code (2). The message `<spec> is not a JSON object: …`
  (`cli.py:1104`) is replaced by the spec parser's `[name] not a hybrid spec: …`, the one `build` gives today.
- **A spec whose `vizzle` is not a list, or that has none,** is refused by the spec parser up front. Today the CLI
  skips it and `build` refuses it a moment later, so only the order changes.
- **New: a pass name given twice** in the table is refused (D4). Today the last one silently wins. This is the one
  addition in an otherwise move-only change.

## 5. Decisions

| # | Question | Recommendation | Why |
|---|---|---|---|
| D1 | One call for "the whole plan" or two calls? | Two: `pass_table` and `spec_passes` | The CLI's loop stays as it is: one spec at a time, its errors prefixed with the spec name, passes run lazily the first time a spec needs them. One call would need every spec read up front and would move the loop into Rust for no gain |
| D2 | Whose error wording? | The core's, with Rust's `{:?}` quoting (`"ws"`), as §3 gives each string. The table's entry rule keeps today's `pass "<name>" must be {"args": [strings]}` through a wrapper; serde's own wording stands where it names the problem well (unknown or duplicate fields). The CLI puts the path or `[name]` in front, and its own pass messages use the same quoting | One message per mistake, written where the rule lives, and one pass named the same way whichever check stops it |
| D3 | The old `"vizzle": "<version>"` pin | Unchanged: accepted, ignored, a note, printed as today (§3). The core produces the note's text; the CLI prints it | Refusing it is a separate decision (`docs/diagram-types/hybrid.md` §2.1 says "a later minor"). h's table still carries the pin, so refusing it now would break h's build |
| D4 | A pass name given twice in the table | Refuse, exit 2, naming it. New behaviour | Silent last-one-wins is the kind of mistake B6 already refuses for nodes. It costs one map visitor, which the entry wrapper (D2) needs anyway. h's table has one pass, so nothing breaks |
| D5 | Release | No release of its own; it ships with the next minor | Only error wording and D4 change. The bump PR's notes mention both |

## 6. Acceptance

| # | Check |
|---|---|
| A1 | `_hybrid_passes` is gone, and between the `# ─── hybrid` banner and `def _repo_root` in `cli.py`, `grep -n 'json\.loads\|"passes"\|"args"\|"\$comment"\|"vizzle"'` finds nothing (output shown in the PR) |
| A2 | Core unit tests, one per rule. For `pass_table`: not an object; `passes` not an object; an unknown top-level field; an entry that is not an object; `args` missing, not a list, or holding a non-string; an extra field in an entry (each of these four gives the `must be` message with the pass name); a duplicate pass name; a duplicate `passes` and a duplicate `args`; `$comment` allowed at both levels; `{}` is an empty table; a string pin gives exactly one note with the bare version, a non-string pin gives its JSON. For `spec_passes`: an undefined pass; a `vizzle` that is not a list; a missing `vizzle`; a concept spec gives `[]`; the order the spec names them is kept. Each refusal test mutation-checked (break the rule and the test fails) |
| A3 | `test_hybrid.py` passes with only these edits: the five strings in `test_a_bad_pass_table_cannot_run` updated to the §4 wording, and one new case for D4. `test_the_old_version_pin_is_ignored_with_a_note` passes unchanged |
| A4 | `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check`, `uv run pytest`, `uv run ruff check`, the plugin skill check: each printed with its own `rc=` |
| A5 | h's hybrid pictures: `vizzle hybrid --check --root <h main checkout>` with the local build reports all eight `✓` (the GENERATED line names the local version; compare with the version line masked, as for the 0.15.0 port) |
| A6 | `docs/plans/impl/hybrid.md` B15 says it was done here; `docs/diagram-types/hybrid.md` §2.1 says the core reads the table and refuses a repeated pass name (no other change to the format); this spec's status is set to built, with a results section |

## 7. Out of scope

- Which `component` options a pass may carry (B2). Stays with Click.
- Refusing the version pin (D3).
- h's pass table: its `$comment` still names `scripts/hybrid-diagrams.mjs`, the script `vizzle hybrid` replaced, and it
  still pins 0.13.0. Both are h's to tidy; this change works with the file as it is.

## 8. Review

An independent reviewer checked the first draft against the code: the split, the API and the line references held.
Folded in: the entry message needs a wrapper to keep the pass name (serde's would drop it); the pin note must print a
string pin bare; a spec with no `vizzle` is refused, not empty; the whole spec is now validated before passes run;
the undefined-pass string is `build`'s, since the core does not know the file name; D4's visitor is a copy of
`ordered_map`, not a reuse, and is marked as the one new behaviour; Python needs literal `"` for the quoting; A1's
grep is now bounded to the hybrid section.
