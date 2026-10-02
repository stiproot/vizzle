#!/usr/bin/env bun
/**
 * Copy example diagrams from ../examples/ to public/diagrams/ before build.
 * Fails if any diagram is missing or does not contain <svg.
 */

import { copyFileSync, readFileSync, mkdirSync, existsSync } from "node:fs";
import { join } from "node:path";

const ROOT = new URL("..", import.meta.url).pathname;
const EXAMPLES_DIR = join(ROOT, "../examples");
const PUBLIC_DIR = join(ROOT, "public/diagrams");

const DIAGRAMS = ["h-components.html", "h-classes.html", "h-diff.html"];

// Ensure public/diagrams/ exists
if (!existsSync(PUBLIC_DIR)) {
  mkdirSync(PUBLIC_DIR, { recursive: true });
}

let errors = 0;

for (const diagram of DIAGRAMS) {
  const source = join(EXAMPLES_DIR, diagram);
  const target = join(PUBLIC_DIR, diagram);

  try {
    // Check if file exists
    const content = readFileSync(source, "utf8");

    // Check if file contains <svg
    if (!content.includes("<svg")) {
      console.error(`✗ ${diagram}: does not contain <svg`);
      errors++;
      continue;
    }

    // Copy file
    copyFileSync(source, target);
    console.log(`✓ ${diagram} (${(content.length / 1024).toFixed(1)} KB)`);
  } catch (err) {
    console.error(`✗ ${diagram}: ${(err as Error).message}`);
    errors++;
  }
}

if (errors > 0) {
  process.exit(1);
}

console.log(`prebuild — ${DIAGRAMS.length} diagrams copied`);
