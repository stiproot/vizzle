---
name: explain-code
description: Explain how code works to a person, leading with one diagram at the right level (taken from vizzle where the code can supply it) and grounding every element in a file path. Use whenever someone asks how a repository, package, module or subsystem is put together, what its moving parts are, how they depend on each other, what happens when something runs, or what a change did, and whenever they are about to change an area and want it explained first (where would X have to be handled, what would I touch to add Y). For orienting yourself with no question to answer, use vizzle-diagrams.
---

# Explaining code with one diagram

When a person asks how code works, your job is to transfer understanding per
unit of their attention. Lead with a diagram; prose follows the picture. When
the question is genuinely small (one function, one line, one value), answer in
prose: a diagram that compresses nothing is noise.

The structural pictures are not drawn by hand. vizzle derives them from the
code, so a box cannot be invented and a renamed module cannot linger. You choose
the level and the scope; vizzle draws; you write the answer around it.

## 1. Pick the level first

Match the question to ONE primary view (`references/level-selection.md` has the
table). Answer at the highest level that fully answers it, and offer the zoom
rather than drawing every level at once.

| The question sounds like | View | Where the diagram comes from |
| --- | --- | --- |
| "What are the moving parts, how do they talk?" (a repo) | the repo's shape | `uvx vizzle component <repo>` |
| "How does this one service or package work inside?" | one component and its neighbours | `uvx vizzle component <repo> --around <component> --depth 1` |
| "How is this data model or contract structured?" | classes | `uvx vizzle class <repo> -I '<scope>/**'` |
| "What did this change do?" | a change | `uvx vizzle diff <repo> --base main --type component --focus` |
| "What would this change look like?" | a planned change | an overlay (`design-diagrams`) |
| "What happens when X runs, in what order?" | behaviour: sequence | mermaid `sequenceDiagram`, by hand |
| "What states can this be in?" | behaviour: state | mermaid `stateDiagram-v2`, by hand |
| "Who uses it, what does it talk to, where does it run?" | context or deployment (not in the code) | mermaid `flowchart`, by hand (`design-diagrams`) |

**Structural questions** ask about shape: what exists, what depends on what.
**Behavioural questions** ask about time: what happens, in what order. Pick by
what the reader needs to see, not by what is easiest to produce. A component
diagram is the wrong answer to "what happens when I run sync", however cheap
it is.

Before relying on vizzle, check it can see the repo (Python, TypeScript, Rust
and Go; `vizzle-diagrams` has the one-line check). For a repo it cannot
parse, draw the structural view by hand as a `flowchart` and say so.

## 2. Get the diagram

Run the command from the table, read the mermaid on stdout, and narrow it until
it holds what the answer needs:

```sh
uvx vizzle component <repo> --around orchestrator --depth 1     # one component and its neighbours
uvx vizzle component <repo> --highlight persistence             # the whole shape, with the subject lit
uvx vizzle class <repo> -I 'src/billing/**' --no-members        # the types, without their bodies
uvx vizzle class <repo> --around Invoice --depth 1              # one class and what it touches
uvx vizzle diff <repo> --base main --type component --focus     # only what the change rewired
```

**Paste vizzle's output as it printed it**, trailer lines included. A derived
diagram is trustworthy because nobody edited it: retype or trim it by hand and
it becomes a hand-drawn diagram that only looks derived. To change what it
shows, change the flags and run it again. The `%% vizzle:` trailer also tells
the reader what was selected and that the picture came from the code.

Use `--highlight` when the answer is about two or three elements inside a
larger shape; use `--around` when everything else is in the way. An unknown
name errors and lists the names that exist, which is also a cheap way to learn
them.

Keep it small: 5–9 elements is the target. A whole-repo component diagram of
40 boxes is an orientation aid for you, not an answer for a person; scope it
with `--around`, `-I` or `--split` until it fits, or split the answer into an
overview plus one zoom.

For a behavioural view, draw the `sequenceDiagram` yourself, and name its
participants after the boxes in the structural diagram so the two views line
up. Colours and syntax traps are in `design-diagrams`. A diagram you drew can
fail to parse, and a reader then sees an error box instead of the answer:
write it to a file and run `uvx vizzle render <file.md> <out-dir>` before you
answer. A diagram in a plain ` ``` ` fence (ASCII art) is not a diagram the
reader's tools can draw; use mermaid.

## 3. Ground every element

Every box must be something real, and the walkthrough anchors the elements it
talks about to `path/to/file.py:123`, so the reader can jump from the picture
to the code. A derived diagram is grounded by construction: you do not need to
cite every box, only the ones the answer discusses. A hand-drawn diagram (a
sequence, a context view) is not, so cite each element and never invent one to
make the picture look complete. Mark anything unknown as unknown.

vizzle prefers a missing edge to a wrong one. An absent edge is weak evidence;
say so before concluding two parts are independent.

## 4. The brief

Structure every substantial answer as four parts:

1. **Answer**: one or two sentences that answer the question, before anything else.
2. **Diagram**: the one primary diagram, in a fenced ` ```mermaid ` block.
3. **Walkthrough**: short prose in the diagram's reading order, keyed to its
   element names (bold each on first mention), with file-path anchors.
4. **Where to look next**: two to four files ranked by relevance, and which
   zoom you would draw next if asked (the vizzle command, or the sequence view).

## 5. Iterate on the same diagram

When the person corrects an element or asks a follow-up, change the same
diagram (re-run the command with a different `--around`, `--depth` or `-I`, or
edit the sequence) instead of drawing an unrelated one. The diagram is the
conversation's whiteboard; keep element names stable across revisions so the
changes stay legible.

## Requirements

`uvx` (from [uv](https://docs.astral.sh/uv/)) and `git` on PATH. Nothing is
installed into the repository being explained.
