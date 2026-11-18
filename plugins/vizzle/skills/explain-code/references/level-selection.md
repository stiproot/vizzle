# Choosing the level

Answer at the highest level that fully answers the question. When a question
spans levels, answer at the highest and offer the zoom.

| Question sounds like | View | Get it with |
| --- | --- | --- |
| "How does this system fit its environment, who uses it, what does it talk to?" | system context | mermaid `flowchart`, by hand: the code does not know its users |
| "What are the moving parts and how do they talk?" (a repository) | the repo's components | `uvx vizzle component <repo>` (`--weights` for which edges carry load) |
| "One package is really several subsystems" | subpackages as components | `uvx vizzle component <repo> --split <dir>` |
| "How does this one service or package work inside?" | one component, its neighbours | `uvx vizzle component <repo> --around <name> --depth 1` |
| "How is this class, module or data model structured?" | classes | `uvx vizzle class <repo> -I '<scope>/**'` (`--no-members` first) |
| "Which module functions does this file expose?" | classes with module boxes | `uvx vizzle class <repo> -I '<file>' --modules` |
| "What did this branch or PR change?" | a component diff | `uvx vizzle diff <repo> --base main --type component --focus` |
| "Where inside the changed parts did it land?" | the zoom | `uvx vizzle diff <repo> --base main --type component --focus -o shape.mmd --zoom inside.mmd` |
| "What would the proposed change look like?" | planned parts over real code | an overlay: see `design-diagrams` |
| "Where does this run, what is the infrastructure?" | deployment | mermaid `flowchart`, by hand |
| "What happens when X, in what order, who calls whom?" | sequence | mermaid `sequenceDiagram`, by hand |
| "What states can this be in, how does it move between them?" | state | mermaid `stateDiagram-v2`, by hand |

## When not to draw

A question about one function, one value or one line gets prose with a
`path:line` anchor and no diagram. Padding a small answer into the four-part
brief is the same failure as drawing a picture that compresses nothing.

## When vizzle cannot see the code

vizzle parses Python, TypeScript and Rust. For anything else, draw the
structural view as a `flowchart` by hand, ground each box in a path, and say
the diagram was drawn rather than derived.
