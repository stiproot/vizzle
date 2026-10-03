#!/usr/bin/env bun
/**
 * Draw vizzle with vizzle, and capture the live `doc --check` evidence the
 * "Diagrams that don't lie" section shows.
 *
 * Runs from site/ on every build (`bun run prebuild`), locally and in CI, against
 * the vizzle built from the current checkout (`uv run --project ..`). Everything
 * it writes — public/diagrams/vizzle-*.html and src/generated/self.json — is
 * generated and gitignored.
 *
 * The point of the assertions is that a diagram that lies must not deploy:
 * a missing <svg>, a component page that drops vizzle-core, a stale managed doc
 * that passes, or a stale-copy demo that does not catch drift all fail the build.
 */

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const SITE_ROOT = new URL("..", import.meta.url).pathname;
const REPO_ROOT = join(SITE_ROOT, "..");
const PUBLIC_DIAGRAMS = join(SITE_ROOT, "public/diagrams");
const GENERATED_DIR = join(SITE_ROOT, "src/generated");
const COMPONENTS_HTML = join(PUBLIC_DIAGRAMS, "vizzle-components.html");
const CLASSES_HTML = join(PUBLIC_DIAGRAMS, "vizzle-classes.html");

interface Run {
  stdout: string;
  stderr: string;
  status: number;
}

function run(command: string, args: string[]): Run {
  const result = spawnSync(command, args, { cwd: SITE_ROOT, encoding: "utf8" });
  if (result.error) fail(`could not run ${command}: ${result.error.message}`);
  return { stdout: result.stdout ?? "", stderr: result.stderr ?? "", status: result.status ?? 1 };
}

function output(run: Run): string {
  return (run.stderr + run.stdout).trim();
}

function fail(message: string): never {
  console.error(`generate-self — ${message}`);
  process.exit(1);
}

/** The stats line vizzle prints when it writes a diagram: `wrote <path>  (summary)`. */
function statsOf(run: Run): string | null {
  const match = /wrote\s+.+?\s+\(([^)]*)\)/.exec(run.stderr + run.stdout);
  return match ? match[1].trim() : null;
}

mkdirSync(PUBLIC_DIAGRAMS, { recursive: true });
mkdirSync(GENERATED_DIR, { recursive: true });

// 1 + 2. The two self diagrams, from the checkout's own vizzle.
const components = run("uv", [
  "run",
  "--project",
  REPO_ROOT,
  "vizzle",
  "component",
  REPO_ROOT,
  "-o",
  COMPONENTS_HTML,
]);
const classes = run("uv", [
  "run",
  "--project",
  REPO_ROOT,
  "vizzle",
  "class",
  join(REPO_ROOT, "crates"),
  "-l",
  "rust",
  "-o",
  CLASSES_HTML,
]);

if (!existsSync(COMPONENTS_HTML)) fail("vizzle component wrote no file");
if (!existsSync(CLASSES_HTML)) fail("vizzle class wrote no file");

const componentsHtml = readFileSync(COMPONENTS_HTML, "utf8");
const classesHtml = readFileSync(CLASSES_HTML, "utf8");

// 3. The real check: the committed managed doc must be current.
const fresh = run("uv", [
  "run",
  "--project",
  REPO_ROOT,
  "vizzle",
  "doc",
  "--dir",
  join(REPO_ROOT, "docs/diagrams"),
  "--root",
  REPO_ROOT,
  "--check",
]);

