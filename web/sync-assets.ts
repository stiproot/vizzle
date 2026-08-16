// Copy vendored JS from bun-managed node_modules into the Python package's
// assets, where the CLI inlines them into generated HTML pages.
//
// Run after `bun install` or after bumping a dependency:
//   bun run sync-assets

import { copyFileSync, readFileSync } from "node:fs";
import { join } from "node:path";

const here = import.meta.dir;
const assets = join(here, "..", "packages", "vizzle-cli", "src", "vizzle_cli", "assets");

const jobs: Array<[string, string]> = [
  [join(here, "node_modules", "d3", "dist", "d3.min.js"), join(assets, "d3.v7.min.js")],
];

for (const [src, dest] of jobs) {
  copyFileSync(src, dest);
  const firstLine = readFileSync(dest, "utf-8").split("\n", 1)[0];
  console.log(`synced ${dest}\n  ${firstLine}`);
}
