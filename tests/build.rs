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
    assert!(!out.join("tags").exists(), "taxonomies arrive in M2");

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
