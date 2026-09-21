#!/bin/bash
# Check that committed .github/rulesets/*.json files match the live rulesets
# on GitHub. This is not run in CI: reading rulesets requires a token with
# administration read access, which GITHUB_TOKEN inside Actions cannot have.
# Run this locally with your own authenticated gh.
#
# Exit 0 if all match, non-zero if any drift detected.

set -euo pipefail

# Check for required tools
if ! command -v gh &> /dev/null; then
  echo "error: gh is not installed or not on PATH" >&2
  echo "install from https://github.com/cli/cli" >&2
  exit 1
fi

if ! command -v jq &> /dev/null; then
  echo "error: jq is not installed or not on PATH" >&2
  exit 1
fi

# Check that gh is authenticated
if ! gh auth status > /dev/null 2>&1; then
  echo "error: gh is not authenticated" >&2
  echo "run: gh auth login" >&2
  exit 1
fi

# Check that we can read rulesets (requires admin read)
if ! gh api repos/stiproot/vizzle/rulesets --limit 1 > /dev/null 2>&1; then
  echo "error: gh cannot read rulesets — your token may not have admin read access" >&2
  exit 1
fi

has_drift=false

# Process each committed ruleset file
for ruleset_file in .github/rulesets/*.json; do
  if [[ ! -f "$ruleset_file" ]]; then
    continue
  fi

  name=$(jq -r '.name' "$ruleset_file")

  # Fetch the live ruleset
  live=$(gh api repos/stiproot/vizzle/rulesets --jq ".[] | select(.name == \"$name\")")

  if [[ -z "$live" ]]; then
    echo "DRIFT: $name (live ruleset not found)"
    has_drift=true
    continue
  fi

  # Extract declared fields from both sides, normalize for comparison
  # Extract from committed file
  committed_fields=$(jq -c '{
    enforcement: .enforcement,
    target: .target,
    bypass_actors: (.bypass_actors | sort_by(.actor_id)),
    conditions: .conditions,
    rules: (.rules | sort_by(.type))
  }' "$ruleset_file")

  # Extract from live ruleset
  live_fields=$(echo "$live" | jq -c '{
    enforcement: .enforcement,
    target: .target,
    bypass_actors: (.bypass_actors | sort_by(.actor_id)),
    conditions: .conditions,
    rules: (.rules | sort_by(.type))
  }')

  # Compare
  if [[ "$committed_fields" != "$live_fields" ]]; then
    echo "DRIFT: $name"
    echo "  committed:"
    echo "$committed_fields" | jq '.' | sed 's/^/    /'
    echo "  live:"
    echo "$live_fields" | jq '.' | sed 's/^/    /'
    has_drift=true
  else
    echo "ok: $name"
  fi
done

if [[ "$has_drift" == true ]]; then
  exit 1
fi

echo "all rulesets match"
exit 0
