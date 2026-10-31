# ferrisite design

Status: **M4 implemented, with grahambrooks.com's templates ported, 2026-10-31;
one M3 criterion blocked on puml.**

**Template port (section 8), 2026-10-31:** grahambrooks.com's Hugo layouts
and the Compose theme partials they use are minijinja templates in the site's
`templates/`. The theme's compiled CSS and fingerprinted JS are vendored
under Hugo's file names (`ferrisite-static/`), so URLs match.

- Parity item 3 now compares links on **whole pages**, every page Hugo
  generates (254), with no content regions: 0 differences. Links only
  ferrisite has are listed with reasons (`ignore_links`: the generated
  stylesheet, Markdown alternates). Script contents are no longer scanned for
  links. The only ignored reference files are Hugo's social cards.
- Token-level comparison of every page (tags, attributes and text, entities
  decoded, whitespace collapsed, code blocks set aside): all non-post pages
  match except list summaries truncated at 200 characters, where Hugo counts
  goldmark's entity spelling (`&rsquo;` is 7 characters) and ferrisite the
  character, and the home page's itemprop description (its automatic summary,
  section 6.1). Posts differ only in Markdown rendering already accepted in
  M1–M3 (smart quotes, autolinks, table alignment), diagrams and
  highlighting. Screenshots of home, a post, `/post/`, a tag page, `/tags/`,
  `/projects/`, a project and `/series/` are pixel-identical apart from those
  summaries and code blocks: the theme's script adds language labels and
  shell prompts only to Chroma's markup (`data-lang`).
- Added for templates (TEMPLATES.md): `current.kind/section/title/date/
  lastmod/canonical`, `page.toc`, `page.terms`, `page.prev/next`, term
  objects with `params`, `weight`, `pages`, `pages_by_date`; pager `first`,
  `last`, `pages`; the `markup/heading.html` hook; Hugo-exact `plainify`,
  `html_unescape`, `truncate`, `jsonify`; `[sections.*] order`; `build
  --base-url`; a 404 card. `.WordCount` and `.ReadingTime` now follow Hugo
  (inline tags no longer split words; 212 words a minute, rounded up).

**M3 results:**

- Diagrams: Mermaid (`mmd`), PlantUML (`puml`) and forge (`forge-dsl`) render at
  build time from Markdown fences and AsciiDoc blocks, each laid out once and
  painted in both palettes (decision O2), with every SVG `id` prefixed per copy
  so the light and dark copies can't resolve each other's markers. All 18
  Mermaid blocks on grahambrooks.com (13 published, 5 in drafts) render;
  unsupported syntax fails the build naming the block's file line.
- Highlighting: syntect with the pure-Rust regex engine, class-based, one
  generated stylesheet (`[markup] stylesheet`, exposed to templates as
  `site.stylesheet`) with light rules and dark rules under
  `html[data-mode="dark"]` plus an OS-preference fallback. Markdown fences, the
  `highlight` shortcode and AsciiDoc `[source]` blocks share it.
- AsciiDoc: `.adoc` content through `adoc`; diagram and source blocks are
  swapped for passthrough blocks before conversion; error diagnostics fail
  the build. Parity with the Hugo build still passes; two builds identical.
- Errors from shortcodes, diagrams and AsciiDoc name file lines (frontmatter
  counted).
