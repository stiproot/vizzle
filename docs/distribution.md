# Distribution

**Status:** implemented — §3 (the name), §4 (one distribution), §5 and §6 (the
release pipeline), §2.3 (the PR comment) and §2.6 (the agent plugin).
`vizzle 0.1.0` is on PyPI, which is all §2.1 and §2.2 ever needed. Not built:
§2.4 (waiting on a `--check` mode) and §2.5 (waiting on a mirror repo), both
in §10. §8 is a standing rejection with trigger conditions.
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

**Implemented 2026-08-16** in `.github/workflows/pr-diagram.yml`.

`vizzle diff --type component --base <base-sha>` on a PR, with the Mermaid
posted as a comment and the HTML uploaded as a build artifact. GitHub renders
` ```mermaid ` fenced blocks natively in comments and in Markdown files, so the
diagram appears inline with **no image hosting, no asset pipeline, and no
external service**. That property is worth protecting in the Mermaid renderer:
it is what makes this rung nearly free.

Four things the workflow does that are easy to leave out, and each of which
turns a nice idea into something people would switch off:

- **It runs on `pull_request`, never `pull_request_target`.** The latter runs
  with a write token while checking out the contributor's code — the
  pwn-request pattern, where a PR rewrites the workflow that is about to run it
  and walks off with the token. The price of safety is that fork PRs get a
  read-only token, so the job **skips** them rather than failing: a red X on
  someone's first contribution is worse than a missing diagram. The two-workflow
  `workflow_run` split that covers forks safely is described at the foot of the
  file, and is worth building when the first outside PR arrives.
- **It edits one comment instead of appending.** A twelve-commit PR should not
  accumulate twelve diagrams; a hidden `<!-- vizzle-component-diff -->` marker
  finds the previous one.
- **It says "no structural change" when nothing moved.** vizzle emits change
  markers only when something actually changed, so their absence is a reliable
  signal — and one honest line is better than a full diagram of an unchanged
  graph on every docs PR.
- **It checks vizzle can see the repo at all** before drawing (§2.6): with no
  `.py`/`.ts` present there is nothing to draw, and an empty diagram would read
  as a claim about the architecture rather than a limit of the tool.

Comments cap at 65536 characters, so a diagram past 60000 is replaced by a link
to the run's artifact.

### 2.4 Committed diagrams, checked for drift

A repo commits `docs/architecture.mmd`, and CI regenerates it and fails if it
differs. Turns a diagram into an artifact that cannot silently rot.

Needs a `--check` mode that does not exist yet (§10).

### 2.5 pre-commit

Lowest rung deliberately: see §10 for why it needs a mirror repo rather than a
hook pointed at this one, and why it is worth least of the five.

### 2.6 A coding agent

**Reader:** an LLM agent orienting itself in a repo before changing it, or
checking what its own change did. Listed last because it is the newest, not
because it reaches fewest — it may end up the mode that runs vizzle most.

An agent is a genuinely different reader from the two in CLAUDE.md, and the
difference is not cosmetic:

- **The HTML view is inert to it.** Zoom, pan, drag, the `+` explode toggle, the
  filter box — every affordance §2.1 exists to deliver is unusable. `--format
  html` is the wrong output for this reader, always.
- **Its constraint is tokens, not attention.** A human skims a big diagram
  cheaply and focuses where they like. An agent pays linearly for every element
  and cannot skim.

**What was measured** (on `~/code/h`, 358 parsed source files; token estimates
at ~3.7 chars/token):

| Output | Bytes | ~Tokens |
| --- | --- | --- |
| `component` → Mermaid | 5.4 KB | **1.5k** |
| `component` → JSON, `classes=False` | 34 KB | 9.3k |
| `component` → JSON, with classes | 185 KB | 50k |
| `class` → Mermaid | 59 KB | 16k |
| `class` → JSON | 156 KB | 42k |

**What that says, and it is the opposite of the obvious guess.** Mermaid is
**3–6× cheaper than JSON** for the same graph, and needs no parsing step — it is
a compact DSL an LLM already reads. The instinct that "a machine reader wants
JSON" is wrong here on token economics.

Part of that gap is content rather than encoding: the JSON export carries all
311 edges including externals, while the Mermaid renderer emits the 54 internal
ones by default. The direction survives the correction.

**The consequence, which is the whole reason an agent-facing skill is worth
shipping:** a component diagram costs ~1.5k tokens and substitutes for reading
358 files. Nothing else an agent can do has that orientation-per-token ratio,
and no agent discovers it unprompted. The corollary matters as much — an
unscoped `class` diagram is ~16k tokens, affordable occasionally but not
reflexively, so agents must be taught to scope it with `-I`.

**How this ships:** as a Claude Code plugin carrying one skill, in *this* repo
as its own single-plugin marketplace — not in the scaffolder that generates it.
A skill describing a flag the CLI does not have is a lying spec, the same
failure CLAUDE.md names for diagram specs, and the only defence is versioning
the skill with the code it documents.

**Sequenced after §6 deliberately.** A skill's first instruction is how to
invoke the tool, so the plugin is worth no more than its install line. Built
before publishing, that line is "clone it and install a Rust toolchain"; built
after, it is `uvx vizzle component .`.

**Implemented 2026-08-16**: `.claude-plugin/marketplace.json` + `plugins/vizzle/`,
carrying the single skill `vizzle-diagrams`. Every command the skill documents
was run against the published `vizzle@0.1.0` before shipping — the lying-spec
guard, applied literally.

The skill's most load-bearing paragraph is not the token table but the
*precondition*: vizzle parses Python and TypeScript only, and component
detection drops any module with no `.py`/`.ts` in it (component.md §3.1 rule
4). A Rust or Go repository therefore yields a near-empty diagram from
manifests that were found perfectly well. Without that warning up front an
agent burns a call and, worse, may report the emptiness as a finding about the
architecture.

### 2.6.1 What the evaluation showed

Tested by installing the plugin and prompting a real agent, which is the only
way to learn whether a skill fires:

- **"Understand the module dependency structure of h — what depends on what?"**
  → ran `uvx vizzle component --weights`, and carried the spec's own caution
  into its answer: *"vizzle prefers a missing edge to a wrong one, so absent
  edges are weak evidence."* The skill changed both the method and the
  epistemics of the result.
- **"Give me an architectural overview of h"** → read `ARCHITECTURE.md`
  instead, and did not reach for vizzle at all.

The second is **left as-is deliberately.** When a repository has a
hand-written architecture document, reading it really is better than
generating a diagram, and a description tuned hard enough to beat that would
fire on everything. The skill wins where it should: structure and dependency
questions, and repos with nothing written down. Revisit only if it turns out
to under-fire on repos that have no docs.

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

**Decided and implemented 2026-08-16.**

There were two: `vizzle-core` (maturin, native, PyO3) and `vizzle` (hatchling,
pure Python, depending on the former).

**The failure mode that decides it.** Two distributions means two releases in
lockstep and two version numbers a user can land between. The binding surface —
10 functions in `crates/vizzle-py/src/lib.rs` — changes whenever the CLI does.
A user with `vizzle` 0.3 resolving against a cached `vizzle-core` 0.2 gets an
`AttributeError` deep in a command, blamed on their repo rather than on us. That
is one fact (the binding contract) expressed in two independently-versioned
places, which is exactly the duplication CLAUDE.md says to hunt.

**What is chosen.** maturin's mixed layout: `packages/vizzle-cli/` is the
project root, with `python-source = "src"` and `manifest-path` reaching back to
`crates/vizzle-py/Cargo.toml`. One distribution, one version, one release, no
skew possible.

**Where the extension lives, and why it moved.** maturin requires the compiled
module to sit *inside* a Python package — `module-name` must resolve to a
directory under `python-source`. So the extension is now `vizzle_cli._core`, and
there is no importable top-level `vizzle_core` at all.

That constraint pushed toward the more honest structure rather than away from
it: the bindings were only ever imported by the CLI, and the underscore now says
so. The shipped wheel holds exactly one top-level name:

```
vizzle_cli/{__init__,cli,git,html,server}.py
vizzle_cli/_core.abi3.so
vizzle_cli/assets/…
```

Consequences: `crates/vizzle-py/Cargo.toml` sets `[lib] name = "_core"`, the
`#[pymodule]` is named `_core`, and the Python side does `from . import _core`.

