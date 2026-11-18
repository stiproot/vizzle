---
description: Explain code or a design with one diagram at the right level, grounded in file paths
---

Treat `$ARGUMENTS` as the question, or the code area to explain (a path, a subsystem name, or a free-form question). Follow the `explain-code` skill end to end: pick the level, get the diagram from vizzle where the code can supply it (`vizzle component`, `vizzle class`, `vizzle diff`), draw only what the code cannot show, and answer with the four-part brief:

1. **Answer**: one or two sentences that answer the question directly.
2. **Diagram**: the one primary diagram, at the level the question needs.
3. **Walkthrough**: short prose keyed to the diagram's elements, with `path:line` anchors.
4. **Where to look next**: two to four files ranked by relevance, and the zoom you would draw next if asked.

If the question is about one function or one value, answer in prose with an anchor and no diagram.
