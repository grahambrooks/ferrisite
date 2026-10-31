//! End-to-end builds of the fixture site in tests/fixtures/site.

use ferrisite::build::{MARKER, Options, build};
use std::path::{Path, PathBuf};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/site")
}

fn build_into(out: &Path, drafts: bool) -> ferrisite::build::Report {
    // SAFETY: tests in this file don't read SOURCE_DATE_EPOCH concurrently with
    // a different value; every test sets the same one.
    unsafe { std::env::set_var("SOURCE_DATE_EPOCH", "1785000000") }; // 2026-07-25
    build(&Options {
        root: fixture(),
        out: out.to_path_buf(),
        drafts,
        future: false,
        base_url: None,
    })
    .expect("build")
}

fn read(out: &Path, rel: &str) -> String {
    std::fs::read_to_string(out.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

#[test]
fn builds_pages_lists_and_static_files() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("public");
    let report = build_into(&out, false);
    assert_eq!(
        report.pages, 4,
        "3 posts + about; draft and future excluded"
    );
    for rel in [
        "index.html",
        "about/index.html",
        "post/post-1/index.html",
        "post/index.html",
        "post/page/2/index.html",
        "css/site.css",
        MARKER,
    ] {
        assert!(out.join(rel).exists(), "missing {rel}");
    }
    assert!(!out.join("post/draft").exists());
    assert!(!out.join("post/future").exists());

    let post = read(&out, "post/post-3/index.html");
    assert!(post.contains("<h2 id=\"section\">Section</h2>"));
    assert!(post.contains("<h2 id=\"section-1\">Section</h2>"));
    assert!(post.contains("<link rel=\"canonical\" href=\"https://example.test/post/post-3/\">"));
    assert!(post.contains("3 January 2026"));

    // Newest first, two per page.
    let page1 = read(&out, "post/index.html");
    let page2 = read(&out, "post/page/2/index.html");
    assert!(page1.find("Post 3").unwrap() < page1.find("Post 2").unwrap());
    assert!(page2.contains("Post 1") && !page2.contains("Post 3"));
    assert!(page1.contains("href=\"/post/page/2/\""));
}

#[test]
fn taxonomies_aliases_feeds_and_shortcodes() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("public");
    build_into(&out, false);
    for rel in [
        "tags/index.html",
        "tags/index.xml",
        "tags/rust/index.html",
        "tags/rust/index.xml",
        "tags/testing/index.html",
        "index.xml",
        "rss.xml",
        "post/index.xml",
        "sitemap.xml",
        "index.json",
        "robots.txt",
        "404.html",
    ] {
        assert!(out.join(rel).exists(), "missing {rel}");
    }
    // Two posts tagged rust, page size 2: one page, plus Hugo's page/1 alias.
    let alias = read(&out, "tags/rust/page/1/index.html");
    assert!(alias.contains("url=https://example.test/tags/rust/"));
    assert!(
        read(&out, "blog/post-one/index.html").contains("url=https://example.test/post/post-1/")
    );
    assert!(
        read(&out, "2026/01/01/post-one.html").contains("url=https://example.test/post/post-1/")
    );
    assert!(read(&out, "post/page/1/index.html").contains("url=https://example.test/post/"));

    assert_eq!(
        read(&out, "rss.xml"),
        read(&out, "index.xml"),
        "feed copy is byte-identical"
    );
    let feed = read(&out, "tags/rust/index.xml");
    assert_eq!(feed.matches("<item>").count(), 2);
    assert!(
        feed.find("Post 2").unwrap() < feed.find("Post 1").unwrap(),
        "newest first"
    );

    let post2 = read(&out, "post/post-2/index.html");
    assert!(
        post2.contains("<a href=\"https://example.test/post/post-1/\">post one</a>"),
        "ref resolves"
    );
    assert!(
        post2.contains("<div class=\"note note-info\">A <em>note</em>.</div>"),
        "template shortcode"
    );
    assert!(
        post2.contains("<span class=\"inline-svg\" aria-label=\"Diagram\" role=\"img\"><svg"),
        "SVG inlined"
    );
    assert!(
        !post2.contains("FERRISITESHORTCODE"),
        "no placeholder left behind"
    );
}

#[test]
fn unknown_shortcode_fails_with_file_and_line() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("site");
    std::fs::create_dir_all(root.join("content/post")).unwrap();
    std::fs::write(
        root.join("ferrisite.toml"),
        "base_url = \"https://e.test\"\ntitle = \"T\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("content/post/a.md"),
        "---\ntitle: A\n---\none\ntwo {{< mystery >}}\n",
    )
    .unwrap();
    let err = build(&Options {
        root,
        out: dir.path().join("out"),
        drafts: false,
        future: false,
        base_url: None,
    })
    .err()
    .expect("unknown shortcode must fail");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("post/a.md") && msg.contains("line 5") && msg.contains("mystery"),
        "{msg}"
    );
}

