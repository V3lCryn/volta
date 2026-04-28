// volta/src/error.rs — unified error type for the Volta compiler

use std::fmt;

/// Source location attached to a diagnostic.
#[derive(Debug, Clone, Default)]
pub struct Span {
    pub file: String,
    pub line: usize,
    pub col:  usize,
}

impl Span {
    pub fn new(file: impl Into<String>, line: usize, col: usize) -> Self {
        Span { file: file.into(), line, col }
    }
}

/// The compiler-wide error type. Each variant is one pipeline phase.
#[derive(Debug)]
pub enum VoltaError {
    /// Invalid character, unterminated string, bad escape, etc.
    Lex {
        span:     Span,
        msg:      String,
        src_line: String,
    },
    /// Unexpected token, missing delimiter, invalid syntax, etc.
    Parse {
        span: Span,
        msg:  String,
    },
    /// Type mismatches, undefined names, etc. (batched — all reported at once).
    Sema {
        errors: Vec<(usize, String, String)>, // (line, msg, hint)
        file:   String,
    },
    /// Internal emit failure — always indicates a compiler bug.
    Emit {
        msg: String,
    },
    /// File I/O error.
    Io {
        path:   String,
        detail: String,
    },
    /// The C compiler reported errors.
    CCompile {
        stderr: String,
        c_path: String,
    },
    /// Import resolution failure.
    Module {
        name:     String,
        searched: String,
    },
}

impl fmt::Display for VoltaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VoltaError::Lex   { msg, .. }         => write!(f, "lex error: {}", msg),
            VoltaError::Parse { msg, .. }          => write!(f, "parse error: {}", msg),
            VoltaError::Sema  { errors, .. }       => write!(f, "{} semantic error(s)", errors.len()),
            VoltaError::Emit  { msg }              => write!(f, "emit error: {}", msg),
            VoltaError::Io    { path, detail }     => write!(f, "{}: {}", path, detail),
            VoltaError::CCompile { .. }            => write!(f, "C compilation failed"),
            VoltaError::Module { name, .. }        => write!(f, "module '{}' not found", name),
        }
    }
}

impl std::error::Error for VoltaError {}

// ── Public renderer ───────────────────────────────────────────────────────────

/// Print `err` to stderr with rustc-style formatting.
pub fn render_error(err: &VoltaError) {
    match err {
        VoltaError::Lex { span, msg, src_line } => {
            eprintln!("\n\x1b[31;1merror\x1b[0m: {}", msg);
            eprintln!("  \x1b[36m-->\x1b[0m {}:{}:{}", span.file, span.line, span.col);
            if span.line > 0 && !src_line.is_empty() {
                let spaces = span.col.saturating_sub(1);
                eprintln!("   \x1b[36m|\x1b[0m");
                eprintln!("\x1b[36m{:3}|\x1b[0m {}", span.line, src_line);
                eprintln!("   \x1b[36m|\x1b[0m {}\x1b[31m^\x1b[0m", " ".repeat(spaces));
                eprintln!("   \x1b[36m|\x1b[0m");
            }
            eprintln!();
        }

        VoltaError::Parse { span, msg } => {
            eprintln!("\n\x1b[31;1merror\x1b[0m: {}", msg);
            eprintln!("  \x1b[36m-->\x1b[0m {}:{}:{}", span.file, span.line, span.col);
            if span.line > 0 {
                render_src_context(&span.file, span.line, span.col);
            }
            eprintln!();
        }

        VoltaError::Sema { errors, file } => {
            for (line, msg, hint) in errors {
                eprintln!("\n\x1b[31;1merror\x1b[0m: {}", msg);
                if *line > 0 {
                    eprintln!("  \x1b[36m-->\x1b[0m {}:{}", file, line);
                    render_src_context(file, *line, 0);
                }
                if !hint.is_empty() {
                    eprintln!("  \x1b[36mhint\x1b[0m: {}", hint);
                }
            }
            eprintln!();
        }

        VoltaError::Emit { msg } => {
            eprintln!("\n\x1b[31;1merror\x1b[0m [internal]: {}", msg);
            eprintln!("  This is a Volta compiler bug — please report it.");
            eprintln!("  https://github.com/V3lCryn/volta/issues");
            eprintln!();
        }

        VoltaError::Io { path, detail } => {
            eprintln!("\n\x1b[31;1merror\x1b[0m: {}", detail);
            eprintln!("  \x1b[36m-->\x1b[0m {}", path);
            eprintln!();
        }

        VoltaError::CCompile { stderr, c_path } => {
            eprintln!("\n\x1b[31;1merror\x1b[0m: C compilation failed");
            eprintln!("  \x1b[36m-->\x1b[0m {}", c_path);
            if !stderr.is_empty() {
                eprintln!();
                for line in stderr.lines() {
                    eprintln!("  {}", line);
                }
            }
            eprintln!();
        }

        VoltaError::Module { name, searched } => {
            eprintln!("\n\x1b[31;1merror\x1b[0m: cannot find module '{}'", name);
            eprintln!("  \x1b[36msearched\x1b[0m: {}", searched);
            eprintln!("  \x1b[36mhint\x1b[0m: install it to ~/.volta/lib/{}.vlt", name);
            eprintln!();
        }
    }
}

/// Print a single warning to stderr (does not stop compilation).
pub fn render_warning(file: &str, line: usize, col: usize, msg: &str, hint: &str) {
    eprintln!("\n\x1b[33;1mwarning\x1b[0m: {}", msg);
    if line > 0 {
        eprintln!("  \x1b[36m-->\x1b[0m {}:{}", file, line);
        render_src_context(file, line, col);
    }
    if !hint.is_empty() {
        eprintln!("  \x1b[36mhint\x1b[0m: {}", hint);
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────────

fn render_src_context(file: &str, line: usize, col: usize) {
    if let Ok(src) = std::fs::read_to_string(file) {
        if let Some(src_line) = src.lines().nth(line.saturating_sub(1)) {
            eprintln!("   \x1b[36m|\x1b[0m");
            eprintln!("\x1b[36m{:3}|\x1b[0m {}", line, src_line);
            if col > 0 {
                let spaces = col.saturating_sub(1);
                eprintln!("   \x1b[36m|\x1b[0m {}\x1b[31m^\x1b[0m", " ".repeat(spaces));
            }
            eprintln!("   \x1b[36m|\x1b[0m");
        }
    }
}
