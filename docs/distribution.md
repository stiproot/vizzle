# Distribution

**Status:** decided, not yet implemented. §3 (the name) is done; §4–§6 are the
build order. §8 is a standing rejection with trigger conditions; §10 is deferred.
**Measured on:** 2026-08-16, against `~/code/h` (358 parsed source files) on Linux.

The spec for how vizzle reaches the repos that use it. Diagram-type specs answer
*"what does this diagram mean?"*; this one answers *"how does a stranger's repo
get a diagram out of vizzle, and what does that cost them?"*

## 1. What vizzle is, distribution-wise

vizzle is a **tool**, not a library. Nothing imports it; you run it and read the
output. That single fact drives everything below.

The corollary is the binding constraint:

> **vizzle must not enter a consuming repo's dependency graph.** The repos that
> benefit are TypeScript, Python, Rust, and mixed. A TypeScript repo should not
> acquire a Python dependency — or a `requirements.txt`, or a virtualenv — in
> order to look at a picture of itself.

This rules out the mode that would otherwise be the obvious default: "add vizzle
to your dev-dependencies." That mode is available to Python repos and is fine
for them, but it cannot be the canonical answer, because it is unavailable to
most of the audience.

What survives the constraint: run it without installing it (§2.1), install it
once per machine (§2.2), or never install it at all and let CI run it (§2.3).

## 2. The consumption ladder

Ranked by how much of the audience each reaches, not by how clever it is. Each
rung serves a named reader.

### 2.1 Ad-hoc, zero install — the primary mode

```sh
uvx vizzle component ~/code/some-repo
uvx vizzle class . -o classes.html
```

**Reader:** someone facing unfamiliar code who wants a diagram *now*.

This is the mode the whole product promise rests on — "as fast and as easy as
possible" means the distance between wanting a diagram and having one should be
one command with no preamble. `uvx` (and `pipx run`) downloads, caches, and runs
in one step, into a throwaway environment that never touches the target repo.

This mode is the reason §3 mattered enough to rename the project.

### 2.2 Installed once — the daily driver

```sh
uv tool install vizzle
vizzle serve ~/code/some-repo --diff --open
```

**Reader:** someone who uses vizzle habitually, especially `vizzle serve`, where
a long-lived process makes the per-invocation cost of `uvx` pointless.

### 2.3 CI, on a pull request — the widest reach

**Reader:** the *second* audience, the one asking "what did this change do?" —
and the only rung that reaches readers who will never install anything.