**Verified**, not assumed — the layout has two failure modes worth testing, and
both were:

- **The sdist reaches outside its project directory.** `manifest-path` points up
  two levels, so the sdist must carry `crates/` and the workspace `Cargo.toml`
  or it cannot build. `uv build` produces a 151 KB sdist containing both, and
  builds a wheel *from that sdist* in a temp directory.
- **The wheel is self-contained.** Installed into an empty venv on a machine
  path with no repo and no Rust toolchain, `vizzle component ~/code/h` renders.

**What it costs.** Every release rebuilds native wheels even for a
Python-only fix; and a contributor touching only Python still needs a Rust
toolchain. The second cost is already paid — `uv sync` builds the extension
today — and the first is CI time, not human time.

**What would change it.** Someone wanting `vizzle-core` as a standalone Python
library. No such consumer exists, and the Rust crate is the better library
surface for anyone who does (§10).

**Versioning — fixed at the same time.** `0.1.0` had been declared in *three*
independent places: the Cargo workspace, `packages/vizzle-cli/pyproject.toml`,
and `crates/vizzle-py/pyproject.toml`. The same duplication as above, one level
down. Deleting the second distribution removed one, and the survivor now
declares `dynamic = ["version"]`, which maturin fills from the Cargo workspace.

**The version is now stated exactly once**, in the root `Cargo.toml`. A release
is a single number to bump.

