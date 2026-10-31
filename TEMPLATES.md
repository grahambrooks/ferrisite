# Templates

ferrisite renders pages with [minijinja](https://docs.rs/minijinja) templates.
Built-in defaults live in `src/templates/`; a site overrides any of them by
putting a file with the same name in its own `templates/` directory.

## Lookup

| Page | Templates tried, in order |
|---|---|
| Regular page in section `post` | `post/single.html`, `single.html` |
| Top-level page (`content/about.md`) | `page.html` |
| Section list (`/post/`, `/post/page/2/`) | `post/list.html`, `list.html` |
| Term page (`/tags/rust/`) | `tags/term.html`, `term.html` |
| Taxonomy index (`/tags/`) | `tags/taxonomy.html`, `taxonomy.html` |
| Home page | `home.html` |
| Not found | `404.html` (written to `/404.html`) |
| robots.txt | `robots.txt` |
| Shortcode `{{< name >}}` | `shortcodes/name.html` (no built-in fallback) |
| Markdown heading | `markup/heading.html`, if present (Hugo's heading render hook) |

Templates can `{% extends "base.html" %}` and override blocks.

## Context

All ordering, filtering and pagination happen before rendering, so templates
only loop and print.

**`site`**: `title`, `base_url`, `author`, `description`, `copyright`,
`language`, `params` (the `[params]` table from `ferrisite.toml`),
`stylesheet` (absolute URL of the generated stylesheet for syntax
highlighting and diagram light/dark switching; link it from your base
template).

**`current`**: the page being rendered, whether or not it has a content file
(every page template, including `404.html`):

| Field | Notes |
|---|---|
| `url`, `permalink` | This output page (`/post/page/2/` on a paginated list) |
| `canonical` | Permalink of the first page of a paginated list; else `permalink` (Hugo's `.Permalink`) |
| `markdown_url` | Absolute URL of the page's Markdown copy (the first page's for lists); none on the 404 page |
| `card` | Absolute URL of the page's social card, when cards are enabled |
| `kind` | `home`, `section`, `page`, `taxonomy`, `term` or `404` |
| `section` | Section name; the taxonomy name on taxonomy and term pages; empty for home, top-level pages and 404 |
| `title` | Page, section, taxonomy or term title; the site title on the home page; `404 Page not found` |
| `date`, `lastmod` | RFC 3339. A list page without its own takes its newest page's, as Hugo does: home from every listed page, a section from its pages, a taxonomy from all its terms' pages, a term from its pages |

**`page`**: present on single pages, and on list and home pages that have an
`_index` file.

| Field | Notes |
|---|---|
| `kind` | `home`, `section`, `page`, `taxonomy` or `term` |
| `title`, `url`, `permalink` | `url` is site-relative (`/post/foo/`); `permalink` is absolute |
| `section` | First directory under `content/`; empty for top-level pages |
| `date`, `lastmod` | RFC 3339 strings, or none. `lastmod` falls back to `date` |
| `description` | Frontmatter `description` |
| `summary` | HTML, already safe. Content before `<!--more-->`, else frontmatter `summary`, else whole paragraphs until 70 words (Hugo's rule) |
| `content` | HTML, already safe. Not set on list items |
| `headings` | `[{level, id, text, html}]`: `text` is plain, `html` the rendered inner HTML (Markdown only) |
| `toc` | Hugo's `.TableOfContents`, byte for byte (levels 2–3, `<nav id="TableOfContents"><ul>…`), safe; empty without headings or for AsciiDoc |
| `word_count`, `reading_time` | Hugo's `.WordCount` (words in the content with tags stripped as `plainify` does) and `.ReadingTime` (`ceil(words / 212)` minutes, 0 for an empty page) |
| `params` | Every frontmatter key, as written |
| `terms` | Single pages only. Taxonomy name → the page's terms in frontmatter order, each a term object (below) |
| `prev`, `next` | Single pages only. The previous (older) and next (newer) page by date within the list that shows this page: its own section's list if that includes it, otherwise the first section list (by name) that does. So a section that excludes some pages (`[sections.post] exclude = { release = true }`) and another that lists them (`[sections.releases]`) each get their own chain. Each is `{title, url, permalink, date, summary, params}`, or none |

**`pager`** (list pages): `url`, `number`, `total`, `prev`, `next` (URLs or
none), `first`, `last` (URLs), `pages` (`[{number, url}]`, every page of the
list, for numbered pagination), `items` (a list of `page` objects without
`content`).

**`section`** (list pages): `name`, `title`, `url`, `count` (items in the list).

**`taxonomy`** (term and taxonomy pages): `name`, `title`, `url`, and on the
taxonomy index `terms`: a list of term objects, in name order. Terms with a
content file but no pages are included with `count` 0.

**`term`** (term pages): a term object.

A **term object** has `name` (as written in frontmatter), `key` (`name`
lowercased, the key of Hugo's `.Data.Terms`), `title`, `url`, `count`,
`params` (the frontmatter of the term's `_index` file, or none), `weight`
(from that file, else 0), `pages` (its pages in the term list's order) and
`pages_by_date` (oldest first: Hugo's `.ByDate`, stable on equal dates; use
`| reverse` for `.ByDate.Reverse`). Pages are `{title, url, permalink,
date, summary, params}`.

**`recent`** (home and 404 pages): the ten newest regular pages.

**Heading hook** (`markup/heading.html`): replaces every heading rendered
from Markdown, in content, summaries and `markdownify` output, with the
template's output (trailing whitespace trimmed). Context: `level`, `id`,
`text` (the inner HTML, safe), `plain`. Headings written as raw HTML are left
alone, as with Hugo's render hook.

## Shortcodes

`ref`, `relref` and `highlight` are built in. Any other `{{< name ... >}}`
renders `shortcodes/name.html`; with no such template the build fails, naming
the file and line. Shortcode templates get:

| Variable | Notes |
|---|---|
| `args` | Positional arguments, as strings |
| `inner` | Content between the tags of a paired shortcode, with nested shortcodes already expanded; safe. Use `inner \| markdownify` to render it as Markdown |
| `page` | The calling page: `title`, `url`, `permalink`, `date`, `section`, `params` |
| `lists` | Section name → that section's list items (after the section's `include`/`exclude`), each with `title`, `url`, `date` |
| `site` | As above |

A shortcode alone in its paragraph replaces the whole paragraph, as in Hugo.

## Filters and functions

- `{{ page.date | date('%-d %B %Y') }}` formats an RFC 3339 string with
  [chrono's strftime syntax](https://docs.rs/chrono/latest/chrono/format/strftime/).
- `{{ absurl('/post/') }}` returns `base_url` joined with the path.
- `{{ text | markdownify }}` renders Markdown, dropping the `<p>` wrapper when
  the result is one paragraph (Hugo's `markdownify`). The heading hook applies.

These reproduce Hugo's functions of the same purpose exactly, for porting
Hugo templates:

- `plainify`: Hugo's `plainify` (strip tags, collapse whitespace, `</p>` and
  `<br>` to newlines, block elements kept apart). Safe input stays safe.
- `html_unescape`: Hugo's `htmlUnescape` (numeric and common named entities).
- `truncate(n)`: Hugo's `truncate n` with the ` …` ellipsis, cutting at a word
  boundary. Plain text is escaped as Go's `html.EscapeString` does (`'` →
  `&#39;`, `"` → `&#34;`); safe HTML keeps its tags (which don't count) and
  has open tags closed, and its entities count as their spelled length, as
  in Hugo. The result is safe.
- `jsonify`: Hugo's `jsonify`: object keys sorted, `<`, `>`, `&` escaped as
  `\u003c`, `\u003e`, `\u0026`. Safe, for use inside `<script>`.

## Escaping

Output is HTML-escaped (`& < > " '`) unless the value is marked safe
(`content`, `summary`). Unlike minijinja's default, `/` is not escaped, so URLs
in attributes stay readable.