// 4. The stale-copy demo: edit one member row inside the generated fence of a
// copy. The manifest is untouched, so a non-zero exit here can only mean vizzle
// detected drift — a malformed copy would error instead (the trap this guards).
const tmp = mkdtempSync(join(tmpdir(), "vizzle-stale-"));
let stale: Run;
let staleOutput = "";
try {
  const docPath = join(REPO_ROOT, "docs/diagrams/core-model.md");
  const markdown = readFileSync(docPath, "utf8");
  const fence = /```mermaid\n([\s\S]*?)```/.exec(markdown);
  if (!fence) fail("docs/diagrams/core-model.md has no mermaid fence to edit");
  const editedBody = fence[1].replace(": String", ": Strng");
  if (editedBody === fence[1]) fail("stale-copy edit changed nothing in the fence");
  const editedDoc =
    markdown.slice(0, fence.index) +
    "```mermaid\n" +
    editedBody +
    "```" +
    markdown.slice(fence.index + fence[0].length);
  writeFileSync(join(tmp, "core-model.md"), editedDoc, "utf8");

  stale = run("uv", [
    "run",
    "--project",
    REPO_ROOT,
    "vizzle",
    "doc",
    "--dir",
    tmp,
    "--root",
    REPO_ROOT,
    "--check",
  ]);
  // Stable spelling on the page: the random temp path is noise.
  staleOutput = output(stale).split(tmp).join("<stale-copy>");
} finally {
  rmSync(tmp, { recursive: true, force: true });
}

// --- Assertions: a demo that passes for the wrong reason must not deploy. ---

if (!componentsHtml.includes("<svg")) fail("vizzle-components.html has no <svg");
if (!classesHtml.includes("<svg")) fail("vizzle-classes.html has no <svg");
if (!componentsHtml.includes("vizzle-core"))
  fail("vizzle-components.html does not mention vizzle-core");

const graphMatch = /<script id="graph-data" type="application[/]json">([\s\S]*?)<\/script>/.exec(
  classesHtml,
);
if (!graphMatch) fail("vizzle-classes.html has no graph-data JSON");
let language: { members?: Array<{ name?: string }> } | undefined;
try {
  const graph = JSON.parse(graphMatch[1]);
  language = (
    graph.classes as Array<{ qualified?: string; members?: Array<{ name?: string }> }>
  ).find((c) => c.qualified === "vizzle_core::model::Language");
} catch (err) {
  fail(`graph-data JSON did not parse: ${(err as Error).message}`);
}
if (!language) fail("vizzle-classes.html is missing vizzle_core::model::Language");
if (!(language.members ?? []).some((member) => member.name === "Rust")) {
  fail(
    "Language has no Rust variant — a post-#57 indicator is missing, so this is not this checkout's code",
  );
}

if (fresh.status !== 0)
  fail(`the real doc --check failed (exit ${fresh.status}):\n${output(fresh)}`);
if (stale.status === 0)
  fail("the stale-copy doc --check passed: the drift demo does not catch drift");
if (!output(stale).includes("out of date")) {
  fail(`the stale-copy check failed for the wrong reason (not drift):\n${output(stale)}`);
}

const componentStats = statsOf(components);
const classStats = statsOf(classes);
if (!componentStats) fail("vizzle component printed no stats line");
if (!classStats) fail("vizzle class printed no stats line");
const classCount = Number.parseInt(classStats, 10);
if (!Number.isFinite(classCount) || classCount <= 0)
  fail(`vizzle class reported no classes: "${classStats}"`);

// --- Metadata and the generated payload the page reads. ---

const shaRun = run("git", ["-C", REPO_ROOT, "rev-parse", "--short", "HEAD"]);
const dateRun = run("git", ["-C", REPO_ROOT, "log", "-1", "--format=%cI"]);
const sha = shaRun.stdout.trim();
const commitDate = dateRun.stdout.trim();
if (!sha) fail("could not read the commit SHA");

const payload = {
  sha,
  commitDate,
  checks: {
    fresh: {
      command: "vizzle doc --dir docs/diagrams --check",
      exitCode: fresh.status,
      output: output(fresh),
    },
    stale: {
      command: "vizzle doc --dir <stale-copy> --check",
      exitCode: stale.status,
      output: staleOutput,
      label: "deliberately edited copy",
    },
  },
  diagrams: {
    components: { stats: componentStats },
    classes: { stats: classStats },
  },
};

writeFileSync(join(GENERATED_DIR, "self.json"), JSON.stringify(payload, null, 2) + "\n", "utf8");

console.log(`generate-self — vizzle drawn by vizzle at ${sha}`);
console.log(`  components: ${componentStats}`);
console.log(`  classes:    ${classStats}`);
console.log(
  `  doc --check: ${fresh.status === 0 ? "current" : "STALE"} · stale copy: exit ${stale.status} (out of date)`,
);
