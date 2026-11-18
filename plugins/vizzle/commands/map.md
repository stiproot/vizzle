---
description: Map a repository or one of its packages as a component diagram derived by vizzle
---

Treat `$ARGUMENTS` as an optional scope: a directory to diagram, or a component's name as `vizzle component` prints it, for `--around` (default: the whole current repository). A name that matches no component errors and lists the names that exist; pick from that list.

1. Check vizzle can see the code (the `vizzle-diagrams` skill has the one-line check). If it cannot, say so and draw the map by hand as a mermaid `flowchart`, grounding every box in a path.
2. For a whole repository, run `uvx vizzle component <repo> --weights`. For one component, run `uvx vizzle component <repo> --around <component> --depth 1`. If one package holds several subsystems, add `--split <dir>`.
3. If the result holds more than about nine boxes, narrow it (`--around`, `-I`, `-E`) or present an overview plus one zoom, rather than one crowded picture.
4. Show the diagram in a ` ```mermaid ` fence, then finish with:
   - **Walkthrough**: short prose keyed to the diagram's components, with file-path anchors for the ones you discuss.
   - **Where to look next**: two to four files ranked by relevance, and the zoom you would draw next if asked (the `vizzle` command).