## 5. The build matrix

**Implemented 2026-08-16** in `.github/workflows/release.yml`.

`crates/vizzle-py/Cargo.toml` sets `abi3-py310`, which is why the matrix is
small: **one wheel per (OS, architecture)**, not per Python version, valid on
every Python ≥ 3.10 forever. The measured artifact is 1.7 MB.

| Target | Wheel tag | Smoke-tested |
| --- | --- | --- |
| Linux x86_64 | `manylinux_2_28_x86_64` | yes |
| Linux aarch64 | `manylinux_2_28_aarch64` | yes (`ubuntu-24.04-arm`) |
| macOS arm64 | `macosx_11_0_arm64` | yes |
| macOS x86_64 | `macosx_10_12_x86_64` | **no** — cross-compiled, no runner |

Five artifacts including the sdist. The sdist is the fallback for anything
unlisted and requires a Rust toolchain on the consumer's machine, which is
acceptable for a long tail.

### 5.1 Three things the first release attempt taught us

All three were found by tagging `v0.1.0` for real. None would have been caught
by building wheels alone, which is the argument for the smoke job.

**glibc 2.17 produces a wheel that builds and cannot import.** `manylinux: auto`
selects manylinux2014 (glibc 2.17). tree-sitter's portable endian shim asks for
`_DEFAULT_SOURCE` — a macro glibc did not gain until **2.19** — so on 2.17
`le16toh` falls back to an implicit declaration, compiles with a warning cargo
never surfaces, and dies at import with `undefined symbol: le16toh`. Confirmed
directly in both containers:

```
manylinux2014   glibc 2.17  → implicit declaration, 1 undefined le16toh
manylinux_2_28  glibc 2.28  → clean
```

Hence `manylinux: "2_28"` pinned explicitly rather than `auto`. **The Linux
floor is glibc 2.28** (2018: RHEL 8, Ubuntu 18.10, Debian 10).

**Intel macOS has no runner any more.** GitHub retired the `macos-13` images,
and the failure mode is nasty: the job **queues forever instead of failing**, so
the release never finishes and nothing says why — every other leg went green in
under three minutes while that one sat pending. The x86_64 wheel is now
cross-compiled on Apple Silicon, which the macOS toolchain handles natively, at
the cost of being the one wheel nothing can execute. Every build job now carries
`timeout-minutes` so the next runner retirement fails loudly instead of hanging.

**Writing a diagram used the platform's default encoding.** `_emit` called
`Path.write_text` with no encoding, and diagrams carry `«guillemets»` and the
`✚ ✖ ✱` glyphs. On Windows that is cp1252 and every write raised
`UnicodeEncodeError`; on Linux under a non-UTF-8 locale it would do the same.
Fixed at both ends — writing output, and `file_in_worktree` reading source,
which had to match how `git show` already decoded the other revision or the two
sides of a diff would be read inconsistently.