#[test]
fn parity_against_itself_passes_and_catches_missing_files() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a"), dir.path().join("b"));
    build_into(&a, false);
    build_into(&b, false);
    let cfg = ferrisite::parity::ParityConfig {
        base_url: "https://example.test/".into(),
        ..Default::default()
    };
    let report = ferrisite::parity::compare(&a, &b, &cfg).unwrap();
    assert!(!report.failed(), "a build matches itself");
    std::fs::remove_file(b.join("tags/rust/index.xml")).unwrap();
    let report = ferrisite::parity::compare(&a, &b, &cfg).unwrap();
    assert_eq!(report.missing, vec!["tags/rust/index.xml".to_string()]);
    assert!(report.failed());
}

#[test]
fn drafts_flag_includes_drafts() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("public");
    build_into(&out, true);
    assert!(out.join("post/draft/index.html").exists());
}

#[test]
fn builds_are_byte_identical() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a"), dir.path().join("b"));
    build_into(&a, false);
    build_into(&b, false);
    let files = |root: &Path| {
        let mut v = Vec::new();
        collect(root, root, &mut v);
        v.sort();
        v
    };
    let (fa, fb) = (files(&a), files(&b));
    assert_eq!(
        fa.iter().map(|x| &x.0).collect::<Vec<_>>(),
        fb.iter().map(|x| &x.0).collect::<Vec<_>>()
    );
    assert_eq!(fa, fb);
}

#[test]
fn refuses_to_clean_a_foreign_directory() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("precious");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("keep.txt"), "mine").unwrap();
    let err = build(&Options {
        root: fixture(),
        out: out.clone(),
        drafts: false,
        future: false,
        base_url: None,
    })
    .err()
    .expect("should refuse");
    assert!(err.to_string().contains("refusing to clean"));
    assert_eq!(
        std::fs::read_to_string(out.join("keep.txt")).unwrap(),
        "mine"
    );
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            collect(root, &p, out);
        } else {
            out.push((
                p.strip_prefix(root).unwrap().display().to_string(),
                std::fs::read(&p).unwrap(),
            ));
        }
    }
}

fn temp_site(files: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("site");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("ferrisite.toml"),
        "base_url = \"https://e.test\"\ntitle = \"T\"\n",
    )
    .unwrap();
    for (path, body) in files {
        let p = root.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }
    (dir, root)
}

#[test]
fn diagrams_highlighting_and_asciidoc() {
    let (dir, root) = temp_site(&[
        (
            "content/post/md.md",
            "---\ntitle: M\n---\n\n```mermaid\nflowchart LR\n  A[Start] --> B[End]\n```\n\n```rust\nfn main() {}\n```\n",
        ),
        (
            "content/post/doc.adoc",
            "---\ntitle: D\n---\n= D\n\n== Flow\n\n[plantuml, seq, svg]\n----\nAlice -> Bob: hello\n----\n\n[source,rust]\n----\nlet x = 1;\n----\n",
        ),
    ]);
    let out = dir.path().join("out");
    build(&Options {
        root,
        out: out.clone(),
        drafts: false,
        future: false,
        base_url: None,
    })
    .expect("build");
    let md = read(&out, "post/md/index.html");
    assert_eq!(md.matches("<svg").count(), 2, "mermaid in light and dark");
    assert!(md.contains("class=\"diagram diagram-mermaid\"") && md.contains("Start"));
    assert!(md.contains(
        "<div class=\"highlight\"><pre tabindex=\"0\" class=\"chroma\"><code class=\"language-rust\" data-lang=\"rust\">"
    ));
    assert!(md.contains("class=\"sx-"), "highlighted spans");
    let doc = read(&out, "post/doc/index.html");
    assert!(doc.contains("class=\"diagram diagram-plantuml\"") && doc.contains("Alice"));
    assert!(
        doc.contains("class=\"sx-"),
        "AsciiDoc source block highlighted"
    );
    assert!(doc.contains("<h2>Flow</h2>"));
    let css = read(&out, "css/ferrisite.css");
    assert!(css.contains("html[data-mode=\"dark\"] .diagram-dark { display: block; }"));
    assert!(css.contains("html[data-mode=\"dark\"] .sx-"));
}

#[test]
fn broken_diagram_fails_with_file_and_line() {
    let (dir, root) = temp_site(&[(
        "content/post/bad.md",
        "---\ntitle: B\n---\n\nIntro.\n\n```mermaid\nnot a diagram\n```\n",
    )]);
    let err = build(&Options {
        root,
        out: dir.path().join("out"),
        drafts: false,
        future: false,
        base_url: None,
    })
    .err()
    .expect("an unsupported diagram must fail the build");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("post/bad.md") && msg.contains("line 7:") && msg.contains("mermaid"),
        "{msg}"
    );
}