- **Blocked:** the 7 event-driven architecture files, built in a scratch site
  (they aren't in `content/` yet): 4 of 7 build, using fixes to `puml` that
  are local and unpublished; 3 need `puml` features it lacks.
  - Fixed locally in `~/dev/projects/puml`, pending Graham: `skinparam
    <element> { … }` blocks never parsed in sequence diagrams (the rule
    stopped at `" }"`), and `autonumber start step "format"` (start kept,
    format ignored, step other than 1 rejected).
  - Missing: sequence `box "…"` participant groups (4 diagrams) and labelled
    class relations `A -- B : label` (1 diagram).
- Worked around in ferrisite, to be fixed upstream: `adoc` doesn't substitute
  attribute references in `include::` targets (`include::{code}/x.clj[]`);
  ferrisite substitutes header-defined attributes first. The files' code
  samples (`_code-samples/`) survive only under the site repo's `jekyll-final`
  tag.
- `puml` diagram kinds without a scene (use case, timing, mind map, Gantt) use
  puml's own SVG, light palette only, in both slots.

Earlier milestone, M2: `ferrisite parity` passes against Hugo's
build of grahambrooks.com (details under "M2 results" below).

**M2 results** (Hugo 0.169 build vs ferrisite build of the same commit):

- 1. Missing files: 0 of 1,340 compared. 215 reference files are ignored by the
  site's `parity.toml`, each with its reason: 211 social cards and the theme's
  fingerprinted CSS/JS (all M4).
- 2. Alias targets: 0 mismatches across ~700 redirect stubs (legacy
  WordPress/Jekyll URLs, 522 retired tags, `page/1/` stubs).
- 3. Content links: 0 differences across 146 posts; 0 new broken links among
  1,491 checked. 8 links are broken identically in the Hugo build (old
  `wp-content` images, one bad fragment) and are reported, not failed.
- 4. Feeds: 67 feeds, identical items in identical order. Of 147 home-feed
  descriptions, 129 are identical; 13 differ only by the theme's heading-anchor
  markup (M4); 5 differ in text (summary word counting around code blocks and
  lists, and one closing quote goldmark leaves straight). Not failures; see
  section 6.1.
- Two builds byte-identical; `/rss.xml` == `/index.xml`; full build ~0.5 s (debug).

Scope adjustments made during M2 (recorded rather than silent):

- Parity item 3 compares links **inside the post body** only, with same-page
  fragment links skipped, because navigation, tag lists, series/project boxes
  and heading anchors are template features ported in M4. The broken-link
  check covers every link on every page regardless.
- Site-specific shortcodes (`block`, `column`, `grid`, `button`, `tip`,
  `recent-posts`, `newsletter`) are minijinja templates in the site's
  `templates/shortcodes/`, not built into ferrisite. Only `ref`, `relref` and
  `highlight` are built in. This replaces section 6.3's "no user-defined
  shortcodes in v1".
- Render-hook equivalents (inline local SVGs, table wrapper) are config options
  under `[markup]`, applied to Markdown images only (raw HTML `<img>` is left
  alone, as with Hugo's hooks).

Earlier milestone, M1: `ferrisite build` renders grahambrooks.com's posts,
plain pages and paginated section lists with built-in templates. Open question O4
(Content-Signal values) remains.

M1 results against the Hugo 0.169 build of the same commit:

- The same 146 posts are rendered (drafts and future-dated pages excluded, as Hugo).
- Heading IDs: the 409-heading golden test passes, and all 413 headings across
  whole posts match Hugo's output.
- Summaries: 131 of 146 match Hugo's feed descriptions word for word. The other
  15 all contain a `{{< ref >}}` shortcode, which M2 expands.
- Two builds are byte-identical. A full build takes about 130 ms (debug).

ferrisite is a pure-Rust static site generator for one known site first,
www.grahambrooks.com, currently built with Hugo 0.169 from
`grahambrooks/grahambrooks.com`. It renders Markdown and AsciiDoc with Graham's
own tools (`adoc`, `mmd`, `puml`, `forge`), produces output that a parity
harness can prove matches the Hugo build, and makes the site's content easy for
AI agents to find and read.

## 1. Goals

1. **Replace Hugo for grahambrooks.com without breaking a URL.** Every URL the
   Hugo build serves today (pages, 524 legacy alias stubs, 522 tag redirects,
   paginated lists, feeds) is served by ferrisite. A parity harness proves it
   before the deploy switches (milestone M5).
2. **Diagrams rendered at build time by Graham's tools.** No browser-side
   Mermaid, no Java, no Node. This removes the jsDelivr script and the
   reload-on-theme-toggle workaround in today's `baseof.html`.
3. **Agent-readable by construction.** Every page also exists as clean
   Markdown, discoverable by convention, alongside `llms.txt`, structured data
   and feeds (section 9).
4. **Deterministic.** The same inputs give byte-identical output, checked in CI
   by building twice and diffing, as the Hugo build is checked today.
5. **Fail loudly.** Unsupported diagram syntax, unknown shortcodes, broken
   internal links and unresolved `ref`s fail the build with file and line
   (decision C in the site's `PLAN.md`). Nothing is silently dropped.

## 2. Non-goals (deliberately not built)

Recorded so a later session doesn't "complete" them:

- **Go templates or Hugo layout compatibility.** Templates are rewritten in
  minijinja. Content stays Hugo-compatible during the transition; layouts don't.
- **A Sass compiler.** The site's compiled theme CSS is vendored once into the
  site repo as plain CSS. (`grass` exists if this is ever revisited.)
- **A plugin API.** New behaviour is a code change in ferrisite. `adoc`
  already defers extensions to stdio filters; ferrisite follows the same
  restraint.
- **Multilingual sites, menus-as-data beyond one main menu, theme
  inheritance, Hugo modules, image processing beyond social cards.**
- **Server-side behaviour.** Output is static files; anything needing HTTP
  headers or content negotiation is documented as a host or CDN setting
  (section 9.6), not generated.
- **Being a general Hugo replacement.** Features exist because
  grahambrooks.com uses them (inventory in section 4).

Prior art in this portfolio: `mdbook-blog` (an mdBook build of the same site).
mdBook is a book generator: no taxonomies, feeds, pagination or aliases, which
is why it isn't the base here.

## 3. Constraints

- **Pure Rust, no runtime or environment dependencies** (vault:
  backstage-rs-pure-rust-no-runtime-deps). One static binary. No asciidoctor,
  no plantuml.jar, no Node.
- **Single crate with directory modules** (vault:
  casual-review-single-crate-layout).
- **CalVer, in-repo Homebrew formula, GitHub Actions release binaries;
  crates.io publish manual** (vault: graham-release-and-distribution-conventions).
- **`make pre-commit` before every commit**: fmt, clippy `-D warnings`, test.
- **Content portability during transition.** Until the Hugo fallback job is
  retired (M5 + one release), every content file must still build under Hugo.
  So ferrisite reads Hugo's frontmatter and shortcode syntax rather than
  inventing its own.

## 4. What grahambrooks.com actually uses (inventory, 2026-10-31)

| Area | In use |
|---|---|
| Content | 215 files: 122 `.md`, 93 `.markdown`; 0 `.adoc` today (7 to migrate). 212 YAML frontmatter, 3 TOML. |
| Frontmatter keys | `title` 211, `date` 163, `tags` 150, `aliases` 143, `categories` 115, `type` 91, `draft` 51, `description` 37, `toc` 29, `projects` 27, `excerpt` 23, `summary`/`repo`/`group`/`weight` 18 (project terms), `series` 15, `release` 11, `image` 10, `install` 9; singletons `build.list`, `hideDescription`, `hideFooterNewsletter`, `outputs`, `searchPage`, `groups`. WordPress leftovers under `meta.*` are ignored. |
| Taxonomies | `tags` (27 terms), `categories` (10), `series` (3), `projects` (18). Term content files carry `title`, `aliases` and term params. |
| URLs | `disablePathToLower` (case preserved); slug = filename; `aliases` (meta-refresh stubs); pagination `/page/N/` at 10; terms with spaces become `-`. |
| Outputs | Home: HTML, RSS `/index.xml`, `/rss.xml` (byte-identical), `/essays.xml`, `/index.json` (search). Sections: HTML + RSS. Taxonomies and terms: HTML + RSS. `sitemap.xml`, `robots.txt`, `404.html`. |
| Shortcodes | `ref` 55, `highlight` 32, `column` 8, `block` 4, `tip` 3, `button` 3, `grid` 1, `recent-posts` 1, `newsletter` 1. |
| Render hooks | Mermaid code blocks, images (local SVGs inlined so `currentColor` follows the theme), tables (wrapped for overflow). |
| Diagrams | 18 Mermaid blocks in 7 files (`flowchart`, `C4Context`); 27 PlantUML blocks in the 7 AsciiDoc files waiting to migrate. |
| Other | Social card per page (title over a base image); JSON-LD; canonical links; reading time; table of contents; prev/next split by `release`; GoatCounter and Buttondown via template snippets. |

## 5. Pipeline

Stages with a data type at each boundary (the md2pdf pattern):

```
files ─discover→ Source[] ─parse→ Page[] ─resolve→ Site ─render→ Output[] ─emit→ public/
                                                                      └─verify→ report
```

| Stage | Module | Input → output | Notes |
|---|---|---|---|
| discover | `content::discover` | site root → `Source { path, kind, raw }` | Walks `content/`, `static/`. Sorted, for determinism. |
| parse | `content::frontmatter` | `Source` → `Page { meta, body, kind }` | YAML (`---`) and TOML (`+++`). Unknown keys kept in `params`. |
| resolve | `site` | `Page[]` → `Site` | Sections, taxonomies (with term files), URLs, aliases, pagination, summaries, `ref` targets, prev/next, series order. All cross-page facts are computed here, never during rendering. |
| render | `markup`, `render` | `Site` → `Output[]` | Markup to HTML fragments, then minijinja templates. Pages render in parallel (rayon); output is collected and sorted by path. |
| emit | `output` | `Output[]` → files | Feeds, sitemap, robots, search index, agent files (section 9), alias stubs, social cards, static copy. |
| verify | `check` | `public/` → report | Internal link check, unresolved anchors; optional double build. |

## 6. Markup

### 6.1 Markdown: comrak

comrak, as in `md2pdf` and backstage-rs, configured to match Hugo's goldmark
defaults so existing posts render the same: tables, strikethrough, autolink,
task lists, footnotes, description lists, **smart punctuation** (goldmark's
typographer), raw HTML allowed (`unsafe: true` today), header IDs.

Two parity details matter because existing links depend on them:

- **Heading IDs** must reproduce Hugo's `anchorize` rules (lowercase, spaces
  to `-`, punctuation dropped), or `#fragment` links and the table of contents
  break. A golden test pins this against Hugo's output for every heading in the
  corpus.
- **Summaries** (known gap: 5 of 147 home-feed summaries end at a different
  paragraph than Hugo's or differ in one quote; refine alongside M4) follow Hugo 0.134+: content before `<!--more-->`, else the
  `summary:` frontmatter, else **whole top-level blocks taken until the word
  count reaches 70** (never a cut paragraph). Checked against Hugo's feed
  descriptions; `description:` is not used for summaries.

Code blocks are dispatched by info string: `mermaid`, `plantuml`/`puml`,
`forge` and `drawio` go to diagrams (section 7); everything else to
highlighting (6.4).

### 6.2 AsciiDoc: adoc

`.adoc` content goes through `adoc` exactly as backstage-rs TechDocs does:
`Preprocessor::with_attributes(..).with_base_dir(..).with_safe_mode(Safe)` →
`parse_with` → `Html5Converter` with no stylesheet, body fragment extracted.
`[plantuml]` and `[mermaid]` blocks are rendered by ferrisite's diagram
module, not by an asciidoctor-diagram equivalent.

### 6.3 Shortcodes

Hugo's `{{< name args >}}` / `{{% %}}` syntax for the nine shortcodes in use,
expanded before Markdown parsing. `ref` resolves against the page index at
resolve time and fails on a missing target. An unknown shortcode is a build
error naming file and line. No user-defined shortcodes in v1.

### 6.4 Syntax highlighting: syntect

Class-based output (`ClassedHTMLGenerator`), with a generated stylesheet for
light and dark. Class names differ from Hugo's chroma classes, so the site's
`chroma.css` is replaced, not reused. Parity is checked on URLs, links and
feeds, not on highlighting markup.

## 7. Diagrams

| Info string | Library | Entry point |
|---|---|---|
| `mermaid` | `mmd` | `mmd::render_with(source, &theme, &measure) -> Result<String, Error>` |
| `plantuml`, `puml` | `puml` | preprocess → `parser::parse` → `render::render` |
| `forge` | `forge-dsl` | model parse → views as `diagram-scene` scenes |
| `drawio` (optional, M3+) | `draws` | `render` module |

All four meet at `diagram-scene`. Output is inline SVG with real `<text>`
(the reason `mmd` exists), plus the scene's accessible title as `<title>`.

**Failure is fatal** (decision C): a parse error fails the build with the
library's message, file and line. There is no client-side fallback.

**Dark mode (decided, O2).** `diagram-scene` themes are fixed palettes
(`Theme::light()`, `Theme::dark()`), and the site switches theme with a
`data-mode` attribute on `<html>` as well as `prefers-color-scheme`. Each
diagram is therefore rendered **twice**, once per palette, into
`<div class="diagram diagram-light">` and `<div class="diagram diagram-dark">`.
CSS shows the one matching `html[data-mode]`, falling back to
`prefers-color-scheme` when the mode is `auto`. This costs roughly double the
diagram bytes and needs no upstream change. A CSS-variable output mode in
`diagram-scene`'s SVG writer would halve that later, without changing the
templates. The Markdown copy (section 9.1) carries the diagram source once,
not the SVG.

## 8. Templates and theme

minijinja templates in the site repo (`templates/`), ported from today's
`layouts/`: base, single post, plain page, post list, term, taxonomy index,
projects hub and project page, series, releases, 404, partials (head, nav,
footer, newsletter, post furniture). Data available to templates is
documented and stable: `site`, `page`, `paginator`, `terms`. No logic beyond
loops and conditionals. Ordering, filtering and grouping happen in `resolve`,
so templates can't make a build non-deterministic.

The Compose look is kept by vendoring the theme's compiled CSS plus the
project's override sheets.

## 9. Agent-readable output

Based on a review of the state of the art on 2026-10-31. Sources are recorded in
section 13. In short: per-page Markdown is what agents use, `llms.txt` is a
cheap convention with little evidence of crawler use, and most discovery
protocols are too early for a static blog.

### 9.1 A Markdown copy of every page (adopt)

For every HTML page, emit `<url>index.md` next to `<url>index.html`
(llms.txt spec v2: directory URLs get `index.md`). The copy is built from the
**source**, not converted back from HTML:

- A short YAML header: `title`, `url` (canonical), `date`, `lastmod`,
  `author`, `tags`, `series`, `description`.
- The body as written, with shortcodes expanded to plain Markdown, relative
  links made absolute, and **diagrams replaced by their source in a fenced
  block** plus the alt text, so an agent reads the diagram's meaning rather
  than SVG.
- AsciiDoc pages get Markdown via the `adoc` AST.

Each HTML page links to it with `<link rel="alternate" type="text/markdown"
href="…/index.md">`. List and term pages get Markdown too: a list of titles,
dates, summaries and `.md` links, which is the cheapest way for an agent to
walk the site.

### 9.2 `/llms.txt` and `/llms-full.txt` (adopt)

`/llms.txt` follows the spec: H1 site title, a blockquote summary from config,
then H2 sections generated from site structure:

- Series, in reading order.
- Projects, each linking to its project page's Markdown.
- Featured posts (from config).
- Recent essays.
- `## Optional`: the full archive by year.

All links point at `.md` copies. `/llms-full.txt` concatenates the essays'
Markdown copies. It's capped by size in config, newest first, because
whole-site dumps get large.

### 9.3 Usage signals in robots.txt (adopt, configurable)

`robots.txt` gets a `Content-Signal:` line (Cloudflare's convention, about 4% of
large sites) with values from config, for example
`search=yes, ai-input=yes, ai-train=no`. The emitter is written so an IETF
aipref `Content-Usage` line can be added when that draft reaches RFC (it's
still an Internet-Draft as of September 2026). The values are Graham's call
(open question O4).

### 9.4 Structured data (adopt; mostly exists today)

JSON-LD `BlogPosting` (`headline`, `datePublished`, `dateModified`, `author`
as `Person` with `sameAs`, `url`, `image`, `keywords`, `wordCount`), plus
`citation` where a post's frontmatter lists sources. `WebSite` on the home
page. Structured data must match visible content. Cloudflare's converter
carries JSON-LD into its Markdown output, so it reaches agents either way.

### 9.5 Feeds and sitemap (adopt)

RSS at `/index.xml` and `/rss.xml` (byte-identical, as now), `/essays.xml`,
plus **Atom** and **JSON Feed** at new URLs. `sitemap.xml` with true
`lastmod`, taken from frontmatter only: `lastmod` if set, else `date` (decided, O5).
No git-history reads, so a `.git`-less build behaves identically.

### 9.6 Host-level settings (documented, not generated)

- **Markdown negotiation**: Cloudflare's "Markdown for Agents" serves
  Markdown for `Accept: text/markdown` on Pro plans and above, and passes
  through an origin's own Markdown. On other plans, a Cloudflare rule or Worker
  can map that `Accept` header to the `.md` copy. In a February 2026 test only
  Claude Code, Cursor and OpenCode sent the header, so the static `.md` copies
  matter more.
- **`Link: <…/index.md>; rel="alternate"; type="text/markdown"`** headers:
  only possible where the host allows custom headers; GitHub Pages doesn't.
- **Content-Type for `.md`**: assumed to be served suitably by the host
  (decided, O3). `text/markdown; charset=utf-8` is the goal; check it once,
  after the first deploy that includes `.md` copies.

### 9.7 Search an agent can use (optional, later)

`/index.json` stays, in its current shape, for the site's search box. A
**separate, optional** remote MCP server (search, list, read-as-Markdown over
that index) could follow later, with a server card once MCP SEP-2127's
`.well-known` path settles. It's not part of the generator.

### 9.8 Skipped, with reasons

NLWeb (no current adoption data found), A2A agent cards (a blog isn't an
agent), `agents.json` (inactive), WebMCP (Chrome origin trial only), RSL and
TDMRep (licensing protocols with little uptake, overkill for a personal blog),
Agent Skills discovery (v0.2 draft; revisit if it matures).

### 9.9 Checking it

`ferrisite check --agents` verifies that every HTML page has a `.md` copy and an
alternate link, that `llms.txt` parses to the spec's structure, and that every
link in it resolves. Lighthouse's Agentic Browsing category and Cloudflare's
isitagentready.com are external checks to run after deploy, not CI gates.

## 10. Social cards

The same design as the current Hugo cards: base image, title, byline, site
name. They're drawn as an SVG template and rasterised with `resvg` (aligned to
`puml`'s version). Text is laid out with `rustybuzz` measurement, the same
approach as `md2pdf`, so wrapping is exact and margins are symmetric. The font
file is OFL-licensed and committed in the site repo. Output is deterministic
PNG.

## 11. Parity harness (`ferrisite parity <reference> <candidate>`)

This is the gate for M2 onward. It compares two built sites:

1. **URL set**: every file in the reference exists in the candidate (HTML,
   XML, JSON, images). Extra files are listed, not failed.
2. **Alias stubs**: each redirect points at the same target.
3. **Internal link graph**: for each HTML page, the set of internal link
   targets (fragments included) matches, after normalising absolute and
   relative forms. Whole pages, or a configured region; links one build has
   by design can be ignored with a reason. Script contents aren't links.
   Broken links in either build are reported.
4. **Feeds**: per feed, the item list (guid, link, title, pubDate) matches in
   order; description differences are reported but don't fail, while summary
   rules settle.
5. **Metadata**: `<title>`, canonical, meta description and `og:*` per page.

The output is a machine-readable report plus a summary. The exit code is
non-zero on any failure in items 1–4.

## 12. Milestones

| | Scope | Done when |
|---|---|---|
| **M0** | This design; repo scaffold; open questions answered. | Graham approves the design. |
| **M1** | discover → parse → resolve → render for posts, plain pages, section lists with pagination; comrak with goldmark-parity options; base templates. | `ferrisite build` renders every post; heading-ID golden test passes against Hugo's output. |
| **M2** | Taxonomies with term files, aliases, all feeds, sitemap, robots, search index, shortcodes; `parity` command. | Parity items 1–4 pass against the Hugo build of the same commit. |
| **M3** | Diagrams (mmd, puml, forge); syntect; dark-mode approach (O2); AsciiDoc via adoc. | All 18 Mermaid blocks render; the 7 migrated AsciiDoc posts build with their PlantUML; any unsupported syntax fails the build. |
| **M4** | Agent-readable outputs (section 9), social cards, `check`, a `serve` command with rebuild on change for the writing loop. | `check --agents` is clean; cards match the current design. |
| **M5** | The site's deploy Action builds with ferrisite; Hugo job kept as a fallback for one release. | The live site is built by ferrisite and parity is clean on that commit. |

## 13. Dependencies (planned)

| Purpose | Crate | Note |
|---|---|---|
| CLI | `clap` 4 | Portfolio standard. |
| Errors | `anyhow`, `thiserror` 2; `miette` for source-located diagnostics | `adoc` already uses `miette`. |
| Frontmatter | `serde_yaml` 0.9.34 (O1), `toml` 1 | Final release of an unmaintained crate; fine for now. |
| Markdown | `comrak` 0.56 | As `md2pdf`. |
| AsciiDoc | `adoc` (git dependency) | |
| Diagrams | `mmd`, `puml`, `forge-dsl`, `diagram-scene` (git), optionally `draws` | |
| Templates | `minijinja` | |
| Highlighting | `syntect` 5 | |
| Cards | `resvg`, `rustybuzz` | |
| Parallelism | `rayon` | Output sorted before writing. |
| Feeds/XML | `quick-xml` | |
| Tests | `insta` | Golden output tests. |

## 14. Decisions and open questions

Decided 2026-10-31:

- **O1. YAML parser:** `serde_yaml`, latest (0.9.34, its final release), for
  consistency with the rest of the portfolio. Revisit if it becomes a problem;
  frontmatter parsing sits behind one module, so swapping is local.
- **O2. Diagram dark mode:** render each diagram in both palettes and switch
  with CSS on `html[data-mode]` (section 7).
- **O3. Content-Type for `.md`:** assume the host serves it suitably; verify
  after the first deploy.
- **O5. `lastmod`:** frontmatter only (`lastmod`, else `date`).
- **O6. Theme:** keep the Compose look, with its compiled CSS vendored into the
  site repo.

Still open:

- **O4. Content-Signal values** for robots.txt (section 9.3), for example
  `search=yes, ai-input=yes, ai-train=no`. Until Graham decides, ferrisite
  emits no `Content-Signal` line.

## Sources (agent-readable output review, 2026-10-31)

Primary sources, as reviewed: llms.txt spec v2 (https://llmstxt.org/); Google
Search on AI features (https://developers.google.com/search/docs/appearance/ai-features);
Cloudflare Markdown for Agents
(https://developers.cloudflare.com/fundamentals/reference/markdown-for-agents/);
Cloudflare agent readiness (https://blog.cloudflare.com/agent-readiness/);
Checkly on agent content negotiation
(https://www.checklyhq.com/blog/state-of-ai-agent-content-negotation/);
IETF aipref documents (https://datatracker.ietf.org/wg/aipref/documents/);
Cloudflare Content Signals (https://blog.cloudflare.com/content-signals-policy);
MCP SEP-2127 (https://github.com/modelcontextprotocol/modelcontextprotocol/pull/2127);
W3C TDMRep (https://w3c.github.io/tdm-reservation-protocol/spec/).
Secondary, treat as indicative: Lighthouse Agentic Browsing coverage
(https://searchengineland.com/google-llms-txt-chrome-lighthouse-478246,
https://www.debugbear.com/blog/lighthouse-agentic-browsing).
