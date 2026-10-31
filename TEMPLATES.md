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

Templates can `{% extends "base.html" %}` and override blocks.

## Context

All ordering, filtering and pagination happen before rendering, so templates
only loop and print.

**`site`**: `title`, `base_url`, `author`, `description`, `params` (the
`[params]` table from `ferrisite.toml`).

**`page`**: present on single pages, and on list and home pages that have an
`_index` file.

| Field | Notes |
|---|---|
| `kind` | `home`, `section` or `page` |
| `title`, `url`, `permalink` | `url` is site-relative (`/post/foo/`); `permalink` is absolute |
| `section` | First directory under `content/`; empty for top-level pages |
| `date`, `lastmod` | RFC 3339 strings, or none. `lastmod` falls back to `date` |
| `description` | Frontmatter `description` |
| `summary` | HTML, already safe. Content before `<!--more-->`, else frontmatter `summary`, else whole paragraphs until 70 words (Hugo's rule) |
| `content` | HTML, already safe. Not set on list items |
| `headings` | `[{level, id, text}]`, for a table of contents |
| `word_count`, `reading_time` | Reading time in minutes at 213 words per minute |
| `params` | Every frontmatter key, as written |

**`pager`** (list pages): `url`, `number`, `total`, `prev`, `next` (URLs or
none), `items` (a list of `page` objects without `content`).

**`section`** (list pages): `name`, `title`, `url`, `count` (items in the list).

**`taxonomy`** (term and taxonomy pages): `name`, `title`, `url`, and on the
taxonomy index `terms`: `[{name, title, url, count}]`.

**`term`** (term pages): `name` (as written in frontmatter), `title`, `url`, `count`.

**`recent`** (home page): the ten newest regular pages.

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
  the result is one paragraph (Hugo's `markdownify`).

## Escaping

Output is HTML-escaped (`& < > " '`) unless the value is marked safe
(`content`, `summary`). Unlike minijinja's default, `/` is not escaped, so URLs
in attributes stay readable.
