"""Run the explain-code evals: each case with and without vizzle's plugin.

Each run is one `claude -p` session over a throwaway worktree of this repository
at the set's `target_ref`, so the code an answer cites is fixed and the working
checkout is never touched. Every plugin the machine's settings enable is turned
off for both configurations, so the baseline is a bare agent and the only
difference is `--plugin-dir plugins/vizzle`.

    uv run python evals/explain-code/run.py                      # every case, both configs, 1 run
    uv run python evals/explain-code/run.py --only repo-shape --runs 3
    uv run python evals/explain-code/grade.py evals/explain-code/runs/<iteration>

Writes runs/<iteration>/<case>/<config>-<n>/{answer.md, transcript.jsonl, meta.json}.
Stdlib only.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tempfile
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
PLUGIN = REPO / "plugins" / "vizzle"
CONFIGS = ("with_skill", "without_skill")
# Read-only tools: an answer reads code and runs vizzle, nothing else.
TOOLS = "Read Grep Glob Bash(uvx vizzle:*) Bash(git ls-files:*) Bash(git log:*)"


def enabled_plugins() -> dict[str, bool]:
    """Every plugin a settings file on this machine enables, mapped to off."""
    found: dict[str, bool] = {}
    for path in (Path.home() / ".claude" / "settings.json", REPO / ".claude" / "settings.json"):
        try:
            names = json.loads(path.read_text()).get("enabledPlugins", {})
        except (OSError, ValueError):
            continue
        found.update(dict.fromkeys(names, False))
    return found


def worktree(ref: str, into: Path) -> Path:
    subprocess.run(
        ["git", "-C", str(REPO), "worktree", "add", "--detach", str(into), ref],
        check=True,
        capture_output=True,
    )
    return into


def run_one(case: dict, config: str, n: int, target: Path, out: Path, model: str, budget: float) -> dict:
    out.mkdir(parents=True, exist_ok=True)
    cmd = [
        "claude",
        "-p",
        case["prompt"],
        "--output-format",
        "stream-json",
        "--verbose",
        "--model",
        model,
        "--max-budget-usd",
        str(budget),
        "--allowedTools",
        TOOLS,
        "--strict-mcp-config",
        "--no-session-persistence",
        "--settings",
        json.dumps({"enabledPlugins": enabled_plugins()}),
    ]
    if config == "with_skill":
        cmd += ["--plugin-dir", str(PLUGIN)]
    started = time.monotonic()
    proc = subprocess.run(cmd, cwd=target, capture_output=True, text=True, timeout=1200)
    elapsed = time.monotonic() - started
    (out / "transcript.jsonl").write_text(proc.stdout)
    result, skills = {}, []
    for line in proc.stdout.splitlines():
        try:
            event = json.loads(line)
        except ValueError:
            continue
        if event.get("type") == "system" and event.get("subtype") == "init":
            skills = event.get("skills", [])
        if event.get("type") == "result":
            result = event
    (out / "answer.md").write_text(result.get("result", ""))
    meta = {
        "case": case["name"],
        "config": config,
        "run": n,
        "exit": proc.returncode,
        "seconds": round(elapsed, 1),
        "cost_usd": result.get("total_cost_usd"),
        "turns": result.get("num_turns"),
        "skills_loaded": sorted(s for s in skills if "vizzle" in s or "explain" in s or "diagram" in s),
        "stderr_tail": proc.stderr.strip().splitlines()[-3:],
    }
    (out / "meta.json").write_text(json.dumps(meta, indent=2) + "\n")
    return meta


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--only", action="append", help="case name (repeatable); default every case")
    parser.add_argument("--config", choices=CONFIGS, action="append", help="default both")
    parser.add_argument("--runs", type=int, default=1)
    parser.add_argument("--model", default="sonnet")
    parser.add_argument("--max-budget-usd", type=float, default=2.0, help="per run")
    parser.add_argument("--parallel", type=int, default=4)
    parser.add_argument("--iteration", default=time.strftime("%Y%m%d-%H%M%S"))
    args = parser.parse_args()

    spec = json.loads((HERE / "evals.json").read_text())
    cases = [c for c in spec["evals"] if not args.only or c["name"] in args.only]
    configs = args.config or list(CONFIGS)
    iteration = HERE / "runs" / args.iteration
    with tempfile.TemporaryDirectory(prefix="explain-code-eval-") as tmp:
        target = worktree(spec["target_ref"], Path(tmp) / "target")
        try:
            jobs = [
                (case, config, n, iteration / case["name"] / f"{config}-{n}")
                for case in cases
                for config in configs
                for n in range(1, args.runs + 1)
            ]
            with ThreadPoolExecutor(args.parallel) as pool:
                metas = list(
                    pool.map(
                        lambda j: run_one(j[0], j[1], j[2], target, j[3], args.model, args.max_budget_usd),
                        jobs,
                    )
                )
        finally:
            subprocess.run(["git", "-C", str(REPO), "worktree", "remove", "--force", str(target)], check=False)
    (iteration / "runs.json").write_text(
        json.dumps({"target_ref": spec["target_ref"], "model": args.model, "runs": metas}, indent=2) + "\n"
    )
    for m in metas:
        print(f"{m['case']:24} {m['config']:14} exit={m['exit']} {m['seconds']}s ${m['cost_usd']} {m['skills_loaded']}")
    print(f"\ngrade: uv run python {HERE.relative_to(REPO)}/grade.py {iteration.relative_to(REPO)}")
    return 0 if all(m["exit"] == 0 for m in metas) else 1


if __name__ == "__main__":
    sys.exit(main())
