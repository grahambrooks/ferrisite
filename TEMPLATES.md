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
| Home page | `home.html` |

Templates can `{% extends "base.html" %}` and override blocks.

## Context

All ordering, filtering and pagination happen before rendering, so templates
only loop and print.

**`site`**: `title`, `base_url`, `author`, `description`.

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

**`section`** (list pages): `name`, `url`, `count`.

**`recent`** (home page): the ten newest regular pages.

## Filters and functions

- `{{ page.date | date('%-d %B %Y') }}` formats an RFC 3339 string with
  [chrono's strftime syntax](https://docs.rs/chrono/latest/chrono/format/strftime/).
- `{{ absurl('/post/') }}` returns `base_url` joined with the path.

## Escaping

Output is HTML-escaped (`& < > " '`) unless the value is marked safe
(`content`, `summary`). Unlike minijinja's default, `/` is not escaped, so URLs
in attributes stay readable.