`vizzle diff --type component --base <base-sha>` on a PR, with the Mermaid
posted as a comment and the HTML uploaded as a build artifact. GitHub renders
` ```mermaid ` fenced blocks natively in comments and in Markdown files, so the
diagram appears inline with **no image hosting, no asset pipeline, and no
external service**. That property is worth protecting in the Mermaid renderer:
it is what makes this rung nearly free.

### 2.4 Committed diagrams, checked for drift

A repo commits `docs/architecture.mmd`, and CI regenerates it and fails if it
differs. Turns a diagram into an artifact that cannot silently rot.

Needs a `--check` mode that does not exist yet (§10).

### 2.5 pre-commit

Lowest rung deliberately: see §10 for why it needs a mirror repo rather than a
hook pointed at this one, and why it is worth least of the five.

## 3. Decision: the distribution name is `vizzle`

**Decided 2026-08-16. Implemented in `40fa209`.**

The project was called `vizzy` until the day distribution was first designed.

**What was measured.** Registry availability, checked directly against
pypi.org, crates.io and registry.npmjs.org:

| Name | PyPI | crates.io | npm |
| --- | --- | --- | --- |
| `vizzy` | **taken** | free | free |
| `vizzle` | free | free | free |
| `vizzly` | free | free | **taken** |
| `vizzum`, `vizzard`, `vizzo`, `vizza`, `vizzr`, `vizzit`, `squizz` | free | free | free |
| `squiz`, `limn`, `codeviz`, `shapeviz` | **taken** | mixed | mixed |

`vizzy` on PyPI is *"Useful tools to visualize NLP data"*, v0.3.0, last released
February 2023 — unrelated, effectively abandoned, and occupied.

**What was chosen, and why.** `vizzle`, applied to the distribution *and* the
command, so they match.

The occupied name breaks §2.1 specifically and severely: `uvx vizzy` would have
installed and run a stranger's package. Not failed — *run*, with a confusing
error at best. The available workaround, `uvx --from vizzy-uml vizzy`, is
precisely the friction the tool exists to remove.

Renaming the distribution while keeping the command `vizzy` was rejected for the
same reason: it fixes `uv tool install` but leaves the primary mode broken.

Reclaiming the name via PEP 541 was rejected as a *blocker* — it takes months,
is uncertain against a real project with an identifiable author, and would have
held the entire distribution plan hostage.

npm availability was a tie-breaker among the free candidates, not an
afterthought: it is the plausible fourth channel if the d3 renderer is ever
published on its own, and it is where `vizzly` fell over.

**What would change it.** Nothing short of acquiring `vizzy` on PyPI, which is
not worth pursuing — the rename is done and cost 214 lines.

**Consequences already absorbed:** crates `vizzle-core` / `vizzle-py`, PyPI
`vizzle` / `vizzle-core`, Python modules `vizzle_core` / `vizzle_cli`, the
frontend global `window.vizzle`, and the Mermaid classDefs `vizzleAdded` /
`vizzleRemoved` / `vizzleModified`. The shared assets keep their `viz-core.*`
names — `viz` was never the brand.

## 4. Decision: one distribution, not two

**Decided 2026-08-16. Not yet implemented.**

Today there are two: `vizzle-core` (maturin, native, PyO3) and `vizzle`
(hatchling, pure Python, depends on the former).

**The failure mode that decides it.** Two distributions means two releases in
lockstep and two version numbers a user can land between. The binding surface —
10 functions in `crates/vizzle-py/src/lib.rs` — changes whenever the CLI does.
A user with `vizzle` 0.3 resolving against a cached `vizzle-core` 0.2 gets an
`AttributeError` deep in a command, blamed on their repo rather than on us. That
is one fact (the binding contract) expressed in two independently-versioned
places, which is exactly the duplication CLAUDE.md says to hunt.

**What is chosen.** maturin's mixed layout (`python-source`), putting
`vizzle_cli/` inside the native wheel. One distribution, one version, one
release, no skew possible.

**What it costs.** Every release rebuilds native wheels even for a
Python-only fix; and a contributor touching only Python still needs a Rust
toolchain. The second cost is already paid — `uv sync` builds the extension
today — and the first is CI time, not human time.

**What would change it.** Someone wanting `vizzle-core` as a standalone Python
library. No such consumer exists, and the Rust crate is the better library
surface for anyone who does (§10).

**Versioning.** `0.1.0` is currently declared in *three* independent places —
the Cargo workspace (inherited by both crates via `version.workspace = true`),
`packages/vizzle-cli/pyproject.toml`, and `crates/vizzle-py/pyproject.toml`.
That is the same duplication as the paragraph above, one level down. Merging to
one distribution removes one of the three; the release process must derive the
remaining Python version from the Cargo workspace rather than restating it.

## 5. The build matrix

`crates/vizzle-py/Cargo.toml` sets `abi3-py310`, which is why the matrix is
small: **one wheel per (OS, architecture)**, not per Python version, valid on
every Python ≥ 3.10 forever. The measured artifact is 1.5 MB.

| Target | Wheel tag |
| --- | --- |
| Linux x86_64 | `manylinux_2_17_x86_64` |
| Linux aarch64 | `manylinux_2_17_aarch64` |
| macOS arm64 | `macosx_11_0_arm64` |
| macOS x86_64 | `macosx_10_12_x86_64` |
| Windows x86_64 | `win_amd64` |

Six artifacts including the sdist. `PyO3/maturin-action` builds this matrix off
the shelf; the sdist is the fallback for anything unlisted and requires a Rust
toolchain on the consumer's machine, which is acceptable for a long tail.

There is no CI in the repo at all today (`.github/` does not exist). It is the
one genuinely absent piece of infrastructure.

## 6. Publishing

Tag-driven: pushing `v*` builds the matrix and publishes to PyPI via **Trusted
Publishing** (OIDC), so no API token is stored as a repository secret.

The Rust crates are published to crates.io only if and when someone wants
`vizzle-core` as a Rust library. Nothing about §2 depends on it, and an
unpublished crate is easier to change than a published one.

## 7. What a consumer must already have

- **`git` on PATH.** `vizzle_cli/git.py` shells out (`git diff --name-status`,
  `git cat-file --batch`). Universal in the context where vizzle is used.
- **Nothing else.** In particular *no Node*: d3 is vendored into the package and
  inlined into every generated page. A vizzle HTML file opens from `file://`
  with no network, which is what makes §9 work.

