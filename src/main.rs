// volta/src/main.rs — Volta compiler v0.3.0

mod error;
mod lexer;
mod ast;
mod parser;
mod sema;
mod emit;

use error::{VoltaError, Span};
use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{self, Command};

const VERSION: &str = "0.3.0";

fn main() {
    let args: Vec<String> = env::args().collect();

    match args.get(1).map(|s| s.as_str()) {
        None | Some("--help") | Some("-h") => { print_help(); }
        Some("--version") | Some("-v")     => { println!("volta {}", VERSION); }
        Some("new")   => cmd_new(&args),
        Some("build") => cmd_build(&args, false),
        Some("run")   => cmd_build(&args, true),
        Some(path)    => {
            let pb = PathBuf::from(path);
            if !pb.exists() {
                error::render_error(&VoltaError::Io {
                    path:   path.to_string(),
                    detail: "file not found".into(),
                });
                process::exit(1);
            }
            run_file(&pb, &args[2..], true);
        }
    }
}

// ── Commands ──────────────────────────────────────────────────────────────────

fn cmd_new(args: &[String]) {
    let name = match args.get(2) {
        Some(n) => n,
        None => {
            eprintln!("usage: volta new <project-name>");
            process::exit(1);
        }
    };

    let dir = PathBuf::from(name);
    if dir.exists() {
        error::render_error(&VoltaError::Io {
            path:   name.clone(),
            detail: format!("directory '{}' already exists", name),
        });
        process::exit(1);
    }

    let write = |path: PathBuf, content: &str| {
        fs::write(&path, content).unwrap_or_else(|e| {
            error::render_error(&VoltaError::Io {
                path:   path.display().to_string(),
                detail: e.to_string(),
            });
            process::exit(1);
        });
    };

    fs::create_dir_all(dir.join("lib")).unwrap_or_else(|e| {
        error::render_error(&VoltaError::Io { path: name.clone(), detail: e.to_string() });
        process::exit(1);
    });
    fs::create_dir_all(dir.join("examples")).unwrap_or_else(|e| {
        error::render_error(&VoltaError::Io { path: name.clone(), detail: e.to_string() });
        process::exit(1);
    });

    write(dir.join("main.vlt"), &format!(
r#"-- {name} — a Volta project
-- run: volta main.vlt

import "lib/utils"

fn greet(who: str) -> str
  return "Hello, {{who}}!"
end

let name = "{name}"
let msg = greet(name)
print(msg)
print("Built with Volta — github.com/V3lCryn/volta")
"#, name=name));

    write(dir.join("lib").join("utils.vlt"),
r#"-- utils.vlt — shared utilities for this project

fn repeat_str(s: str, n: i64) -> str
  let result: str = ""
  let i: i64 = 0
  while i < n do
    result = result .. s
    i += 1
  end
  return result
end

fn clamp(val: i64, lo: i64, hi: i64) -> i64
  if val < lo do return lo end
  if val > hi do return hi end
  return val
end
"#);

    write(dir.join(".gitignore"), "*.c\ntarget/\n");

    println!("\x1b[32m✓\x1b[0m Created project '{}'", name);
    println!();
    println!("  \x1b[36mcd {}\x1b[0m", name);
    println!("  \x1b[36mvolta main.vlt\x1b[0m");
    println!();
}

fn cmd_build(args: &[String], run: bool) {
    let path = match args.get(2) {
        Some(p) => p,
        None => {
            eprintln!("usage: volta {} <file.vlt>", if run { "run" } else { "build" });
            process::exit(1);
        }
    };
    let pb = PathBuf::from(path);
    if !pb.exists() {
        error::render_error(&VoltaError::Io {
            path:   path.clone(),
            detail: "file not found".into(),
        });
        process::exit(1);
    }
    run_file(&pb, &args[3..], run);
}

// ── Core pipeline ─────────────────────────────────────────────────────────────

fn run_file(input_path: &Path, extra_args: &[String], do_run: bool) {
    if let Err(e) = try_run_file(input_path, extra_args, do_run) {
        error::render_error(&e);
        process::exit(1);
    }
}

fn try_run_file(input_path: &Path, extra_args: &[String], do_run: bool) -> Result<(), VoltaError> {
    let dir  = input_path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let stem = input_path
        .file_stem()
        .ok_or_else(|| VoltaError::Io {
            path:   input_path.display().to_string(),
            detail: "invalid file name".into(),
        })?
        .to_string_lossy()
        .to_string();

    let c_path   = dir.join(format!("{}.c",  stem));
    let bin_path = dir.join(&stem);

    let c_code = compile_file(input_path)?;

    fs::write(&c_path, &c_code).map_err(|e| VoltaError::Io {
        path:   c_path.display().to_string(),
        detail: e.to_string(),
    })?;

    let compiler = if which("clang") { "clang" } else { "cc" };

    let bin_str = bin_path.to_str().unwrap_or("a.out");
    let c_str   = c_path.to_str().unwrap_or("out.c");

    let output = Command::new(compiler)
        .args([
            "-o", bin_str, c_str,
            "-std=c99",
            "-Wno-unused-function",
            "-Wno-unused-variable",
            "-Wno-int-conversion",
            "-lm",
            "-I/usr/local/opt/libpq/include",
            "-L/usr/local/opt/libpq/lib",
            "-lpq",
        ])
        .output()
        .map_err(|e| VoltaError::Io {
            path:   compiler.to_string(),
            detail: format!("could not start C compiler: {}", e),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(VoltaError::CCompile {
            stderr,
            c_path: c_path.display().to_string(),
        });
    }

    if do_run {
        let abs_bin = bin_path.canonicalize().unwrap_or_else(|_| bin_path.clone());
        let run_status = Command::new(&abs_bin)
            .args(extra_args)
            .status()
            .map_err(|e| VoltaError::Io {
                path:   abs_bin.display().to_string(),
                detail: format!("could not run binary: {}", e),
            })?;
        if !run_status.success() {
            process::exit(run_status.code().unwrap_or(1));
        }
    } else {
        println!("\x1b[32m✓\x1b[0m built: {}", bin_path.display());
    }

    Ok(())
}

// ── Module resolution ─────────────────────────────────────────────────────────

fn compile_file(path: &Path) -> Result<String, VoltaError> {
    let filename = path.to_string_lossy().into_owned();
    let mut visited = HashSet::new();
    let all_stmts = collect_stmts(path, &mut visited)?;
    let prog = ast::Program { stmts: all_stmts };

    let mut checker = sema::Checker::new();
    checker.check_program(&prog);

    for w in &checker.warnings {
        error::render_warning(&filename, w.line, 0, &w.msg, &w.hint);
    }

    if !checker.errors.is_empty() {
        let errors = checker.errors.iter()
            .map(|e| (e.line, e.msg.clone(), e.hint.clone()))
            .collect();
        return Err(VoltaError::Sema { errors, file: filename });
    }

    emit::Emitter::new()
        .emit_program(&prog)
        .map_err(|e| VoltaError::Emit { msg: e.msg })
}

// Recursively collect all stmts from a file and its transitive imports.
// Imported files contribute only definitions (no top-level code).
fn collect_stmts(
    path:    &Path,
    visited: &mut HashSet<PathBuf>,
) -> Result<Vec<ast::Stmt>, VoltaError> {
    let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if visited.contains(&canonical) { return Ok(Vec::new()); }
    visited.insert(canonical.clone());

    let filename = path.to_string_lossy().into_owned();
    let src = fs::read_to_string(path).map_err(|e| VoltaError::Io {
        path:   filename.clone(),
        detail: e.to_string(),
    })?;

    let (imports, clean_src) = extract_imports(&src);
    let mut all_stmts: Vec<ast::Stmt> = Vec::new();

    for imp in &imports {
        let imp_path = resolve_import(imp, path);
        if !imp_path.exists() {
            return Err(VoltaError::Module {
                name:     imp.clone(),
                searched: imp_path.display().to_string(),
            });
        }
        // Recursively collect transitive imports from this module
        let imp_stmts = collect_stmts(&imp_path, visited)?;
        // Only definitions cross module boundaries — no top-level code
        for stmt in imp_stmts {
            match &stmt {
                ast::Stmt::FnDef(_)
                | ast::Stmt::StructDef(_)
                | ast::Stmt::PackedStructDef(_)
                | ast::Stmt::EnumDef(_)
                | ast::Stmt::ExternBlock(_)
                | ast::Stmt::DeviceBlock(_) => { all_stmts.push(stmt); }
                _ => {}
            }
        }
    }

    // Parse this file and append its stmts after the imports
    let toks = lex_source(&clean_src, &filename)?;
    let prog  = parse_tokens(toks, &filename)?;
    all_stmts.extend(prog.stmts);
    Ok(all_stmts)
}

fn extract_imports(src: &str) -> (Vec<String>, String) {
    let mut imports = Vec::new();
    let mut clean   = String::new();
    for line in src.lines() {
        let t = line.trim();
        if t.starts_with("import ") {
            let name = t.trim_start_matches("import").trim().trim_matches('"').to_string();
            imports.push(name);
            clean.push('\n');
        } else {
            clean.push_str(line);
            clean.push('\n');
        }
    }
    (imports, clean)
}

fn resolve_import(name: &str, current_file: &Path) -> PathBuf {
    let current_dir = current_file.parent().unwrap_or(Path::new("."));

    // 1. Relative to the importing file
    let rel = current_dir.join(format!("{}.vlt", name));
    if rel.exists() { return rel; }

    // 2. lib/ subdirectory next to the importing file
    let lib_rel = current_dir.join("lib").join(format!("{}.vlt", name));
    if lib_rel.exists() { return lib_rel; }

    // 3. Current working directory
    let cwd = PathBuf::from(format!("{}.vlt", name));
    if cwd.exists() { return cwd; }

    // 4. ~/.volta/lib/
    if let Ok(home) = env::var("HOME") {
        let stdlib = PathBuf::from(&home).join(".volta").join("lib").join(format!("{}.vlt", name));
        if stdlib.exists() { return stdlib; }
    }
    // 5. VOLTA_LIB env var (for custom stdlib locations)
    if let Ok(lib_dir) = env::var("VOLTA_LIB") {
        let p = PathBuf::from(&lib_dir).join(format!("{}.vlt", name));
        if p.exists() { return p; }
    }

    // Not found — return the relative path so the caller can report a good error
    rel
}

// ── Per-phase adapters ────────────────────────────────────────────────────────

fn lex_source(src: &str, filename: &str) -> Result<Vec<lexer::Token>, VoltaError> {
    lexer::Lexer::new(src).tokenize().map_err(|e| VoltaError::Lex {
        span:     Span::new(filename, e.line, e.col),
        msg:      e.msg.clone(),
        src_line: e.src_line.clone(),
    })
}

fn parse_tokens(tokens: Vec<lexer::Token>, filename: &str) -> Result<ast::Program, VoltaError> {
    parser::Parser::new(tokens).parse_program().map_err(|e| VoltaError::Parse {
        span: Span::new(filename, e.line, e.col),
        msg:  e.msg.clone(),
    })
}

// ── UI helpers ────────────────────────────────────────────────────────────────

fn print_help() {
    println!("\x1b[1mVolta\x1b[0m {} — a scripting language that compiles to C", VERSION);
    println!();
    println!("\x1b[1mUSAGE\x1b[0m");
    println!("  volta <file.vlt>           compile and run");
    println!("  volta run <file.vlt>       compile and run");
    println!("  volta build <file.vlt>     compile only");
    println!("  volta new <project>        create a new project");
    println!("  volta --version            show version");
    println!("  volta --help               show this message");
    println!();
    println!("\x1b[1mEXAMPLES\x1b[0m");
    println!("  volta hello.vlt");
    println!("  volta new myproject && cd myproject && volta main.vlt");
    println!();
    println!("\x1b[1mDOCS\x1b[0m");
    println!("  https://github.com/V3lCryn/volta");
}

fn which(cmd: &str) -> bool {
    Command::new("which").arg(cmd).output()
        .map(|o| o.status.success()).unwrap_or(false)
}
