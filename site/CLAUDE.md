# site/ — vizzle website

This directory is a standalone Astro 7 + Tailwind v4 + Firebase Hosting static site.

## Commands

```sh
cd site

# Development
bun run dev            # Start dev server on http://localhost:3000
bun run preview        # Preview production build locally

# Verification (run all three before committing)
bun run lint           # oxlint + oxfmt + check-tokens
bun run type-check     # astro check
bun run build          # Build site to dist/

# Maintenance
bun run format         # Format code with oxfmt
bun run clean          # Remove dist/, .astro/, build artifacts
```

## Steering rules

### Design tokens

All colors, spacing, typography go in `src/styles/tokens.css`. Use `var(--*)`
everywhere else. The `check-tokens.ts` guard fails the build on hardcoded hex values,
font sizes, border-radius, etc. It is part of the `lint` script.

### Diagrams

There are two galleries. The h example diagrams (`h-components.html`,
`h-classes.html`, `h-diff.html`) live in `../examples/` and are copied to
`dist/diagrams/` during the `prebuild` step (which runs before `astro build`).

The vizzle self diagrams are different: they are regenerated from THIS
repository's own checkout on every build, so the site always shows the commit
being deployed. `scripts/generate-self.ts` (also run by `prebuild`, after
`prebuild.ts`) runs vizzle via `uv run --project ..` from `site/`:

- `vizzle component <repo root> -o public/diagrams/vizzle-components.html`
- `vizzle class <repo root>/crates -l rust -o public/diagrams/vizzle-classes.html`
- `vizzle doc --dir <repo root>/docs/diagrams --check` — the real managed-doc check
- the same check against a temp copy of `docs/diagrams/core-model.md` with one
  member row inside its generated fence deliberately edited (the stale demo)

It writes `src/generated/self.json` (commit sha + date, both checks' output and
exit codes, and each diagram's stats line) for `index.astro` to render. Both
generated diagrams and `src/generated/` are gitignored. The build fails if:

- Any diagram file is missing or does not contain `<svg` (empty or corrupt)
- The component diagram does not mention `vizzle-core`
- The real `doc --check` is non-zero
- The stale demo is zero, or its output does not contain `out of date` (a copy
  that fails for malformation would exit non-zero with an error, not drift)
- The Rust class graph lacks `vizzle_core::model::Language` with its post-#57
  `Rust` variant (guards against rendering from a stale extension)

The self diagrams are regenerated per deploy; in CI `.github/workflows/site.yml`
builds vizzle from the checkout with `uv sync --reinstall-package vizzle` first.
The managed doc `docs/diagrams/core-model.md` must stay current, and the `Lint`
CI job runs `vizzle doc --dir docs/diagrams --check` to gate that. The hero
eyebrow advertises the Rust engine: "UML for git · Python + TypeScript + Rust".

### Node version

Node ≥ 22.12 is required (see `package.json` engines field and
`.github/workflows/site.yml`). The site will not build on older versions.

## Gotchas

1. **@theme static, not @theme**. Tailwind v4 with `@theme` (without `static`)
   loads no tokens — utilities emit nothing, the page looks blank. This one
   sentence catches a green build that ships empty.

2. **Space variables are `--space-*`, not `--spacing-*`**. Tailwind v4 does not
   auto-generate the latter. Use the former from `tokens.css` in `@theme static`.

3. **Node 22 in CI**. The GitHub Action runs on the version named in
   `.github/workflows/site.yml` line `node-version`. If it falls behind
   22.12, Astro 7 will refuse to build.

4. **No CSS imports in `.astro` files** (only in pages). Astro scopes imports to
   components; styles leak to global scope unexpectedly. Global CSS lives in
   `src/styles/global.css` and is imported once by `BaseLayout.astro`.

5. **Diagram iframes are fully self-contained**. They include d3, CSS, and SVG
   inline. They will render even offline. Do not add any runtime fetches to the
   iframe `src` attribute (no query params, no API calls).

## Structure

- `src/pages/index.astro` — The gallery page: h examples and the self diagrams, plus the live `doc --check` output. Renders via `DiagramTabs.astro`.
- `src/components/DiagramTabs.astro` — Shared tabbed diagram gallery (ARIA tabs, lazy iframes, keyboard nav, command strip). Used by both the h and self galleries; a new gallery uses this component, not a copied page.
- `src/pages/robots.txt.ts` — Robots.txt endpoint (driven by INDEXABLE flag).
- `src/layouts/BaseLayout.astro` — Page shell (head, body, meta tags).
- `src/config/site.ts` — Site constants (SITE_URL, INDEXABLE).
- `src/styles/tokens.css` — Design tokens (colors, spacing, typography).
- `src/styles/global.css` — Global styles (imports, resets, utilities).
- `scripts/prebuild.ts` — Copy h diagrams from `../examples/` before build.
- `scripts/generate-self.ts` — Draw vizzle's own diagrams from the checkout, run both `doc --check`s, and write `src/generated/self.json`; fails the build on drift.
- `scripts/check-tokens.ts` — Guard script: fail on hardcoded design values.
- `astro.config.mjs` — Astro config (output, integrations, vite plugins).
- `firebase.json` — Firebase Hosting config (site name, cache headers).
- `.oxlintrc.json` — Oxlint config (ignore patterns).
- `tsconfig.json` — TypeScript strict config (path aliases `~/*` → `src/*`).

## Firebase setup

The site deploys to `stiproot-vizzle` Firebase Hosting site. Before the first
deploy, you must:

1. Create the Firebase project `stiproot-vizzle` at console.firebase.google.com
2. Create a Hosting site named `stiproot-vizzle` in that project
3. Add GitHub secret `FIREBASE_SERVICE_ACCOUNT` (service account JSON key)
4. Add GitHub secret `FIREBASE_PROJECT_ID` (the project ID)
5. `GITHUB_TOKEN` is automatic

The workflow at `.github/workflows/site.yml` reads these secrets and deploys
on push to `main` when `site/**`, `examples/**`, `crates/**`, `packages/**`,
`docs/diagrams/**`, or the workflow file changes.

## The Install section's version

`scripts/generate-self.ts` reads the released version from PyPI's JSON API at build time, never from
`Cargo.toml` (main can be ahead of the last release, and the page must not name a version `uvx vizzle`
cannot fetch). PyPI caches that endpoint for 15 minutes, and the site only picks up a release on its
next deploy; if PyPI is unreachable the section omits the number rather than guessing.