#[test]
fn agent_readable_output_cards_and_check() {
    let (dir, root) = temp_site(&[
        (
            "content/post/a.md",
            "---\ntitle: First post\ndate: 2026-01-02\ntags: [rust]\n---\n\nSee [the other](/post/b/).\n\n```mermaid\nflowchart LR\n  A --> B\n```\n",
        ),
        (
            "content/post/b.md",
            "---\ntitle: Second\ndate: 2026-01-01\n---\n\nHello.\n",
        ),
    ]);
    std::fs::write(
        root.join("ferrisite.toml"),
        "base_url = \"https://e.test\"\ntitle = \"T\"\ntaxonomies = [\"tags\"]\n[cards]\nenabled = true\n[agents]\nrecent_section = \"post\"\n",
    )
    .unwrap();
    let out = dir.path().join("out");
    build(&Options {
        root: root.clone(),
        out: out.clone(),
        drafts: false,
        future: false,
        base_url: None,
    })
    .expect("build");
    let md = read(&out, "post/a/index.md");
    assert!(
        md.starts_with("---\ntitle: First post\nurl: https://e.test/post/a/\n"),
        "{md}"
    );
    assert!(
        md.contains("[the other](https://e.test/post/b/)"),
        "links made absolute"
    );
    assert!(
        md.contains("```mermaid\nflowchart LR"),
        "diagram kept as source"
    );
    let llms = read(&out, "llms.txt");
    assert!(llms.starts_with("# T\n"));
    assert!(llms.contains("(https://e.test/post/a/index.md)"));
    assert!(
        out.join("llms-full.txt").exists()
            && out.join("atom.xml").exists()
            && out.join("feed.json").exists()
    );
    let html = read(&out, "tags/rust/index.html");
    assert!(html.contains(
        "<link rel=\"alternate\" type=\"text/markdown\" href=\"https://e.test/tags/rust/index.md\">"
    ));
    let png = std::fs::read(out.join("images/cards/post-a.png")).unwrap();
    assert_eq!(&png[..4], b"\x89PNG");

    let report = ferrisite::check::run(&out, "https://e.test/", true, &[]).unwrap();
    assert!(
        !report.failed(),
        "{:?} {:?}",
        report.broken_links,
        report.agent_problems
    );

    // An unlisted broken link fails `check`; listing it with a reason passes.
    std::fs::write(
        out.join("post/b/index.html"),
        "<a href=\"/missing/\">x</a><link rel=\"alternate\" type=\"text/markdown\" href=\"https://e.test/post/b/index.md\">",
    )
    .unwrap();
    let report = ferrisite::check::run(&out, "https://e.test/", false, &[]).unwrap();
    assert_eq!(report.broken_links.len(), 1);
    let known = [ferrisite::config::KnownBroken {
        link: "/missing/".into(),
        reason: "test".into(),
    }];
    let report = ferrisite::check::run(&out, "https://e.test/", false, &known).unwrap();
    assert!(!report.failed() && report.known_broken.len() == 1);
}

