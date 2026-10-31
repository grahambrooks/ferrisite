# CLAUDE.md

ferrisite is a pure-Rust static site generator, built first to replace Hugo for
www.grahambrooks.com (source: `~/dev/projects/blogging/hugo-blog-migration`,
remote `grahambrooks/grahambrooks.com`). **Read DESIGN.md before changing
anything**; it records scope, non-goals and open questions.

## Rules

- Run `make pre-commit` before every commit; commit only after it prints
  `All pre-commit checks passed.` Fix failures at the root; no `#[allow]` or
  skipped hooks.
- Single crate, directory modules under `src/`. No workspace.
- Pure Rust: never shell out to asciidoctor, plantuml, node or a browser.
  Diagrams go through `mmd`, `puml`, `forge-dsl` and `diagram-scene`.
- Output must be deterministic: sort anything collected in parallel or from a
  map before writing it.
- Unsupported input fails the build with file and line. Never drop content
  silently.
- Anything in DESIGN.md section 2 (non-goals) stays out unless Graham changes
  the design.
- Trunk-based: commit to `main`.
