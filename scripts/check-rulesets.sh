#!/bin/bash
# Check that committed .github/rulesets/*.json files match the live rulesets
# on GitHub. This is not run in CI: reading rulesets requires a token with
# administration read access, which GITHUB_TOKEN inside Actions cannot have.
# Run this locally with your own authenticated gh.
#
# The comparison is semantic and one-sided: only the keys the committed file
# declares are compared, recursively. The live API returns fields the file
# does not state (e.g. rules[].parameters.do_not_enforce_on_create), and
# those must not report drift.
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

# Derive the repository from the checkout, so a fork checks itself
repo=$(gh repo view --json nameWithOwner -q .nameWithOwner)

# Check that we can read rulesets (requires admin read).
# The list endpoint only returns summaries (id, name, ...); the fields being
# compared come from the per-id endpoint fetched below.
if ! live_index=$(gh api "repos/$repo/rulesets" 2> /dev/null); then
  echo "error: gh cannot read rulesets on $repo — your token may not have admin read access" >&2
  exit 1
fi

# prune(live; committed): project the live object onto the shape the
# committed file declares. Objects keep only declared keys, recursively.
# Arrays of objects that all carry a "type" (rules) are paired by type;
# other arrays of objects are pruned against the first element's shape and
# sorted; scalar arrays are sorted. Comparing prune(live; c) against
# prune(c; c) is then an exact equality.
prune_def='
  def prune($l; $c):
    if ($c|type) == "object" then
      if ($l|type) != "object" then $l
      else reduce ($c|keys[]) as $k ({}; . + {($k): prune($l[$k]; $c[$k])})
      end
    elif ($c|type) == "array" then
      if ($l|type) != "array" then $l
      elif ($c|length) == 0 then $l
      elif ($c[0]|type) == "object" and ($c | all(has("type"))) then
        [$c[] | . as $ce
          | ([$l[]? | select(.type == $ce.type)] | first) as $le
          | if $le == null then {type: $ce.type, missing: true}
            else prune($le; $ce)
            end]
      elif ($c[0]|type) == "object" then
        [$l[] | prune(.; $c[0])] | sort
      else
        $l | sort
      end
    else $l
    end;
'

has_drift=false

# Process each committed ruleset file
for ruleset_file in .github/rulesets/*.json; do
  if [[ ! -f "$ruleset_file" ]]; then
    continue
  fi

  name=$(jq -r '.name' "$ruleset_file")

  # The list endpoint gives us the id; the full ruleset (bypass_actors,
  # conditions, rules) only comes back from the per-id endpoint.
  id=$(echo "$live_index" | jq -r --arg name "$name" \
    '[.[] | select(.name == $name)] | first | .id // empty')

  if [[ -z "$id" ]]; then
    echo "DRIFT: $name (live ruleset not found)"
    has_drift=true
    continue
  fi

  live=$(gh api "repos/$repo/rulesets/$id")

  result=$(jq -cn --argjson live "$live" --slurpfile cfile "$ruleset_file" "
    $prune_def
    (\$cfile[0] | {enforcement, target, bypass_actors, conditions, rules}) as \$c
    | prune(\$c; \$c) as \$want
    | prune(\$live; \$c) as \$got
    | {match: (\$want == \$got),
       want: \$want,
       got: \$got,
       diffs: ([(\$want, \$got) | paths(scalars)]
         | unique
         | map(. as \$p
             | select((\$want | getpath(\$p)) != (\$got | getpath(\$p)))
             | {path: (\$p | map(tostring) | join(\".\")),
                committed: (\$want | getpath(\$p)),
                live: (\$got | getpath(\$p))}))}
  ")

  if [[ $(echo "$result" | jq -r '.match') == "true" ]]; then
    echo "ok: $name"
  else
    echo "DRIFT: $name"
    if [[ $(echo "$result" | jq '.diffs | length') -gt 0 ]]; then
      echo "$result" | jq -r '.diffs[] | "  \(.path): committed=\(.committed|tojson) live=\(.live|tojson)"'
    else
      echo "  committed:"
      echo "$result" | jq '.want' | sed 's/^/    /'
      echo "  live:"
      echo "$result" | jq '.got' | sed 's/^/    /'
    fi
    has_drift=true
  fi
done

if [[ "$has_drift" == true ]]; then
  exit 1
fi

echo "all rulesets match"
exit 0
