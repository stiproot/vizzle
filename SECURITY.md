# Security Policy

## Reporting a Vulnerability

If you discover a security vulnerability in vizzle, please report it privately using GitHub's vulnerability reporting feature rather than opening a public issue. You can report a vulnerability at:

https://github.com/stiproot/vizzle/security/advisories/new

Private reporting allows us to investigate and address the issue before public disclosure.

## Supported Versions

Only the latest released version of vizzle receives security fixes. If you are using an older version, please upgrade to the latest release.

## Scope

vizzle parses source code it is pointed at and generates self-contained HTML diagrams. It does not execute the code it parses.

The main security surface is:

- **Parsing untrusted source code**: vizzle safely parses Python and TypeScript code without executing it. The parser is resilient to malformed or adversarial input.
- **Generated diagram output**: vizzle generates fully self-contained HTML pages that inline all data derived from the parsed source. These pages contain no external dependencies and are safe to share and view in any browser.

If you have concerns about a specific aspect of vizzle's security, please include the details in your private report.
