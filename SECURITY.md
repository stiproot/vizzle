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
  imported.
- **Generated output**: HTML pages that inline data derived from the parsed
  source, with no external dependencies at view time.

If a report concerns a specific part of this surface, include the details in
the private report.
