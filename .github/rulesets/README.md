# Rulesets

GitHub stores rulesets in its own settings, not in the repository, so they are
invisible to anyone reading the code and easy to lose. These files are the
intended configuration, kept here so the rules are reviewable in a diff.

**They are not applied automatically.** Import each one at
*Settings → Rules → Rulesets → New ruleset → Import a ruleset*, or POST it:

```sh
gh api -X POST repos/stiproot/vizzle/rulesets --input .github/rulesets/main.json
gh api -X POST repos/stiproot/vizzle/rulesets --input .github/rulesets/release-tags.json
```

If you edit a ruleset in the web UI, export it back over these files, or they
become a description of something that is no longer true.

## `main.json`

Protects the default branch:

- **No deletion, no force push.** History on `main` is append-only.
- **CI must pass** — `Rust`, `Lint`, `Python 3.10`, `Python 3.13`, `cargo audit`.
  Note the consequence of including `cargo audit`: a newly published advisory
  against a dependency will block merges until it is dealt with. That is the
  intent, and the bypass below is the escape hatch when it lands at a bad time.
- **Repository admins bypass everything** (`RepositoryRole` 5, `always`), so
  pushing straight to `main` keeps working.

Deliberately *not* included: a required pull request. Requiring PRs and then
granting yourself a bypass is ceremony rather than a control. Add the
`pull_request` rule if a collaborator without admin ever gets write access.

Also not included: `required_signatures`. It would reject every unsigned push,
which is real friction to adopt deliberately rather than by surprise.

## `release-tags.json`

Protects `v*` tags — deletion, force-update, and plain update are all blocked.

A published tag is what PyPI's Trusted Publishing trusts and what a user reads
to know which source built their wheel; silently moving one makes that a lie.
Admins bypass, which is what made re-tagging `v0.1.0` possible while the
release pipeline was still being debugged.
