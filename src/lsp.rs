#![allow(dead_code)]
// volta/src/lsp.rs — minimal LSP server (JSON-RPC over stdin/stdout)
//
// Implements just enough of LSP to give VS Code real-time diagnostics:
//   initialize / initialized / shutdown / exit
//   textDocument/didOpen  → publishDiagnostics
//   textDocument/didChange → publishDiagnostics
//   textDocument/didSave   → publishDiagnostics
//
// No external dependencies — JSON is hand-built/parsed with the tiny
// helpers below.  The compiler pipeline runs in-process.

use std::collections::HashSet;
use std::io::{self, BufRead, Read, Write};
use std::path::PathBuf;

use crate::{ast, emit, lexer, parser, sema};

// ── Entry point ───────────────────────────────────────────────────────────────

pub fn run_server() {
    let stdin  = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let mut reader = stdin.lock();

    eprintln!("[volta-lsp] starting");

    loop {
        // Read Content-Length header
        let mut content_length: Option<usize> = None;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                return; // EOF
            }
            let line = line.trim_end_matches(['\r', '\n']).to_string();
            if line.is_empty() { break; }
            if line.starts_with("Content-Length:") {
                let val = line["Content-Length:".len()..].trim();
                content_length = val.parse().ok();
            }
        }

        let len = match content_length {
            Some(n) => n,
            None    => continue,
        };

        let mut body = vec![0u8; len];
        if reader.read_exact(&mut body).is_err() { return; }
        let body = match std::str::from_utf8(&body) {
            Ok(s)  => s.to_string(),
            Err(_) => continue,
        };

        if let Some(response) = handle_message(&body) {
            let encoded = format!("Content-Length: {}\r\n\r\n{}", response.len(), response);
            let _ = out.write_all(encoded.as_bytes());
            let _ = out.flush();
        }
    }
}

// ── Message dispatch ──────────────────────────────────────────────────────────

fn handle_message(body: &str) -> Option<String> {
    let method  = json_str(body, "method")?;
    let id      = json_raw(body, "id");

    match method.as_str() {
        "initialize" => {
            let result = r#"{"capabilities":{"textDocumentSync":1},"serverInfo":{"name":"volta-lsp","version":"0.4.0"}}"#;
            Some(ok_response(id.as_deref(), result))
        }

        "initialized" | "$/cancelRequest" => None,

        "shutdown" => Some(ok_response(id.as_deref(), "null")),

        "exit" => {
            std::process::exit(0);
        }

        "textDocument/didOpen" => {
            if let Some(text) = nested_str(body, "params", "textDocument", "text") {
                let uri = nested_str(body, "params", "textDocument", "uri")
                    .unwrap_or_default();
                publish_diagnostics(&uri, &text);
            }
            None
        }

        "textDocument/didChange" => {
            // incremental or full — we always get the full text with syncKind=1
            let uri = nested_str(body, "params", "textDocument", "uri")
                .unwrap_or_default();
            if let Some(text) = first_change_text(body) {
                publish_diagnostics(&uri, &text);
            }
            None
        }

        "textDocument/didSave" => {
            if let Some(uri) = nested_str(body, "params", "textDocument", "uri") {
                if let Ok(path) = uri_to_path(&uri) {
                    if let Ok(text) = std::fs::read_to_string(&path) {
                        publish_diagnostics(&uri, &text);
                    }
                }
            }
            None
        }

        _ => {
            // Respond with MethodNotFound for requests (have an id), ignore notifications
            if let Some(id_val) = &id {
                Some(error_response(id_val, -32601, "Method not found"))
            } else {
                None
            }
        }
    }
}

// ── Diagnostics ───────────────────────────────────────────────────────────────

fn publish_diagnostics(uri: &str, src: &str) {
    let diags = collect_diagnostics(src);
    let diag_json = format!(
        r#"{{"uri":{},"diagnostics":[{}]}}"#,
        json_escape(uri),
        diags.join(",")
    );
    let notification = format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{}}}"#,
        diag_json
    );
    let encoded = format!("Content-Length: {}\r\n\r\n{}", notification.len(), notification);
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let _ = out.write_all(encoded.as_bytes());
    let _ = out.flush();
}

struct Diagnostic {
    line:    usize, // 0-based
    col:     usize, // 0-based
    end_col: usize,
    msg:     String,
    sev:     u8, // 1=error 2=warning
}

impl Diagnostic {
    fn to_json(&self) -> String {
        format!(
            r#"{{"range":{{"start":{{"line":{line},"character":{col}}},"end":{{"line":{line},"character":{end}}}}},"severity":{sev},"source":"volta","message":{msg}}}"#,
            line = self.line.saturating_sub(1),
            col  = self.col.saturating_sub(1),
            end  = self.end_col.max(self.col.saturating_sub(1) + 1),
            sev  = self.sev,
            msg  = json_escape(&self.msg),
        )
    }
}

