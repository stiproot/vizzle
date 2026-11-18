"""Mechanical grader for the explain-code evals.

Checks what the skill claims and a script can verify: which diagram the answer
led with, whether a structural one came from vizzle, its size, whether every
path it cites exists at the target revision, whether it renders, and whether the
brief has its parts. Whether the explanation is *right* beyond its needles is a
judgement, left to a reader of the answers.

    uv run python evals/explain-code/grade.py evals/explain-code/runs/<iteration> [--no-render]

Writes grading.json beside each answer and summary.{json,md} in the iteration.
Stdlib only, plus `vizzle render` (and a browser) for the render check.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]

FENCE = re.compile(r"```mermaid\s*\n(.*?)```", re.S)
# A path-like token with an optional :line. Longest extensions first, so `.json`
# is not read as `.js`; the guards keep `hashlib.sha256` from matching `.sh`.
PATHREF = re.compile(
    r"(?<![\w/`.\-])(/?(?:[\w.\-]+/)*[\w.\-]+"
    r"\.(?:json|toml|yaml|yml|lock|html|mjs|css|md|py|rs|ts|js|sh))"
    r"(?![\w])(?::(\d+))?"
)
ANCHOR = re.compile(r"\.(?:py|rs|ts|js|mjs|toml|ya?ml|json|md):\d+")
SECTIONS = {
    "answer": re.compile(r"^\s*(?:#+\s*|\*\*)\s*(?:1\.\s*)?answer\b", re.I | re.M),
    "walkthrough": re.compile(r"^\s*(?:#+\s*|\*\*)\s*(?:3\.\s*)?walkthrough\b", re.I | re.M),
    "next": re.compile(r"^\s*(?:#+\s*|\*\*)\s*(?:4\.\s*)?where to look next\b", re.I | re.M),
}
# What each case's primary diagram must be, and whether vizzle must have drawn it.
EXPECT = {
    "repo-shape": ({"flowchart"}, True),
    "doc-check-sequence": ({"sequenceDiagram"}, False),
    # curated.rs holds two types and its logic is functions, so a derived class
    # diagram cannot carry this answer; an honest hand-drawn pipeline can.
    "curated-zoom": ({"classDiagram", "flowchart"}, False),
    "restraint-one-function": (set(), False),
}


def kind(body: str) -> str:
    for line in body.splitlines():
        s = line.strip()
        if s and not s.startswith(("%%", "---", "title:")):
            return s.split()[0]
    return "empty"


def elements(k: str, body: str) -> int:
    if k == "sequenceDiagram":
        return len(re.findall(r"^\s*(?:participant|actor)\s+\S+", body, re.M))
    if k.startswith("classDiagram"):
        return len(set(re.findall(r"^\s*class\s+([\w.]+)", body, re.M)))
    if k.startswith("stateDiagram"):
        return len(set(re.findall(r"^\s*(?:state\s+)?(\w+)\s*(?:-->|:)", body, re.M)))
    return len(set(re.findall(r"^\s*(\w+)\s*[\[\(\{>]", body, re.M)))


def paths(text: str, target: Path) -> tuple[list[str], list[str]]:
    ok, bad = set(), set()
    for m in PATHREF.finditer(text):
        raw = m.group(1)
        rel = raw.lstrip("/")
        candidates = [target / rel]
        if "/" not in rel:
            # A bare file name cited after its directory was named once.
            candidates += list(target.glob(f"**/{rel}"))[:1]
        (ok if any(c.exists() for c in candidates) else bad).add(raw)
    return sorted(ok), sorted(bad)


def renders(answer: Path, out: Path) -> tuple[bool, str]:
    proc = subprocess.run(
        ["uv", "run", "vizzle", "render", str(answer), str(out)],
        cwd=REPO,
        capture_output=True,
        text=True,
        timeout=900,
    )
    if proc.returncode == 0:
        return True, f"{len(list(out.glob('*.png')))} fence(s) rendered"
    lines = (proc.stderr or proc.stdout).strip().splitlines()
    # Quote the parser's complaint, not the browser hint printed after every failure.
    errors = [line for line in lines if "error" in line.lower() and "mmdc failed" not in line]
    return False, " | ".join(errors[:2] or lines[-3:])


def skill_ran(run: Path) -> bool:
    """Whether the agent invoked explain-code, which `skills_loaded` (available) does not say."""
    transcript = run / "transcript.jsonl"
    if not transcript.exists():
        return False
    for line in transcript.read_text().splitlines():
        try:
            event = json.loads(line)
        except ValueError:
            continue
        message = event.get("message")
        content = message.get("content") if isinstance(message, dict) else None
        for block in content if isinstance(content, list) else []:
            is_skill = isinstance(block, dict) and block.get("type") == "tool_use" and block.get("name") == "Skill"
            if is_skill and "explain-code" in str(block.get("input", {}).get("skill", "")):
                return True
    return False


def grade(run: Path, case: dict, target: Path, render: bool, scratch: Path) -> dict:
    answer = run / "answer.md"
    text = answer.read_text() if answer.exists() else ""
    checks: list[dict] = []

    def check(name: str, passed: bool, evidence: str) -> None:
        checks.append({"text": name, "passed": bool(passed), "evidence": evidence})

    if not text.strip():
        check("produced an answer", False, "answer.md is empty")
        return {"case": case["name"], "checks": checks, "passed": 0, "total": 1}

    fences = FENCE.findall(text)
    kinds = [kind(f) for f in fences]
    counts = [elements(k, f) for k, f in zip(kinds, fences, strict=True)]
    ok, bad = paths(text, target)
    anchors = len(ANCHOR.findall(text))
    words = len(text.split())
    want, from_vizzle = EXPECT[case["name"]]

    check("Every path cited exists at the target revision", not bad, f"{len(ok)} resolved; unresolved: {bad or 'none'}")
    if want:
        check("Leads with the right kind of diagram", bool(kinds) and kinds[0] in want, f"fences={kinds or 'none'}")
        check("Leads with one primary diagram, not every level", 1 <= len(fences) <= 2, f"{len(fences)} fences")
        check("Primary diagram holds 4-9 elements", bool(counts) and 4 <= counts[0] <= 9, f"counts={counts or 'none'}")
        check("At least 3 path:line anchors", anchors >= 3, f"{anchors} anchors")
        found = {k: bool(p.search(text)) for k, p in SECTIONS.items()}
        check("Uses the four-part brief", all(found.values()), f"sections found: {found}")
        if render:
            good, evidence = renders(answer, scratch / f"{case['name']}-{run.name}")
            check("Every mermaid fence renders", good, evidence)
    else:
        check("Draws no diagram for a one-function question", not fences, f"{len(fences)} fences")
        check("Stays under 400 words", words < 400, f"{words} words")
        check("At least 1 path:line anchor", anchors >= 1, f"{anchors} anchors")
    if from_vizzle:
        derived = bool(fences) and "%% vizzle:" in fences[0]
        check("The structural diagram came from vizzle", derived, "trailer present" if derived else "hand-drawn")
    for needle, what in case["needles"].items():
        check(
            f"Names {what}", needle.lower() in text.lower(), "present" if needle.lower() in text.lower() else "absent"
        )

    passed = sum(c["passed"] for c in checks)
    return {"case": case["name"], "checks": checks, "passed": passed, "total": len(checks)}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("iteration", type=Path)
    parser.add_argument("--no-render", action="store_true")
    args = parser.parse_args()

    spec = json.loads((HERE / "evals.json").read_text())
    by_name = {c["name"]: c for c in spec["evals"]}
    rows = []
    with tempfile.TemporaryDirectory(prefix="explain-code-grade-") as tmp:
        target = Path(tmp) / "target"
        subprocess.run(
            ["git", "-C", str(REPO), "worktree", "add", "--detach", str(target), spec["target_ref"]],
            check=True,
            capture_output=True,
        )
        try:
            for run in sorted(args.iteration.glob("*/*-*")):
                if not (run / "meta.json").exists():
                    continue
                meta = json.loads((run / "meta.json").read_text())
                graded = grade(run, by_name[meta["case"]], target, not args.no_render, Path(tmp))
                (run / "grading.json").write_text(json.dumps(graded, indent=2) + "\n")
                rows.append(
                    {
                        **meta,
                        "passed": graded["passed"],
                        "total": graded["total"],
                        "skill_ran": skill_ran(run),
                        "checks": graded["checks"],
                    }
                )
        finally:
            subprocess.run(["git", "-C", str(REPO), "worktree", "remove", "--force", str(target)], check=False)

    lines = [
        "| case | config | run | passed | explain-code ran | cost | seconds | skills available |",
        "|---|---|---|---|---|---|---|---|",
    ]
    for r in sorted(rows, key=lambda r: (r["case"], r["config"], r["run"])):
        lines.append(
            f"| {r['case']} | {r['config']} | {r['run']} | {r['passed']}/{r['total']} | "
            f"{'yes' if r['skill_ran'] else 'no'} | "
            f"${r['cost_usd']} | {r['seconds']} | {', '.join(r['skills_loaded']) or '-'} |"
        )
    totals = {}
    for r in rows:
        t = totals.setdefault(r["config"], [0, 0])
        t[0] += r["passed"]
        t[1] += r["total"]
    lines += ["", *(f"- **{c}**: {p}/{t} checks ({p / t:.0%})" for c, (p, t) in sorted(totals.items()))]
    # Per check: where a gap between the configurations comes from.
    by_check: dict[str, dict[str, list[int]]] = {}
    for r in rows:
        for c in r["checks"]:
            cell = by_check.setdefault(c["text"], {}).setdefault(r["config"], [0, 0])
            cell[0] += c["passed"]
            cell[1] += 1
    configs = sorted(totals)
    lines += ["", "| check | " + " | ".join(configs) + " |", "|---|" + "---|" * len(configs)]
    for text, cells in sorted(by_check.items()):
        shown = [f"{cells[c][0]}/{cells[c][1]}" if c in cells else "-" for c in configs]
        lines.append(f"| {text} | " + " | ".join(shown) + " |")
    (args.iteration / "summary.md").write_text("\n".join(lines) + "\n")
    (args.iteration / "summary.json").write_text(json.dumps({"rows": rows, "totals": totals}, indent=2) + "\n")
    print("\n".join(lines))
    return 0


if __name__ == "__main__":
    sys.exit(main())
