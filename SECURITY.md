# Security Policy

## Reporting a Vulnerability

Report vulnerabilities privately via GitHub's advisory form, not a public
issue: https://github.com/stiproot/vizzle/security/advisories/new. Private
reporting lets a fix land before disclosure.

## Supported Versions

Only the latest release receives security fixes; upgrade before reporting.

## Scope

vizzle parses source code and generates self-contained HTML diagrams. It does
not execute the code it parses. The security surface is:

- **Parsing untrusted source**: source files are parsed, never executed or
  imported. Names from the source (identifiers, manifest `name` fields,
  directory names) are data in every output: entity-encoded in Mermaid, and
  embedded as JSON that is escaped for the script element it sits in, so a
  hostile name cannot inject a diagram statement, a directive, or markup.
- **Generated output**: HTML pages that inline data derived from the parsed
  source, with no external dependencies at view time. Everything the page
  draws goes through DOM text and attribute setters, never `innerHTML`.
- **Git**: `vizzle diff` and `vizzle serve --diff` run `git` in the analysed
  repository. Revisions from `--base`/`--head` are passed after
  `--end-of-options`, so an option-shaped value is a bad revision rather than
  a git flag. Git itself honours that repository's own `.git/config`, as it
  does for any command run there; a repository received as an archive with
  its `.git/` inside deserves the same caution as running `git log` in it.
- **`vizzle serve`**: an HTTP server bound to `127.0.0.1` by default, serving
  the diagram and a reload event stream and nothing else — no request ever
  names a file. On a loopback bind it rejects requests whose `Host` header is
  not local, so a web page cannot read the diagram by DNS rebinding.
  `--host` with a non-loopback address publishes the repository's structure
  to that network, and the command says so when started that way.
- **`vizzle render`**: the one place vizzle runs code it did not ship. It
  looks for `mmdc` on `PATH` and otherwise runs `@mermaid-js/mermaid-cli`
  through `bunx` or `npx -y`, which fetches the package (and, unless a browser
  is found, a Chrome build) from the npm registry at run time, unpinned. Put
  a vetted `mmdc` on `PATH` where that is not acceptable.

If a report concerns a specific part of this surface, include the details in
the private report.
