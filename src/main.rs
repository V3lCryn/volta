// volta/src/main.rs — Volta compiler v0.2.0

mod lexer;
mod ast;
mod parser;
mod sema;
mod emit;

use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{self, Command};

const VERSION: &str = "0.2.0";

fn main() {
    let args: Vec<String> = env::args().collect();

    match args.get(1).map(|s| s.as_str()) {
        None | Some("--help") | Some("-h") => { print_help(); }
        Some("--version") | Some("-v")     => { println!("volta {}", VERSION); }
        Some("new")   => cmd_new(&args),
        Some("build") => cmd_build(&args, false),
        Some("run")   => cmd_build(&args, true),
        Some(path)    => {
            // Direct: volta file.vlt [args...]
            let pb = PathBuf::from(path);
            if !pb.exists() {
                error_exit(&format!("file not found: {}", path));
            }
            run_file(&pb, &args[2..], true);
        }
    }
}

// ── Commands ──────────────────────────────────────────────────────────────────

fn cmd_new(args: &[String]) {
    let name = args.get(2).unwrap_or_else(|| {
        eprintln!("usage: volta new <project-name>");
        process::exit(1);
    });

    let dir = PathBuf::from(name);
    if dir.exists() {
        error_exit(&format!("directory '{}' already exists", name));
    }

    fs::create_dir_all(dir.join("lib")).unwrap();
    fs::create_dir_all(dir.join("examples")).unwrap();

    // main.vlt
    fs::write(dir.join("main.vlt"), format!(
r#"-- {name} — a Volta project
-- run: volta main.vlt

fn greet(who: str) -> str
  return "Hello, {{who}}!"
end

let name = "{name}"
let msg = greet(name)
print(msg)
print("Built with Volta -- github.com/V3lCryn/volta")
"#, name=name)).unwrap();

    // lib/utils.vlt
    fs::write(dir.join("lib").join("utils.vlt"),
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
"#).unwrap();

    // .gitignore
    fs::write(dir.join(".gitignore"),
"*.c\ntarget/\n").unwrap();

    println!("\x1b[32m✓\x1b[0m Created project '{}'", name);
    println!("");
    println!("  \x1b[36mcd {}\x1b[0m", name);
    println!("  \x1b[36mvolta main.vlt\x1b[0m");
    println!("");
}

fn cmd_build(args: &[String], run: bool) {
    let path = args.get(2).unwrap_or_else(|| {
        eprintln!("usage: volta {} <file.vlt>", if run { "run" } else { "build" });
        process::exit(1);
    });
    let pb = PathBuf::from(path);
    if !pb.exists() { error_exit(&format!("file not found: {}", path)); }
    run_file(&pb, &args[3..], run);
}

// ── Core pipeline ─────────────────────────────────────────────────────────────

fn run_file(input_path: &Path, extra_args: &[String], do_run: bool) {
    let dir  = input_path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let stem = input_path.file_stem().unwrap().to_string_lossy().to_string();
    let c_path   = dir.join(format!("{}.c", stem));
    let bin_path = dir.join(&stem);

    // Compile
    let mut visited = HashSet::new();
    let c_code = compile_file(input_path, &dir, &mut visited);

    fs::write(&c_path, &c_code).unwrap_or_else(|e| {
        error_exit(&format!("cannot write {}: {}", c_path.display(), e));
    });

    let compiler = if which("clang") { "clang" } else { "cc" };
    let status = Command::new(compiler)
        .args([
            "-o", bin_path.to_str().unwrap(),
            c_path.to_str().unwrap(),
            "-std=c99",
            "-Wno-unused-function",
            "-Wno-unused-variable",
            "-Wno-int-conversion",
            "-lm",
        ])
        .status()
        .unwrap_or_else(|e| error_exit(&format!("compiler error: {}", e)));

    if !status.success() {
        eprintln!("\n\x1b[31merror\x1b[0m: compilation failed — check {}", c_path.display());
        process::exit(1);
    }

    if do_run {
        let abs_bin = bin_path.canonicalize().unwrap_or(bin_path.clone());
        let run_status = Command::new(&abs_bin)
            .args(extra_args)
            .status()
            .unwrap_or_else(|e| error_exit(&format!("run error: {}", e)));
        if !run_status.success() {
            process::exit(run_status.code().unwrap_or(1));
        }
    } else {
        println!("\x1b[32m✓\x1b[0m built: {}", bin_path.display());
    }
}

// ── Module resolution ─────────────────────────────────────────────────────────

fn compile_file(path: &Path, base_dir: &Path, visited: &mut HashSet<PathBuf>) -> String {
    let canonical = fs::canonicalize(path).unwrap_or(path.to_path_buf());
    if visited.contains(&canonical) { return String::new(); }
    visited.insert(canonical);

    let src = fs::read_to_string(path).unwrap_or_else(|_| {
        error_exit(&format!("cannot read: {}", path.display()));
    });

    let (imports, clean_src) = extract_imports(&src);

    let mut imported: Vec<ast::Stmt> = Vec::new();
    for imp in &imports {
        let imp_path = resolve_import(imp, path);
        if !imp_path.exists() {
            print_error(
                path.to_str().unwrap_or("?"), 0, 0,
                &format!("cannot find module '{}'", imp),
                &format!("looked for: {}", imp_path.display()),
            );
            process::exit(1);
        }
        let imp_src  = fs::read_to_string(&imp_path).unwrap_or_default();
        let (_, imp_clean) = extract_imports(&imp_src);
        let imp_dir  = imp_path.parent().unwrap_or(base_dir);
        let toks = lex_source(&imp_clean, imp_path.to_str().unwrap_or("?"));
        let prog = parse_tokens(toks, imp_path.to_str().unwrap_or("?"));
        for stmt in prog.stmts {
            match &stmt {
                ast::Stmt::FnDef(_) | ast::Stmt::StructDef(_) |
                ast::Stmt::ExternBlock(_) | ast::Stmt::DeviceBlock(_) => {
                    imported.push(stmt);
                }
                _ => {}
            }
        }
        compile_file(&imp_path, imp_dir, visited);
    }

    let toks = lex_source(&clean_src, path.to_str().unwrap_or("?"));
    let mut prog = parse_tokens(toks, path.to_str().unwrap_or("?"));
    imported.append(&mut prog.stmts);
    prog.stmts = imported;

    // Type check
    let mut checker = sema::Checker::new();
    checker.check_program(&prog);

    for w in &checker.warnings {
        eprintln!("\x1b[33mwarning\x1b[0m: {}", w.msg);
        if !w.hint.is_empty() { eprintln!("  \x1b[36mhint\x1b[0m: {}", w.hint); }
    }
    if !checker.errors.is_empty() {
        for e in &checker.errors {
            eprintln!("\x1b[31merror\x1b[0m: {}", e.msg);
            if !e.hint.is_empty() { eprintln!("  \x1b[36mhint\x1b[0m: {}", e.hint); }
        }
        process::exit(1);
    }

    emit::Emitter::new().emit_program(&prog)
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
    // Try relative to current file
    let rel = current_dir.join(format!("{}.vlt", name));
    if let Ok(c) = fs::canonicalize(&rel) { if c.exists() { return c; } }
    if rel.exists() { return rel; }
    // Try from cwd
    let cwd = PathBuf::from(format!("{}.vlt", name));
    if cwd.exists() { return cwd; }
    // Try ~/.volta/lib/
    if let Ok(home) = env::var("HOME") {
        let stdlib = PathBuf::from(home).join(".volta").join("lib").join(format!("{}.vlt", name));
        if stdlib.exists() { return stdlib; }
    }
    rel
}

fn lex_source(src: &str, filename: &str) -> Vec<lexer::Token> {
    lexer::Lexer::new(src).tokenize().unwrap_or_else(|e| {
        print_error(filename, e.line, e.col, &e.msg, "");
        process::exit(1);
    })
}

fn parse_tokens(tokens: Vec<lexer::Token>, filename: &str) -> ast::Program {
    parser::Parser::new(tokens).parse_program().unwrap_or_else(|e| {
        print_error(filename, e.line, 0, &e.msg, "");
        process::exit(1);
    })
}

// ── Error display ─────────────────────────────────────────────────────────────

fn print_error(filename: &str, line: usize, col: usize, msg: &str, hint: &str) {
    eprintln!("\n\x1b[31merror\x1b[0m: {}", msg);
    eprintln!("  \x1b[36m-->\x1b[0m {}:{}:{}", filename, line, col);
    if line > 0 {
        if let Ok(src) = fs::read_to_string(filename) {
            if let Some(src_line) = src.lines().nth(line.saturating_sub(1)) {
                eprintln!("   \x1b[36m|\x1b[0m");
                eprintln!("\x1b[36m{:3}\x1b[0m\x1b[36m|\x1b[0m {}", line, src_line);
                let spaces = if col > 1 { col - 1 } else { 0 };
                eprintln!("   \x1b[36m|\x1b[0m {}\x1b[31m^\x1b[0m", " ".repeat(spaces));
                eprintln!("   \x1b[36m|\x1b[0m");
            }
        }
    }
    if !hint.is_empty() {
        eprintln!("  \x1b[33mhint\x1b[0m: {}", hint);
    }
    eprintln!();
}

fn error_exit(msg: &str) -> ! {
    eprintln!("\n\x1b[31merror\x1b[0m: {}\n", msg);
    process::exit(1);
}

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
