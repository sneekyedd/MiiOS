//! A tiny web server on 127.0.0.1:8765 (only this laptop can reach it) that
//! serves Mii Hub, plus your Miis so Mii Hub can bring them in:
//!   /                  -> Mii Hub (the auto-updated copy, else the built-in one)
//!   /miios-hook.js     -> the MiiOS -> Mii Hub bridge script
//!   /miis/list.json    -> names of the .glb files in ~/MiiOS/miis
//!   /miis/NAME.glb     -> one Mii

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;

fn hub_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    let updated = PathBuf::from(home).join(".local/share/miios/apps/mii-hub");
    if updated.join("index.html").exists() { updated } else { PathBuf::from("/opt/miios/apps/mii-hub") }
}

fn mime(p: &str) -> &'static str {
    match p.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "html" => "text/html; charset=utf-8", "js" | "mjs" => "text/javascript", "css" => "text/css",
        "json" => "application/json", "png" => "image/png", "jpg" | "jpeg" => "image/jpeg", "svg" => "image/svg+xml",
        "glb" => "model/gltf-binary", "ogg" => "audio/ogg", "mp3" => "audio/mpeg", "wav" => "audio/wav",
        "woff2" => "font/woff2", "woff" => "font/woff", "ico" => "image/x-icon", _ => "application/octet-stream",
    }
}

fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) { out.push(v); i += 3; continue; }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn reply(mut s: TcpStream, code: &str, ctype: &str, body: &[u8]) {
    let head = format!("HTTP/1.1 {code}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n", body.len());
    let _ = s.write_all(head.as_bytes());
    let _ = s.write_all(body);
}

fn handle(mut s: TcpStream) {
    let mut buf = [0u8; 4096];
    let n = s.read(&mut buf).unwrap_or(0);
    let req = String::from_utf8_lossy(&buf[..n]);
    let path = req.split_whitespace().nth(1).unwrap_or("/").split('?').next().unwrap_or("/");
    let path = decode(path);
    if path.split('/').any(|p| p == "..") { return reply(s, "403 Forbidden", "text/plain", b"no"); }
    if path == "/miis/list.json" {
        let mut names: Vec<String> = std::fs::read_dir(super::miis::dir()).into_iter().flatten().flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.to_ascii_lowercase().ends_with(".glb")).collect();
        names.sort();
        let json = format!("[{}]", names.iter().map(|n| format!("\"{}\"", n.replace('\\', "\\\\").replace('"', "\\\""))).collect::<Vec<_>>().join(","));
        return reply(s, "200 OK", "application/json", json.as_bytes());
    }
    let file = if let Some(rest) = path.strip_prefix("/miis/") { super::miis::dir().join(rest) }
        else if path == "/miios-hook.js" { PathBuf::from("/usr/share/miios/miios-hook.js") }
        else { let mut p = hub_dir().join(path.trim_start_matches('/')); if p.is_dir() { p = p.join("index.html"); } p };
    match std::fs::read(&file) {
        Ok(body) => reply(s, "200 OK", mime(&file.to_string_lossy()), &body),
        Err(_) => reply(s, "404 Not Found", "text/plain", b"not found"),
    }
}

pub fn start() {
    std::thread::spawn(|| {
        let Ok(l) = TcpListener::bind("127.0.0.1:8765") else { crate::log!("web server: port 8765 busy"); return };
        for s in l.incoming().flatten() { std::thread::spawn(move || handle(s)); }
    });
}
