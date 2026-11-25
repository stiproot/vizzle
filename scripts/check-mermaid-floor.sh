#!/usr/bin/env bash
# Render vizzle's class-diagram fixtures on the oldest and newest Mermaid we support.
#
# Why this exists: Mermaid accepts `classDef` in a classDiagram only from 11.4.1 and
# rejects the whole diagram before that, so vizzle colours class diagrams with `style`
# lines, which render from 10.7.0 (docs/plans/impl/class-style-statements.md). The core
# test `assert_class_diagram_floor` guards the shape in CI; this script proves the
# rendering, which needs Chrome and a network, so it is run by hand, not in CI.
#
#   bash scripts/check-mermaid-floor.sh
#
# mermaid-cli 10.9.1 bundles Mermaid 10.9 (in the 10.7–11.3 window where classDef
# fails); 12.0.0 is the version vizzle render pins. The control fixture is the old
# classDef form and must FAIL on the old Mermaid, which proves the check can fail.
set -euo pipefail

main() {
  local here fixtures out cli status=0
  here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
  fixtures="$here/tests/fixtures/mermaid-floor"
  out="$(mktemp -d)"
  # Expanded now: `out` is local to main and gone when the EXIT trap runs.
  # shellcheck disable=SC2064
  trap "rm -rf '$out'" EXIT

  for cli in 10.9.1 12.0.0; do
    for f in "$fixtures"/*.mmd; do
      local name want got
      name="$(basename "$f" .mmd)"
      want=renders
      if [[ "$name" == control-* && "$cli" == 10.* ]]; then want=fails; fi
      if npx -y -p "@mermaid-js/mermaid-cli@$cli" mmdc -q -i "$f" -o "$out/$name-$cli.svg" >"$out/log" 2>&1 \
        && ! grep -q 'Syntax error in' "$out/$name-$cli.svg"; then
        got=renders
      else
        got=fails
      fi
      if [[ "$got" == "$want" ]]; then
        printf 'ok    mermaid-cli %-7s %-24s %s\n' "$cli" "$name" "$got"
      else
        printf 'FAIL  mermaid-cli %-7s %-24s %s, expected %s\n' "$cli" "$name" "$got" "$want"
        sed 's/^/      /' "$out/log" | head -5
        status=1
      fi
    done
  done
  return "$status"
}

main "$@"
