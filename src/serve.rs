//! `ferrisite serve`: build with drafts against a localhost base URL, serve
//! the output, and rebuild when content, templates, static files or the
//! config change. A small std-only HTTP server: this is the writing loop, not
//! a production server.

use crate::build::{self, Options};
use anyhow::{Context, Result};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub fn serve(root: PathBuf, out: PathBuf, port: u16, drafts: bool) -> Result<()> {
    let base = format!("http://localhost:{port}/");
    let opts = Options {
        root: root.clone(),
        out: out.clone(),
        drafts,
        future: drafts,
        base_url: Some(base.clone()),
    };
    rebuild(&opts);
    let watched: Vec<PathBuf> = ["content", "templates", "static", "ferrisite.toml"]
        .iter()
        .map(|p| root.join(p))
        .chain(
            crate::config::Config::load(&root)
                .map(|c| {
                    c.static_dirs
                        .iter()
                        .map(|d| root.join(d))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
        )
        .collect();
    std::thread::spawn(move || {
        let mut last = newest(&watched);
        loop {
            std::thread::sleep(Duration::from_millis(500));
            let now = newest(&watched);
            if now > last {
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

fn rebuild(opts: &Options) {
    let started = std::time::Instant::now();
    match build::build(opts) {
        Ok(r) => println!(
            "Built {} pages in {} ms",
            r.pages,
            started.elapsed().as_millis()
        ),
        Err(e) => eprintln!("Build failed: {e:#}"),
    }
}

/// The newest modification time under any watched path.
fn newest(paths: &[PathBuf]) -> SystemTime {
    fn walk(p: &Path, best: &mut SystemTime) {
        let Ok(meta) = std::fs::metadata(p) else {
            return;
        };
        if let Ok(m) = meta.modified() {
            *best = (*best).max(m);
        }
        if meta.is_dir()
            && let Ok(entries) = std::fs::read_dir(p)
        {
            for e in entries.flatten() {
                walk(&e.path(), best);
            }
        }
    }
    let mut best = SystemTime::UNIX_EPOCH;
    for p in paths {
        walk(p, &mut best);
    }
    best
}

fn respond(mut stream: TcpStream, out: &Path) -> std::io::Result<()> {
    let mut line = String::new();
    BufReader::new(&stream).read_line(&mut line)?;
    let path = line.split_whitespace().nth(1).unwrap_or("/");
    let path = path.split(['?', '#']).next().unwrap_or("/");
    let rel = percent_decode(path.trim_start_matches('/'));
    if rel.split('/').any(|seg| seg == "..") {
        return write(&mut stream, "400 Bad Request", "text/plain", b"bad path");
    }
    let mut file = out.join(&rel);
    if file.is_dir() {
        file = file.join("index.html");
    }
    match std::fs::read(&file) {
        Ok(body) => write(&mut stream, "200 OK", content_type(&file), &body),
        Err(_) => {
            let body =
                std::fs::read(out.join("404.html")).unwrap_or_else(|_| b"not found".to_vec());
            write(
                &mut stream,
                "404 Not Found",
                "text/html; charset=utf-8",
                &body,
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
