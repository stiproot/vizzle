#!/usr/bin/env bun
/**
 * Fail the build when hardcoded design values are used instead of tokens.
 *
 * The site's look comes from design tokens in src/styles/tokens.css.
 * A literal `#ff6b9d` or `font-size: 24px` still renders correctly today
 * but silently stops tracking the token tomorrow.
 */

import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const ROOT = new URL("..", import.meta.url).pathname;
const TARGET = join(ROOT, "src");

/** tokens.css is the canonical source of design literals — that's where they belong. */
const isTokenFile = (path: string): boolean => path.endsWith("tokens.css");

const EXTENSIONS = [".astro", ".css", ".ts", ".tsx"];

interface Violation {
  file: string;
  line: number;
  text: string;
  rule: string;
}

const DECLARATIONS: Array<{
  name: string;
  property: RegExp;
  ok: (value: string) => boolean;
  hint: string;
}> = [
  {
    name: "font-size",
    property: /font-size:\s*([^;]+)/,
    ok: (v) => v.includes("var(--text-") || v === "inherit",
    hint: "use var(--text-*) from tokens.css",
  },
  {
    name: "border-radius",
    property: /border-radius:\s*([^;]+)/,
    ok: (v) => v.includes("var(--radius-") || v === "0" || v === "50%",
    hint: "use var(--radius-*) from tokens.css",
  },
  {
    name: "font-weight",
    property: /font-weight:\s*([^;]+)/,
    ok: (v) => v.includes("var(--font-weight-") || v === "inherit",
    hint: "use var(--font-weight-*) from tokens.css",
  },
];

const COLOR_RULES: Array<{ name: string; pattern: RegExp; hint: string }> = [
  {
    name: "vizzle-palette-hex",
    pattern:
      /#(?:1f2328|57606a|d0d7de|ffffff|f6f8fa|0969da|ddf4ff|1a7f37|dafbe1|cf222e|ffebe9|bf8700|fff1c2|7d4e00)/i,
    hint: "use var(--color-*) from tokens.css instead of palette hex values",
  },
  {
    name: "hex-color",
    pattern: /(?<![\w)])#(?:[0-9a-fA-F]{3}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})\b/,
    hint: "use var(--color-*) from tokens.css",
  },
  {
    name: "rgb-color",
    pattern: /\b(?:rgba?|hsla?)\(\s*\d/,
    hint: "use var(--color-*) from tokens.css",
  },
  {
    name: "tailwind-default-palette",
    pattern:
      /\b(?:text|bg|border|ring|from|to|via)-(?:gray|slate|zinc|neutral|stone|red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose)-\d{2,3}\b/,
    hint: "use token-based utilities (e.g. text-muted, bg-structure) instead of Tailwind default palette",
  },
];

const UTILITY_RULES: Array<{ name: string; pattern: RegExp; hint: string }> = [
  {
    name: "tailwind-default-spacing",
    pattern:
      /\b(?:[a-z]+:)*(?:p[trblxy]?|m[trblxy]?|gap|space-[xy]|w|h|max-w)-(?:\d+|px|full|screen)(?=[\s"'])/,
    hint: "use a var(--space-*) or var(--measure-*) token-backed arbitrary utility",
  },
  {
    name: "tailwind-default-typography",
    pattern:
      /\b(?:[a-z]+:)*(?:text-(?:xs|sm|base|lg|xl|[2-9]xl)|font-(?:normal|medium|semibold|bold|extrabold))(?=[\s"'])/,
    hint: "use a var(--text-*) or var(--font-weight-*) token-backed arbitrary utility",
  },
  {
    name: "tailwind-default-radius",
    pattern: /\b(?:[a-z]+:)*rounded-(?:sm|md|lg|xl)(?=[\s"'])/,
    hint: "use a var(--radius-*) token-backed arbitrary utility",
  },
];

const ALLOWED_LINE = [
  /^\s*\*/,
  /^\s*\/\//,
  /@media/,
  /mask-image|background-image:\s*url\(/,
  /1px solid/,
];

const walk = (dir: string, out: string[] = []): string[] => {
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) walk(path, out);
    else if (EXTENSIONS.some((ext) => path.endsWith(ext))) out.push(path);
  }
  return out;
};

const violations: Violation[] = [];

for (const file of walk(TARGET)) {
  const rel = relative(TARGET, file);
  if (isTokenFile(rel)) continue;

  const lines = readFileSync(file, "utf8").split("\n");
  lines.forEach((text, index) => {
    if (ALLOWED_LINE.some((allowed) => allowed.test(text))) return;

    const report = (rule: string): void => {
      violations.push({
        file: `src/${rel}`,
        line: index + 1,
        text: text.trim(),
        rule,
      });
    };

    for (const rule of COLOR_RULES) {
      if (rule.pattern.test(text)) {
        report(`${rule.name} — ${rule.hint}`);
        return;
      }
    }

    for (const rule of UTILITY_RULES) {
      if (rule.pattern.test(text)) {
        report(`${rule.name} — ${rule.hint}`);
        return;
      }
    }

    for (const rule of DECLARATIONS) {
      const match = rule.property.exec(text);
      if (match?.[1] && !rule.ok(match[1].trim())) {
        report(`${rule.name} — ${rule.hint}`);
        return;
      }
    }
  });
}

if (violations.length > 0) {
  process.stderr.write(`check-tokens — ${violations.length} hardcoded design value(s):\n\n`);
  for (const v of violations) {
    process.stderr.write(`  ${v.file}:${v.line}\n`);
    process.stderr.write(`    ${v.text}\n`);
    process.stderr.write(`    ${v.rule}\n\n`);
  }
  process.stderr.write("Design tokens belong in src/styles/tokens.css.\n");
  process.exit(1);
}

process.stdout.write("check-tokens — no hardcoded design values\n");
