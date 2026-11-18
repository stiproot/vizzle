# explain-code evals

Does the `explain-code` skill make an agent answer a question about code the way
the skill says: one diagram at the right level, from vizzle when the code can
supply it, every cited path real, the four-part brief, and no diagram for a
one-function question?

Four questions about **this repository**, each aimed at a different level
(`evals.json`). Each runs with and without vizzle's plugin; the only difference
between the two is `--plugin-dir plugins/vizzle`. Every plugin the machine's
settings enable is switched off for both, so the baseline is a bare agent. (A
user-level skill such as `explain-clearly` is not a plugin and loads in both;
it is listed in the results.)

```sh
uv run python evals/explain-code/run.py                 # every case, both configs, 1 run each
uv run python evals/explain-code/run.py --runs 3 --iteration <name>
uv run python evals/explain-code/grade.py evals/explain-code/runs/<name>   # --no-render to skip mermaid-cli
```

- The target is a throwaway `git worktree` at `target_ref`, so the code an answer
  cites never moves and the working checkout is never touched. Move `target_ref`
  only together with the needles and the expected answers.
- `run.py` needs `claude` on PATH; `grade.py` needs `vizzle render` (and a
  browser) for its render check. Both are stdlib-only Python.
- Runs land in `runs/` (git-ignored: transcripts are large). Commit a summary
  when it sets or moves the baseline.

## What the grader checks

Mechanically, per answer: every cited path exists at the target revision; the
answer leads with the expected kind of diagram (or none, for the one-function
question); one primary diagram of 4–9 elements; at least 3 `path:line` anchors;
the four-part brief; every mermaid fence renders; the structural diagram of
`repo-shape` came from vizzle (its `%% vizzle:` trailer); and each case's needles.
Whether the explanation is *right* beyond those needles is a judgement left to a
reader of `answer.md`.

## Baseline (2026-11-18, `target_ref` 233ee82, sonnet, 3 runs per configuration)

[`baseline-2026-11-18.md`](baseline-2026-11-18.md): **with the skill 95/111
checks (86%), without 67/111 (60%)**, 24 runs, $2.12.

| case | with skill | without | explain-code ran (with skill) |
| --- | --- | --- | --- |
| repo-shape | 30/33 | 15/33 | 3 of 3 |
| curated-zoom | 27/30 | 16/30 | 3 of 3 |
| doc-check-sequence | 20/30 | 18/30 | 1 of 3 |
| restraint-one-function | 18/18 | 18/18 | 0 of 3 |

**What the gap is, and is not.** The skill changes the *form* of an answer, not
its content as far as these checks can see: every needle check (does the answer
name managed.py, the stale decision, the curated-only kinds...) passes in both
configurations. The 28-check gap is all form: leading with a mermaid diagram of
the right kind (6/9 against 0/9; without the skill the agent drew ASCII or
nothing), one primary diagram, its size, the structural diagram coming from
vizzle (3/3 against 0/3), and the four-part brief's headings (7/9 against 0/9).
The last two are the skill's own conventions, so a bare agent cannot be expected
to pass them; they measure whether the skill was followed, not whether the
answer is better. The restraint case says nothing about the skill: it never ran
there, and a bare agent passes it too. "skills available" in the table lists
what was installed, not what ran; "explain-code ran" is the `Skill` call in the
transcript.

How it got there. Iteration 1 (76% against 59%) found two defects in the skill,
both fixed before this baseline: it did not trigger on "how is this part put
together, where would a new kind be handled" (0 of 3 runs), and agents ran
`vizzle component` and then retyped the diagram, which drops the trailer and
makes a derived diagram a hand-drawn one. The description was widened, and the
skill now says to paste vizzle's output as printed and to render a hand-drawn
diagram before answering. Iteration 1 also showed one assertion was wrong:
`curated.rs` holds two types and its logic is functions, so `curated-zoom` no
longer requires a derived diagram; an honest hand-drawn flowchart passes.

## Open, for the next iteration

1. **Sequence questions.** `doc-check-sequence` barely moves (20 against 18): the
   skill triggered in 1 of 3 runs, and with or without it the agent answered in
   prose or with a flowchart instead of a `sequenceDiagram`.
2. **Content.** No check here tells a better explanation from a worse one beyond
   the needles. A judged comparison (a reader scoring paired answers blind) is
   what would show whether the diagram-first form helps a reader.
3. **Derivation is checked on one case.** Only `repo-shape` asserts the diagram
   came from vizzle; a second structural case where a derived diagram is the
   right answer would make that check worth more than 3 data points.
4. **Anchors on the repo overview.** `repo-shape` cites paths but rarely
   `path:line` (a whole-repo answer has few lines to point at); decide whether
   the assertion or the skill should change.