**Windows is out of the matrix for now**, by decision rather than by defect —
the encoding bug it exposed is fixed. Adding it back means a `windows-latest`
leg in both matrices and nothing else.

**Three guards the release runs before it can publish**, each protecting a
failure this layout actually invites:

- **Tag vs. version.** §4 made the version a single number in the root
  `Cargo.toml`, which means a tag can now disagree with it. `check-version`
  fails the run before the matrix starts rather than after twenty minutes.
- **sdist completeness.** `manifest-path` reaches outside the project
  directory, so the sdist must carry `crates/` and the workspace manifest or it
  cannot build. The `sdist` job asserts all three files are present — a
  regression here would surface as a stranger's failed `pip install`.
- **A smoke test per platform.** Each wheel is installed on a clean runner and
  used to render this repo. A wheel that imports but cannot render is still a
  broken release, and only running it catches that.

`ci.yml` covers the same checks CLAUDE.md calls "verifying" on every push and
pull request, across Python 3.10 (the floor abi3 promises) and 3.13.

## 6. Publishing

**Implemented 2026-08-16.**

Tag-driven: pushing `v*` builds the matrix and publishes to PyPI via **Trusted
Publishing** (OIDC), so no API token is stored as a repository secret — PyPI
verifies GitHub directly.

**The publisher registration this depends on**, recorded here because it lives
in PyPI's web UI where nothing in this repo can point at it:

| Field | Value |
| --- | --- |
| PyPI project | `vizzle` |
| Owner / repository | `stiproot` / `vizzle` |
| Workflow | `release.yml` |
| Environment | `pypi` |

Two couplings that will bite silently if forgotten. **Renaming
`.github/workflows/release.yml` breaks publishing** until the registration is
updated to match — the filename is part of the trust relationship, not just a
path. And the `publish` job names `environment: pypi`, which must exist as a
GitHub environment; it is also the natural place to require manual approval
before a release leaves the building.

Releasing is therefore: bump `version` in the root `Cargo.toml`, commit, tag
`vX.Y.Z`, push the tag.

The Rust crates are published to crates.io only if and when someone wants
`vizzle-core` as a Rust library. Nothing about §2 depends on it, and an
unpublished crate is easier to change than a published one.

## 7. What a consumer must already have

- **`git` on PATH.** `vizzle_cli/git.py` shells out (`git diff --name-status`,
  `git cat-file --batch`). Universal in the context where vizzle is used.
- **Python ≥ 3.10**, and on Linux **glibc ≥ 2.28** (§5.1). Anything older falls
  back to the sdist and needs a Rust toolchain.
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
- **A GitHub Action as a marketplace listing.** §2.3 now works as a plain
  workflow, dogfooded here; packaging it as a reusable action or a marketplace
  listing is worth doing once a second repo has copied the file and the
  differences between them show what actually needs parameterising.
- **Fork-PR coverage for §2.3**, via the `workflow_run` split. Needs a first
  outside contributor to be worth the second workflow.
- **`--format json`.** `--format` is `Choice(["mermaid", "html"])`; the JSON
  exists in the bindings (`component_json_from_dir`, `graph_json_from_dir`) but
  never reaches stdout or a file — it is an intermediate on the way to HTML.
  Deferred **on evidence**, not oversight: the §2.6 measurements make JSON the
  more expensive way for an agent to read a graph, so it is not the agent
  format. Its one real advantage is queryability — `jq '.edges[] |
  select(.to=="packages/js/core")'` answers *"what depends on this?"* without
  reading the graph at all, which Mermaid cannot do. Worth adding when a
  concrete query use-case appears; it is one `Choice` entry and a branch.
- **A `/vizzle` command and a code-map subagent** alongside the §2.6 skill.
  The skill is the load-bearing piece; these are conveniences on top of it, and
  worth revisiting once the skill has been used enough to show where it fails to
  trigger.

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
5. An agent with the §2.6 plugin installed, asked to explain an unfamiliar
   repo, reaches for `vizzle component` before reading files — and spends
   ~1.5k tokens instead of tens of thousands.
