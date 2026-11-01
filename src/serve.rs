//! `ferrisite serve`: build with drafts against a localhost base URL, serve
//! the output, rebuild when anything under the site root changes, and reload
//! open pages after each rebuild. Social cards are skipped. A small std-only
//! HTTP server: this is the writing loop, not a production server.

use crate::build::{self, Options};
use anyhow::{Context, Result};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

pub fn serve(root: PathBuf, out: PathBuf, port: u16, drafts: bool) -> Result<()> {
    let base = format!("http://localhost:{port}/");
    let opts = Options {
        root: root.clone(),
        out: out.clone(),
        drafts,
        future: drafts,
        base_url: Some(base.clone()),
        cards: false,
    };
    rebuild(&opts);
    let skip = Skip::new(&root, &out);
    std::thread::spawn(move || {
        let mut last = fingerprint(&root, &skip);
        loop {
            std::thread::sleep(Duration::from_millis(500));
            let now = fingerprint(&root, &skip);
            if now != last {
                last = now;
                rebuild(&opts);
            }
        }
    });
    let listener =
        TcpListener::bind(("127.0.0.1", port)).with_context(|| format!("binding port {port}"))?;
    println!("Serving {} at {base} (Ctrl+C to stop)", out.display());
    for stream in listener.incoming().flatten() {
        let out = out.clone();
        std::thread::spawn(move || {
            let _ = respond(stream, &out);
        });
    }
    Ok(())
}

/// Counts finished builds; pages poll it and reload when it changes.
static BUILDS: AtomicU64 = AtomicU64::new(0);

const RELOAD_PATH: &str = "/__ferrisite/build";

/// Injected into every HTML response: reload when a rebuild finishes.
const RELOAD_SCRIPT: &str = "<script>(()=>{let v=null;setInterval(async()=>{try{const t=await (await fetch('/__ferrisite/build',{cache:'no-store'})).text();if(v!==null&&t!==v)location.reload();v=t}catch(e){}},1000)})();</script>";

fn rebuild(opts: &Options) {
    let started = std::time::Instant::now();
    match build::build(opts) {
        Ok(r) => {
            BUILDS.fetch_add(1, Ordering::SeqCst);
            println!(
                "Built {} pages in {} ms",
                r.pages,
                started.elapsed().as_millis()
            )
        }
        Err(e) => eprintln!("Build failed: {e:#}"),
    }
}

/// What the watcher ignores: dot-directories and dot-files (`.git`, editor
/// swap files), the output directory, and other tools' output and caches.
/// Everything else under the root is watched, because AsciiDoc includes,
/// PlantUML `!include`s and forge imports can read any file there and are
/// resolved inside those libraries, out of ferrisite's sight.
struct Skip {
    out: PathBuf,
}

const SKIP_DIRS: &[&str] = &["public", "resources", "node_modules", "target"];

impl Skip {
    fn new(root: &Path, out: &Path) -> Skip {
        let out = if out.is_absolute() {
            out.to_path_buf()
        } else {
            root.join(out)
        };
        Skip {
            out: out.canonicalize().unwrap_or(out),
        }
    }

    fn skips(&self, path: &Path, name: &str, is_dir: bool) -> bool {
        name.starts_with('.')
            || (is_dir && SKIP_DIRS.contains(&name))
            || (is_dir && path.canonicalize().is_ok_and(|p| p == self.out))
    }
}

/// A fingerprint of every watched file's path, size and modification time.
/// It changes when a file is edited, added, removed or renamed; directory
/// times are left out, so an editor's swap file appearing beside a watched
/// file doesn't trigger a rebuild.
fn fingerprint(root: &Path, skip: &Skip) -> u64 {
    fn walk(dir: &Path, skip: &Skip, h: &mut u64) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = e.file_name();
            let Ok(meta) = std::fs::metadata(e.path()) else {
                continue;
            };
            if skip.skips(&e.path(), &name.to_string_lossy(), meta.is_dir()) {
                continue;
            }
            if meta.is_dir() {
                walk(&e.path(), skip, h);
            } else {
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|m| m.duration_since(SystemTime::UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_nanos());
                for bytes in [
                    e.path().as_os_str().as_encoded_bytes(),
                    &meta.len().to_le_bytes(),
                    &mtime.to_le_bytes(),
                ] {
                    for &b in bytes {
                        *h = (*h ^ b as u64).wrapping_mul(0x100000001b3);
                    }
                }
            }
        }
    }
    let mut h = 0xcbf29ce484222325;
    walk(root, skip, &mut h);
    h
}