## 8. Decision: not a standalone Rust binary — for now

**Decided 2026-08-16. Standing rejection, with trigger conditions.**

The alternative is a pure-Rust `vizzle` distributed by cargo-dist, Homebrew, or
`curl | sh`: one static binary, no Python anywhere, no wheel matrix.

**What was measured.** Median of five runs on `~/code/h`, 358 parsed files:

| Command | Wall time |
| --- | --- |
| `vizzle --help` (interpreter + click, no work) | **0.07 s** |
| `vizzle component` → `.mmd` | 0.16 s |
| `vizzle component` → `.html` (500 KB page) | 0.16 s |
| `vizzle class` → `.html` (457 KB page) | 0.15 s |
| `vizzle diff --base HEAD~3` | 0.08 s |

**What that says.** The Python shell costs ~70 ms of a 160 ms run. A full
rewrite would buy at most 70 ms on a real repo, and the porting cost is ~400
lines — `git.py` (128) and `html.py` (70) are trivial, but `server.py` (215:
`http.server` + SSE + `watchfiles`) is real work. Seventy milliseconds is not
worth 400 lines.

It is worth being honest that the rewrite would not *violate* the layering in
CLAUDE.md — it would arguably simplify it, collapsing three layers into one.
The argument against it is cost and timing, not architecture.

**What would change it** — any one of these:

- A Homebrew formula, or any install path that must not assume Python.
- Distribution to people who do not have and will not install a Python runtime.
- Startup latency becoming visible, e.g. if `vizzle` is invoked per-file in a
  loop or wired into an editor's save hook, where 70 ms × N starts to show.

## 9. Distributing the output, not just the tool

Half of vizzle's value lands as an artifact someone looks at, not a command
someone runs — and the generated page is *already* a distribution channel,
because it is fully self-contained (§7). A 500 KB HTML file can be attached to a
PR, uploaded as a CI artifact, published to GitHub Pages, or emailed, and it
works offline with no infrastructure.

**This is a constraint on the renderer, not just an observation.** The moment a
generated page fetches anything at runtime, this rung dies and §2.3 gets much
more expensive. Keep pages self-contained.

## 10. Out of scope for now

- **`vizzle check`** — regenerate a committed diagram and exit non-zero on
  drift. Blocks §2.4. Needs a decision about what "unchanged" means when layout
  is force-directed (the `.mmd` is deterministic; the `.html` embeds coordinates).
- **A pre-commit mirror repo.** A `language: python` hook pointed at *this* repo
  would build from sdist and demand a Rust toolchain on every contributor's
  machine — unacceptable. The fix is the ruff/black pattern: a separate mirror
  repo pinning the published wheel. Worth doing only once §2.4 proves out.
- **Homebrew, `curl | sh`, cargo-dist.** All downstream of §8.
- **Publishing `vizzle-core` to crates.io** (§6) or the d3 renderer to npm.
- **A GitHub Action as a marketplace listing.** §2.3 works as a plain workflow
  step first; a published action is packaging on top of something that works.

## 11. Acceptance

Distribution is done when, from a machine that has never seen vizzle:

1. `uvx vizzle component ~/code/h` prints a diagram, having installed nothing
   into the target repo and requiring no Rust toolchain.
2. `uv tool install vizzle && vizzle --version` reports the version that a
   `v*` tag published, with no separate `vizzle-core` version to reconcile.
3. A pull request in an unrelated repo shows a component diff as a rendered
   Mermaid comment, produced by a workflow step that installs nothing globally.
4. The generated HTML from (3) opens from `file://` with the network disabled
   and renders without console errors.
