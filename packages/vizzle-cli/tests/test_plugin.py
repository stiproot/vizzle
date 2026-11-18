"""The agent plugin is held to the CLI it documents.

The plugin's skills and commands are prose over this CLI. A command or flag a
skill names that the CLI does not have is the lying spec again: an agent runs
it, fails, and loses trust in the rest. These tests read every skill and
command file and check what they name against the real Click command tree, and
check the plugin loads under both agents (docs/plans/impl/skill-set.md §6).
"""

import json
import re
from pathlib import Path

import click
import pytest
from click.testing import CliRunner
from vizzle_cli.cli import main

ROOT = Path(__file__).resolve().parents[3]
PLUGIN = ROOT / "plugins" / "vizzle"
PROSE = sorted([*PLUGIN.glob("skills/**/*.md"), *PLUGIN.glob("commands/*.md")])

# A fenced block, indented (inside a list item) or not, with ``` or ~~~.
FENCE = re.compile(r"^[ \t]*(```|~~~)[^\n]*\n(.*?)^[ \t]*\1", re.M | re.S)
INLINE = re.compile(r"`([^`\n]+)`")
# `vizzle <command>`, with an optional pinned version (`vizzle@0.15.0`, `vizzle==0.15.0`).
INVOCATION = re.compile(r"\bvizzle(?:(?:@|==)\S+)?\s+([a-z][a-z-]*)(.*)")
# Where one invocation ends on its line: a pipe, a chain, a comment.
INVOCATION_END = re.compile(r"\s(?:\||&&|;|#)\s|\s#")
FLAG = re.compile(r"(?<![\w-])(--?[A-Za-z][\w-]*)")
# A value that stands for one the reader supplies: `<path>`, `$ARGUMENTS`, `X.Y.Z`.
PLACEHOLDER = re.compile(r"^[<$]|^[A-Z][A-Z.]*$")

# Colours a skill names on purpose although they are not the palette's.
OFF_PALETTE_ON_PURPOSE = {
    "rgb(255, 235, 233)": "the opaque fill design-diagrams warns against, as the example of what breaks dark mode",
}


def option_table(command: click.Command) -> dict[str, click.Option]:
    table = {"--help": None}
    for param in command.params:
        if isinstance(param, click.Option):
            for name in [*param.opts, *param.secondary_opts]:
                table[name] = param
    return table


COMMANDS = {name: option_table(cmd) for name, cmd in main.commands.items()}
EVERY_FLAG = {flag for table in COMMANDS.values() for flag in table}


def code_spans(text: str) -> tuple[list[str], list[str]]:
    """Lines inside fenced blocks (shell continuations joined), and inline code spans outside them."""
    fenced = [line for match in FENCE.finditer(text) for line in re.sub(r"\\\n\s*", " ", match.group(2)).splitlines()]
    inline = INLINE.findall(FENCE.sub("", text))
    return fenced, inline


def invocations(text: str) -> list[tuple[str, list[str], str]]:
    """Every `vizzle <command> ...` named in code: (command, its tokens, source line).

    An inline span that starts with a command name and carries a flag
    (`component --focus`) is read as that command too: prose that names a flag
    against a command makes the same claim as a full invocation.
    """
    fenced, inline = code_spans(text)
    found = []
    for line in fenced + inline:
        matches = list(INVOCATION.finditer(line))
        for match in matches:
            rest = INVOCATION_END.split(match.group(2), maxsplit=1)[0]
            found.append((match.group(1), rest.split(), line.strip()))
        words = line.split()
        if not matches and line in inline and len(words) > 1 and words[0] in COMMANDS and FLAG.match(words[1]):
            found.append((words[0], words[1:], line.strip()))
    return found


def problems_in(text: str) -> list[str]:
    """What the prose claims about the CLI that the CLI does not do."""
    problems = []
    for command, tokens, line in invocations(text):
        if command not in COMMANDS:
            problems.append(f"`vizzle {command}` is not a command: {line}")
            continue
        table = COMMANDS[command]
        for i, token in enumerate(tokens):
            if not FLAG.fullmatch(token.split("=", 1)[0]):
                continue
            flag, _, inline_value = token.partition("=")
            if flag not in table:
                problems.append(f"`vizzle {command}` has no {flag}: {line}")
                continue
            option = table[flag]
            if option is None or not isinstance(option.type, click.Choice):
                continue
            value = inline_value or (tokens[i + 1] if i + 1 < len(tokens) else "")
            supplied = value and not value.startswith("-") and not PLACEHOLDER.search(value)
            if supplied and value not in option.type.choices:
                problems.append(f"`vizzle {command} {flag}` takes {list(option.type.choices)}, not {value}: {line}")
    return problems


def test_the_plugin_has_prose_to_check() -> None:
    names = {p.relative_to(PLUGIN).as_posix() for p in PROSE}
    for expected in (
        "skills/vizzle-diagrams/SKILL.md",
        "skills/explain-code/SKILL.md",
        "skills/design-diagrams/SKILL.md",
        "commands/explain.md",
        "commands/map.md",
    ):
        assert expected in names, f"{expected} is missing; found {sorted(names)}"


@pytest.mark.parametrize("path", PROSE, ids=lambda p: p.relative_to(PLUGIN).as_posix())
def test_every_command_and_flag_named_exists(path: Path) -> None:
    problems = problems_in(path.read_text())
    assert not problems, "\n".join(problems)