fn respond(mut stream: TcpStream, out: &Path) -> std::io::Result<()> {
    let mut line = String::new();
    BufReader::new(&stream).read_line(&mut line)?;
    let path = line.split_whitespace().nth(1).unwrap_or("/");
    let path = path.split(['?', '#']).next().unwrap_or("/");
    if path == RELOAD_PATH {
        let n = BUILDS.load(Ordering::SeqCst).to_string();
        return write(&mut stream, "200 OK", "text/plain", n.as_bytes());
    }
    let rel = percent_decode(path.trim_start_matches('/'));
    if rel.split('/').any(|seg| seg == "..") {
        return write(&mut stream, "400 Bad Request", "text/plain", b"bad path");
    }
    let mut file = out.join(&rel);
    if file.is_dir() {
        file = file.join("index.html");
    }
    match std::fs::read(&file) {
        Ok(body) => {
            let ctype = content_type(&file);
            let body = if ctype.starts_with("text/html") {
                with_reload(body)
            } else {
                body
            };
            write(&mut stream, "200 OK", ctype, &body)
        }
        Err(_) => {
            let body =
                std::fs::read(out.join("404.html")).unwrap_or_else(|_| b"not found".to_vec());
            write(
                &mut stream,
                "404 Not Found",
                "text/html; charset=utf-8",
                &with_reload(body),
            )
        }
    }
}

fn write(stream: &mut TcpStream, status: &str, ctype: &str, body: &[u8]) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)
}

/// `body` with the reload script before `</body>` (or at the end).
fn with_reload(body: Vec<u8>) -> Vec<u8> {
    let at = body
        .windows(7)
        .rposition(|w| w.eq_ignore_ascii_case(b"</body>"))
        .unwrap_or(body.len());
    let mut out = Vec::with_capacity(body.len() + RELOAD_SCRIPT.len());
    out.extend_from_slice(&body[..at]);
    out.extend_from_slice(RELOAD_SCRIPT.as_bytes());
    out.extend_from_slice(&body[at..]);
    out
}

fn content_type(p: &Path) -> &'static str {
    match p.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "xml" => "application/xml",
        "md" => "text/markdown; charset=utf-8",
        "txt" => "text/plain; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ico" => "image/x-icon",
        _ => "application/octet-stream",
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reload_script_goes_before_the_last_body_close() {
        let html = b"<html><body><p>&lt;/body&gt;</p></BODY></html>".to_vec();
        let out = String::from_utf8(with_reload(html)).unwrap();
        assert!(
            out.ends_with(&format!("{RELOAD_SCRIPT}</BODY></html>")),
            "{out}"
        );
        let bare = String::from_utf8(with_reload(b"<p>x</p>".to_vec())).unwrap();
        assert_eq!(bare, format!("<p>x</p>{RELOAD_SCRIPT}"));
    }

    #[test]
    fn watcher_sees_includes_but_not_output_or_dot_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for d in ["content", "_code-samples", ".git", "out", "public"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        std::fs::write(root.join("content/a.md"), "x").unwrap();
        let skip = Skip::new(root, Path::new("out"));
        let base = fingerprint(root, &skip);
        for ignored in [
            ".git/index",
            "out/index.html",
            "public/x.html",
            "content/.a.md.swp",
        ] {
            std::fs::write(root.join(ignored), "x").unwrap();
        }
        assert_eq!(
            fingerprint(root, &skip),
            base,
            "ignored paths don't trigger"
        );
        std::fs::write(root.join("_code-samples/a.java"), "x").unwrap();
        let added = fingerprint(root, &skip);
        assert_ne!(added, base, "an included file triggers a rebuild");
        std::fs::remove_file(root.join("content/a.md")).unwrap();
        assert_ne!(fingerprint(root, &skip), added, "so does a deletion");
    }
}