#[test]
fn template_context_terms_neighbours_toc_and_hooks() {
    let post = |title: &str, date: &str, extra: &str, body: &str| {
        format!("---\ntitle: \"{title}\"\ndate: {date}\n{extra}---\n{body}")
    };
    let (dir, root) = temp_site(&[
        (
            "ferrisite.toml",
            "base_url = \"https://e.test/\"\ntitle = \"T\"\ntaxonomies = [\"tags\", \"series\", \"projects\"]\n\
             [pagination]\npage_size = 1\n\
             [sections.post]\nexclude = { release = true }\n\
             [sections.releases]\nsource = \"post\"\ninclude = { release = true }\norder = \"date-reverse\"\n",
        ),
        (
            "content/post/a.md",
            &post(
                "A",
                "2026-01-01T00:00:00Z",
                "series: [s]\nprojects: [tool]\ntags: [Rust]\n",
                "## One\n\n### One.a\n\n## Two\n\nBody.\n",
            ),
        ),
        (
            "content/post/b.md",
            &post("B", "2026-01-03T00:00:00Z", "series: [s]\n", "B.\n"),
        ),
        (
            "content/post/r1.md",
            &post(
                "R1",
                "2026-01-02T00:00:00Z",
                "release: true\nprojects: [tool]\n",
                "R1.\n",
            ),
        ),
        (
            "content/post/r2.md",
            &post("R2", "2026-01-04T00:00:00Z", "release: true\n", "R2.\n"),
        ),
        ("content/releases/_index.md", "---\ntitle: Releases\n---\n"),
        (
            "content/series/s/_index.md",
            "---\ntitle: The series\n---\n",
        ),
        (
            "content/projects/tool/_index.md",
            "---\ntitle: Tool\nweight: 3\nrepo: https://github.com/x/tool\ninstall: [\"brew install tool\"]\n---\n",
        ),
        (
            "templates/markup/heading.html",
            "<h{{ level }} id=\"{{ id }}\" class=hooked>{{ text }}</h{{ level }}>\n",
        ),
        (
            "templates/single.html",
            "{% for s in page.terms.series %}series={{ s.title }}:{% for p in s.pages_by_date %}{{ p.title }},{% endfor %}{% endfor %}\n\
             {% for p in page.terms.projects %}project={{ p.title }}/{{ p.params.repo }}/{{ p.params.install[0] }}/{{ p.count }}/{{ p.weight }}{% endfor %}\n\
             prev={% if page.prev %}{{ page.prev.title }}{% endif %} next={% if page.next %}{{ page.next.title }}{% endif %}\n\
             kind={{ current.kind }} section={{ current.section }}\n\
             toc={{ page.toc }}\n\
             {{ page.content }}\n\
             md={{ '## Hi' | markdownify }}\n\
             plain={{ '<p>a <em>b</em></p><p>c</p>' | plainify }}|\n\
             trunc={{ \"it's a long sentence\" | truncate(10) }}\n\
             json={{ {'b': '<x>', 'a': 1} | jsonify }}\n\
             unesc={{ 'it&rsquo;s' | html_unescape }}\n",
        ),
        (
            "templates/list.html",
            "{{ current.kind }}|{{ current.title }}|{{ current.date }}|{{ current.canonical }}|{{ pager.first }}|{{ pager.last }}|{% for p in pager.pages %}{{ p.number }}={{ p.url }};{% endfor %}|{% for i in pager.items %}{{ i.title }}{% endfor %}",
        ),
        (
            "templates/taxonomy.html",
            "{% for t in taxonomy.terms %}{{ t.key }}:{{ t.count }}:{{ t.weight }}:{% for p in t.pages %}{{ p.title }}{% endfor %};{% endfor %}|{{ current.date }}",
        ),
        (
            "templates/404.html",
            "{{ current.kind }}|{{ current.title }}|{{ recent | length }}",
        ),
    ]);
    let out = dir.path().join("out");
    build(&Options {
        root,
        out: out.clone(),
        drafts: false,
        future: false,
        base_url: None,
    })
    .expect("build");

    let a = read(&out, "post/a/index.html");
    assert!(a.contains("series=The series:A,B,"), "{a}");
    assert!(
        a.contains("project=Tool/https://github.com/x/tool/brew install tool/2/3"),
        "{a}"
    );
    // Essays link to essays: A's neighbours skip the release notes.
    assert!(a.contains("prev= next=B"), "{a}");
    assert!(a.contains("kind=page section=post"));
    assert!(a.contains("toc=<nav id=\"TableOfContents\">\n  <ul>\n    <li><a href=\"#one\">One</a>\n      <ul>\n        <li><a href=\"#onea\">One.a</a></li>"), "{a}");
    assert!(a.contains("<h2 id=\"one\" class=hooked>One</h2>"), "{a}");
    assert!(a.contains("md=<h2 id=\"hi\" class=hooked>Hi</h2>"), "{a}");
    assert!(a.contains("plain=a b\nc\n|"), "{a}");
    assert!(a.contains("trunc=it&#39;s a …"), "{a}");
    assert!(a.contains(r#"json={"a":1,"b":"\u003cx\u003e"}"#), "{a}");
    assert!(a.contains("unesc=it’s"), "{a}");
    // Release notes link to release notes.
    let r1 = read(&out, "post/r1/index.html");
    assert!(r1.contains("prev= next=R2"), "{r1}");

    // A list page takes its newest page's date; pagers know every page.
    let post = read(&out, "post/page/2/index.html");
    assert_eq!(
        post,
        "section|Post|2026-01-04T00:00:00+00:00|https://e.test/post/|/post/|/post/page/2/|1=/post/;2=/post/page/2/;|A"
    );
    let releases = read(&out, "releases/index.html");
    assert!(releases.ends_with("|R2"), "date-reverse order: {releases}");
    let projects = read(&out, "projects/index.html");
    assert_eq!(projects, "tool:2:3:R1A;|2026-01-02T00:00:00+00:00");
    let tags = read(&out, "tags/index.html");
    assert!(tags.starts_with("rust:1:0:A;"), "{tags}");
    assert_eq!(read(&out, "404.html"), "404|404 Page not found|4");
}