@pytest.mark.parametrize("path", PROSE, ids=lambda p: p.relative_to(PLUGIN).as_posix())
def test_every_flag_named_on_its_own_exists_somewhere(path: Path) -> None:
    # Prose like "add `--weights`" names a flag without its command. It must at
    # least be a flag of some command; the invocation check above is the strict one.
    _, inline = code_spans(path.read_text())
    named = {m.group(1) for span in inline if not INVOCATION.search(span) for m in [FLAG.match(span)] if m}
    unknown = sorted(named - EVERY_FLAG)
    assert not unknown, f"no vizzle command has {unknown}"


@pytest.mark.parametrize(
    "prose",
    [
        # --focus exists only on `diff`; the first draft of the skill-set spec put it on `component`.
        "```sh\nuvx vizzle component . --focus  # a comment --is-not-a-flag\n```\n",
        "```sh\nuvx vizzle component . \\\n  --frobnicate\n```\n",
        "1. Run it:\n\n   ```sh\n   uvx vizzle component . --frobnicate\n   ```\n",
        "~~~sh\nuvx vizzle component . --frobnicate\n~~~\n",
        "```sh\nuvx vizzle==0.15.0 component . --frobnicate\n```\n",
        "```sh\nuvx vizzle palette -f yaml\n```\n",
        "```sh\nuvx vizzle diff . --type=sequence\n```\n",
        "Narrow it with `component --focus`.",
        "run `vizzle frobnicate`",
    ],
)
def test_the_check_catches_a_lie(prose: str) -> None:
    assert problems_in(prose), f"passed: {prose!r}"


def test_the_check_passes_placeholders_and_real_values() -> None:
    assert not problems_in(
        "```sh\nuvx vizzle diff <path> --type component -f mermaid\nuvx vizzle render a.md out/ --theme=both\n```\n"
    )


def frontmatter(path: Path) -> dict[str, str]:
    text = path.read_text()
    assert text.startswith("---\n"), f"{path} does not start with --- frontmatter"
    block = text[4 : text.index("\n---", 4)]
    keys = {}
    for line in block.splitlines():
        key, _, value = line.partition(":")
        keys[key.strip()] = value.strip()
    return keys


@pytest.mark.parametrize("skill", sorted(PLUGIN.glob("skills/*/SKILL.md")), ids=lambda p: p.parent.name)
def test_skill_frontmatter_is_the_two_keys_both_agents_read(skill: Path) -> None:
    keys = frontmatter(skill)
    assert sorted(keys) == ["description", "name"], f"{skill}: {sorted(keys)}"
    assert keys["name"] == skill.parent.name


@pytest.mark.parametrize("command", sorted(PLUGIN.glob("commands/*.md")), ids=lambda p: p.stem)
def test_commands_carry_a_description(command: Path) -> None:
    assert frontmatter(command).get("description"), command


def test_the_plugin_loads_under_both_agents() -> None:
    claude = json.loads((PLUGIN / ".claude-plugin" / "plugin.json").read_text())
    codex = json.loads((PLUGIN / ".codex-plugin" / "plugin.json").read_text())
    assert claude["name"] == codex["name"] == "vizzle"
    assert claude["version"] == codex["version"], "bump-version.sh moves both"
    assert codex["skills"] == "./skills/"

    claude_market = json.loads((ROOT / ".claude-plugin" / "marketplace.json").read_text())
    codex_market = json.loads((ROOT / ".agents" / "plugins" / "marketplace.json").read_text())
    assert [p["source"] for p in claude_market["plugins"]] == ["./plugins/vizzle"]
    (entry,) = codex_market["plugins"]
    assert entry["name"] == "vizzle"
    assert entry["source"] == {"source": "local", "path": "./plugins/vizzle"}
    assert entry["policy"]["installation"] in {"AVAILABLE", "INSTALLED_BY_DEFAULT", "NOT_AVAILABLE"}
    assert entry["policy"]["authentication"] in {"ON_INSTALL", "ON_USE"}
    assert entry["category"]


def test_the_plugin_needs_no_other_plugin() -> None:
    # vizzle's plugin is the single source of truth for diagramming; naming the
    # retired one as something to load would send an agent back to it.
    retired = re.compile(r"code-comprehension|comprehend-code|generated-diagrams|c4-mermaid", re.I)
    hits = [
        f"{path.relative_to(ROOT)}:{n}: {line.strip()}"
        for path in sorted(PLUGIN.rglob("*"))
        if path.is_file()
        for n, line in enumerate(path.read_text().splitlines(), 1)
        if retired.search(line)
    ]
    assert not hits, "\n".join(hits)


def test_every_colour_a_skill_gives_is_the_palettes() -> None:
    result = CliRunner().invoke(main, ["palette", "-f", "json"])
    assert result.exit_code == 0, result.output
    palette = json.loads(result.output)
    source = " ".join([*(s["rect"] for s in palette["sequence"]), *palette["classDefs"]])
    colour = re.compile(
        r"#[0-9a-fA-F]{3,8}\b|(?:rgba?|hsla?)\(\s*\d[^)]*\)|(?:fill|stroke|color|background):\s*[a-z]+\b"
    )
    allowed = set(colour.findall(source))
    stray = sorted(
        f"{path.relative_to(PLUGIN)}: {c}"
        for path in PROSE
        for c in colour.findall(path.read_text())
        if c not in allowed and c not in OFF_PALETTE_ON_PURPOSE
    )
    assert not stray, f"use `vizzle palette`'s colours: {stray}"