fn collect_diagnostics(src: &str) -> Vec<String> {
    let mut out = Vec::new();

    // Lex
    let tokens = match lexer::Lexer::new(src).tokenize() {
        Ok(t)  => t,
        Err(e) => {
            let d = Diagnostic { line: e.line, col: e.col, end_col: e.col + 1, msg: e.msg, sev: 1 };
            out.push(d.to_json());
            return out;
        }
    };

    // Parse
    let prog = match parser::Parser::new(tokens).parse_program() {
        Ok(p)  => p,
        Err(e) => {
            let d = Diagnostic { line: e.line, col: e.col, end_col: e.col + 8, msg: e.msg, sev: 1 };
            out.push(d.to_json());
            return out;
        }
    };

    // Sema
    let mut checker = sema::Checker::new();
    checker.check_program(&prog);

    for e in &checker.errors {
        let d = Diagnostic { line: e.line, col: 1, end_col: 80, msg: e.msg.clone(), sev: 1 };
        out.push(d.to_json());
    }
    for w in &checker.warnings {
        let d = Diagnostic { line: w.line, col: 1, end_col: 80, msg: w.msg.clone(), sev: 2 };
        out.push(d.to_json());
    }

    out
}

// ── JSON-RPC helpers ──────────────────────────────────────────────────────────

fn ok_response(id: Option<&str>, result: &str) -> String {
    let id_val = id.unwrap_or("null");
    format!(r#"{{"jsonrpc":"2.0","id":{},"result":{}}}"#, id_val, result)
}

fn error_response(id: &str, code: i32, msg: &str) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":{},"error":{{"code":{},"message":{}}}}}"#,
        id, code, json_escape(msg)
    )
}

// Tiny hand-rolled JSON field extractors — no serde dependency.

/// Extract a top-level string field: "key":"value"
fn json_str(body: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\":", key);
    let start  = body.find(&needle)? + needle.len();
    let rest   = body[start..].trim_start();
    if rest.starts_with('"') {
        Some(parse_json_string(&rest[1..]))
    } else {
        None
    }
}

/// Extract a top-level raw field (number, null, or string) as text.
fn json_raw(body: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\":", key);
    let start  = body.find(&needle)? + needle.len();
    let rest   = body[start..].trim_start();
    if rest.starts_with('"') {
        Some(format!("\"{}\"", parse_json_string(&rest[1..])))
    } else {
        let end = rest.find([',', '}', ']']).unwrap_or(rest.len());
        Some(rest[..end].trim().to_string())
    }
}

/// Extract a string two levels deep: params.obj.field
fn nested_str(body: &str, _outer: &str, obj: &str, field: &str) -> Option<String> {
    // find "obj": then "field": inside the same object
    let obj_key = format!("\"{}\":", obj);
    let obj_pos = body.find(&obj_key)? + obj_key.len();
    let rest    = &body[obj_pos..];
    let brace   = rest.find('{')?;
    let inner   = &rest[brace..find_object_end(rest, brace)];
    json_str(inner, field)
}

/// Extract text from first element of contentChanges array.
fn first_change_text(body: &str) -> Option<String> {
    let key   = "\"contentChanges\":";
    let start = body.find(key)? + key.len();
    let rest  = body[start..].trim_start();
    // rest starts with [ { "text": "..." } ]
    let brace = rest.find('{')?;
    let inner = &rest[brace..find_object_end(rest, brace)];
    json_str(inner, "text")
}

/// Find the closing '}' matching the '{' at `start` within `s`.
fn find_object_end(s: &str, start: usize) -> usize {
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut i = start;
    while i < bytes.len() {
        let b = bytes[i];
        if in_str {
            if b == b'\\' { i += 1; }
            else if b == b'"' { in_str = false; }
        } else {
            match b {
                b'"' => in_str = true,
                b'{' => depth += 1,
                b'}' => { depth -= 1; if depth == 0 { return i + 1; } }
                _    => {}
            }
        }
        i += 1;
    }
    s.len()
}

/// Parse a JSON string starting after the opening '"', handling \n \t \\ \".
fn parse_json_string(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    loop {
        match chars.next() {
            None | Some('"') => break,
            Some('\\') => match chars.next() {
                Some('n')  => out.push('\n'),
                Some('t')  => out.push('\t'),
                Some('r')  => out.push('\r'),
                Some('"')  => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('/') => out.push('/'),
                Some('b')  => out.push('\x08'),
                Some('f')  => out.push('\x0C'),
                Some('u')  => {
                    // skip 4 hex digits
                    for _ in 0..4 { chars.next(); }
                }
                _ => {}
            },
            Some(c) => out.push(c),
        }
    }
    out
}

/// Escape a string for JSON output.
fn json_escape(s: &str) -> String {
    let mut out = String::from('"');
    for ch in s.chars() {
        match ch {
            '"'  => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c    => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Convert a file:// URI to a PathBuf.
fn uri_to_path(uri: &str) -> Result<PathBuf, ()> {
    let path = uri.strip_prefix("file://").ok_or(())?;
    // On Windows: file:///C:/foo → /C:/foo → C:/foo
    #[cfg(windows)]
    let path = path.trim_start_matches('/');
    Ok(PathBuf::from(path))
}
