#!/bin/bash
# The version is stated once, in the root Cargo.toml, and everything else
# follows it. This script is how it moves, so the places that follow never
# drift from the one that leads:
#
#   Cargo.toml          [workspace.package] version — the source of truth;
#                       maturin derives the Python package version from it.
#   Cargo.lock          the two workspace crates' entries (cargo refreshes them).
#   plugins/vizzle/.claude-plugin/plugin.json
#                       the agent plugin. Its skill documents this CLI's
#                       surface, and the marketplace installs it from this
#                       repository, so it carries the version of the code it
#                       describes (docs/distribution.md §2.6).
#   .github/workflows/pr-diagram.yml
#                       `vizzle==X.Y.Z` — a pin to the *published* wheel, so it
#                       is deliberately NOT moved by `bump`: the version does
#                       not exist on PyPI until the tag is pushed and the
#                       release has run. Move it afterwards with `pin`.
#
# Usage:
#   scripts/bump-version.sh bump patch|minor|major   step the version everywhere
#   scripts/bump-version.sh pin                      point pr-diagram.yml at the current version
#   scripts/bump-version.sh check                    verify nothing has drifted (run in CI)
#
# `bump` and `pin` only edit files; committing and tagging stay in your hands,
# and the script prints the next steps. See "Releasing" in CLAUDE.md.

set -euo pipefail

cd "$(dirname "$0")/.."

CARGO_TOML=Cargo.toml
CARGO_LOCK=Cargo.lock
PLUGIN_JSON=plugins/vizzle/.claude-plugin/plugin.json
PR_DIAGRAM=.github/workflows/pr-diagram.yml
SEMVER='[0-9]+\.[0-9]+\.[0-9]+'

fail() { echo "error: $*" >&2; exit 1; }

cargo_version() {
  # The first `version =` line is [workspace.package]; dependency tables use
  # `name = "x"` and never a bare `version =`.
  grep -m1 -E '^version = "' "$CARGO_TOML" | sed -E 's/^version = "([^"]+)".*/\1/'
}

plugin_version() {
  grep -m1 -E '^\s*"version":' "$PLUGIN_JSON" | sed -E 's/.*"version": *"([^"]+)".*/\1/'
}

pin_version() {
  grep -m1 -oE "vizzle==$SEMVER" "$PR_DIAGRAM" | sed 's/vizzle==//'
}

lock_versions() {
  # Each workspace crate is a [[package]] block: `name = "..."` then `version = "..."`.
  awk -v RS= '/name = "vizzle-(core|py)"/ { for (i = 1; i <= NF; i++) if ($i == "version") print $(i + 2) }' "$CARGO_LOCK" | tr -d '"'
}

expect_one_match() {
  local pattern=$1 file=$2
  local n
  n=$(grep -cE "$pattern" "$file" || true)
  [ "$n" -eq 1 ] || fail "expected exactly one line matching '$pattern' in $file, found $n"
}

next_version() {
  local current=$1 part=$2
  IFS=. read -r major minor patch <<<"$current"
  case "$part" in
    major) echo "$((major + 1)).0.0" ;;
    minor) echo "$major.$((minor + 1)).0" ;;
    patch) echo "$major.$minor.$((patch + 1))" ;;
    *) fail "bump takes patch, minor or major; got '$part'" ;;
  esac
}

cmd_bump() {
  local part=${1:-}
  [ -n "$part" ] || fail "usage: $0 bump patch|minor|major"
  local current new
  current=$(cargo_version)
  new=$(next_version "$current" "$part")

  expect_one_match '^version = "' "$CARGO_TOML"
  expect_one_match '^\s*"version":' "$PLUGIN_JSON"
  sed -i -E "0,/^version = \"$SEMVER\"/s//version = \"$new\"/" "$CARGO_TOML"
  sed -i -E "s/^(\s*\"version\": *)\"$SEMVER\"/\1\"$new\"/" "$PLUGIN_JSON"
  # Refresh only the workspace crates' entries; dependencies stay where they are.
  cargo update --workspace --quiet

  cmd_check --after-bump
  echo "bumped $current -> $new"
  echo
  echo "next:"
  echo "  git commit -am 'Release $new' && git tag v$new && git push && git push origin v$new"
  echo "  # once release.yml has published to PyPI:"
  echo "  $0 pin"
}

cmd_pin() {
  local version current
  version=$(cargo_version)
  current=$(pin_version)
  if [ "$current" = "$version" ]; then
    echo "pr-diagram.yml already pins vizzle==$version"
    return
  fi
  sed -i -E "s/vizzle==$SEMVER/vizzle==$version/g" "$PR_DIAGRAM"
  echo "pinned pr-diagram.yml: vizzle==$current -> vizzle==$version"
  echo
  echo "next: commit it. The pin must point at a version that exists on PyPI,"
  echo "so this belongs on its own branch after the release has published."
}

cmd_check() {
  local version plugin pin lock status=0
  version=$(cargo_version)
  plugin=$(plugin_version)
  pin=$(pin_version)
  [[ "$version" =~ ^$SEMVER$ ]] || fail "Cargo.toml version '$version' is not X.Y.Z"

  if [ "$plugin" != "$version" ]; then
    echo "drift: $PLUGIN_JSON says $plugin, Cargo.toml says $version" >&2
    status=1
  fi
  for lock in $(lock_versions); do
    if [ "$lock" != "$version" ]; then
      echo "drift: $CARGO_LOCK has a workspace crate at $lock, Cargo.toml says $version (run: cargo update --workspace)" >&2
      status=1
    fi
  done
  # The pin lags the version between a bump and `pin`; that window is the
  # release itself, so it is reported but never fails the check.
  if [ "$pin" != "$version" ]; then
    echo "note: $PR_DIAGRAM pins vizzle==$pin while Cargo.toml says $version — run '$0 pin' once $version is on PyPI"
  fi

  if [ "$status" -eq 0 ]; then
    [ "${1:-}" = "--after-bump" ] || echo "versions agree: $version (pr-diagram pin: $pin)"
  fi
  return "$status"
}

case "${1:-}" in
  bump) cmd_bump "${2:-}" ;;
  pin) cmd_pin ;;
  check) cmd_check ;;
  *) sed -n '2,25p' "$0" | sed 's/^# \{0,1\}//'; exit 2 ;;
esac
