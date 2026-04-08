// volta/src/main.rs

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

fn main() {
    let args: Vec<String> = env::args().collect();

    // volta --help
    if args.len() < 2 || args[1] == "--help" || args[1] == "-h" {
        print_help();
        return;
    }

    // volta --version
    if args[1] == "--version" || args[1] == "-v" {
        println!("volta 0.2.0");
        return;
    }

    let input_path = PathBuf::from(&args[1]);

    if !input_path.exists() {
        error_exit(&format!("file not found: {}", input_path.display()));
    }

    let dir = input_path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let stem = input_path.file_stem().unwrap().to_string_lossy().to_string();
    let c_path   = dir.join(format!("{}.c", stem));
    let bin_path = dir.join(&stem);

    // Compile the full program (resolves imports recursively)
    let mut visited = HashSet::new();
    let combined = compile_file(&input_path, &dir, &mut visited);

    // Write C output
    fs::write(&c_path, &combined).unwrap_or_else(|e| {
        error_exit(&format!("error writing {}: {}", c_path.display(), e));
    });

    // Compile with clang/gcc
    let compiler = if which("clang") { "clang" } else { "cc" };
    let status = Command::new(compiler)
        .args([
            "-o", bin_path.to_str().unwrap(),
            c_path.to_str().unwrap(),
            "-std=c99",
            "-Wno-unused-function",
            "-Wno-unused-variable",
            "-lm",
        ])
        .status()
        .unwrap_or_else(|e| {
            error_exit(&format!("compiler not found: {}", e));
        });

    if !status.success() {
        eprintln!("\n\x1b[31merror\x1b[0m: C compilation failed");
        eprintln!("  check {} for details", c_path.display());
        process::exit(1);
    }

    // Auto-run — use absolute path to avoid working directory issues
    let abs_bin = bin_path.canonicalize().unwrap_or(bin_path.clone());
    let run_status = Command::new(&abs_bin)
        .args(&args[2..]) // pass remaining args to the program
        .status()
        .unwrap_or_else(|e| {
            error_exit(&format!("failed to run: {}", e));
        });

    if !run_status.success() {
        process::exit(run_status.code().unwrap_or(1));
    }
}

// Recursively compile a .vlt file, resolving imports
fn compile_file(path: &Path, base_dir: &Path, visited: &mut HashSet<PathBuf>) -> String {
    let canonical = path.canonicalize().unwrap_or(path.to_path_buf());

    if visited.contains(&canonical) {
        return String::new(); // already included
    }
    visited.insert(canonical.clone());

    let src = fs::read_to_string(path).unwrap_or_else(|_| {
        error_exit(&format!("cannot read file: {}", path.display()));
    });

    // Extract import statements before parsing
    let (imports, clean_src) = extract_imports(&src);

    // Resolve and compile each import first
    let mut imported_ast_stmts: Vec<ast::Stmt> = Vec::new();
    for import_name in &imports {
        let import_path = resolve_import(import_name, base_dir, path);
        if !import_path.exists() {
            print_error(
                path.to_str().unwrap_or("?"),
                0, 0,
                &format!("cannot find module '{}'", import_name),
                &format!("looked for: {}", import_path.display()),
            );
            process::exit(1);
        }
        let import_dir = import_path.parent().unwrap_or(base_dir);
        let import_src = fs::read_to_string(&import_path).unwrap_or_default();
        let (_, clean_import) = extract_imports(&import_src);
        let import_tokens = lex_with_errors(&clean_import, import_path.to_str().unwrap_or("?"));
        let import_prog   = parse_with_errors(import_tokens, import_path.to_str().unwrap_or("?"));
        // Only include fn/struct/extern defs from imports (not top-level statements)
        for stmt in import_prog.stmts {
            match &stmt {
                ast::Stmt::FnDef(_) | ast::Stmt::StructDef(_) | ast::Stmt::ExternBlock(_) | ast::Stmt::DeviceBlock(_) => {
                    imported_ast_stmts.push(stmt);
                }
                _ => {} // skip top-level code from imports
            }
        }
        // Recurse for nested imports
        let _ = compile_file(&import_path, import_dir, visited);
    }

    // Parse the main file
    let tokens = lex_with_errors(&clean_src, path.to_str().unwrap_or("?"));
    let mut prog = parse_with_errors(tokens, path.to_str().unwrap_or("?"));

    // Prepend imported definitions
    imported_ast_stmts.append(&mut prog.stmts);
    prog.stmts = imported_ast_stmts;

    // Type check
    let mut checker = sema::Checker::new();
    checker.check_program(&prog);

    // Print warnings
    for w in &checker.warnings {
        eprintln!("\x1b[33mwarning\x1b[0m: {}", w.msg);
        if !w.hint.is_empty() {
            eprintln!("  \x1b[36mhint\x1b[0m: {}", w.hint);
        }
    }

    // Print errors and exit if any
    if !checker.errors.is_empty() {
        for e in &checker.errors {
            eprintln!("\x1b[31merror\x1b[0m: {}", e.msg);
            if !e.hint.is_empty() {
                eprintln!("  \x1b[36mhint\x1b[0m: {}", e.hint);
            }
        }
        std::process::exit(1);
    }

    emit::Emitter::new().emit_program(&prog)
}

fn extract_imports(src: &str) -> (Vec<String>, String) {
    let mut imports = Vec::new();
    let mut clean = String::new();
    for line in src.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("import ") {
            // import "module_name"  or  import "path/to/module"
            let name = trimmed
                .trim_start_matches("import")
                .trim()
                .trim_matches('"')
                .to_string();
            imports.push(name);
            clean.push('\n'); // keep line numbers intact
        } else {
            clean.push_str(line);
            clean.push('\n');
        }
    }
    (imports, clean)
}

fn resolve_import(name: &str, base_dir: &Path, current_file: &Path) -> PathBuf {
    let current_dir = current_file.parent().unwrap_or(base_dir);
    // Try relative to current file first
    let relative = current_dir.join(format!("{}.vlt", name));
    // Canonicalize to resolve ../ etc
    if let Ok(canon) = relative.canonicalize() {
        if canon.exists() { return canon; }
    }
    if relative.exists() { return relative; }
    // Try relative to working directory
    let from_cwd = PathBuf::from(format!("{}.vlt", name));
    if let Ok(canon) = from_cwd.canonicalize() {
        if canon.exists() { return canon; }
    }
    if from_cwd.exists() { return from_cwd; }
    // Try stdlib location: ~/.volta/lib/
    if let Some(home) = dirs_home() {
        let stdlib = home.join(".volta").join("lib").join(format!("{}.vlt", name));
        if stdlib.exists() { return stdlib; }
    }
    relative // return even if doesn't exist — caller handles error
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

fn lex_with_errors(src: &str, filename: &str) -> Vec<lexer::Token> {
    lexer::Lexer::new(src).tokenize().unwrap_or_else(|e| {
        print_error(filename, e.line, 0, &e.msg, "");
        process::exit(1);
    })
}

fn parse_with_errors(tokens: Vec<lexer::Token>, filename: &str) -> ast::Program {
    parser::Parser::new(tokens).parse_program().unwrap_or_else(|e| {
        print_error(filename, e.line, 0, &e.msg, "");
        process::exit(1);
    })
}

// Pretty error display
fn print_error(filename: &str, line: usize, col: usize, msg: &str, hint: &str) {
    eprintln!("\n\x1b[31merror\x1b[0m: {}", msg);
    eprintln!("  \x1b[36m-->\x1b[0m {}:{}:{}", filename, line, col);
    // Try to show the source line
    if line > 0 {
        if let Ok(src) = std::fs::read_to_string(filename) {
            if let Some(src_line) = src.lines().nth(line.saturating_sub(1)) {
                eprintln!("   |");
                eprintln!("{:3}| {}", line, src_line);
                if col > 0 {
                    eprintln!("   | {}\x1b[31m^\x1b[0m", " ".repeat(col.saturating_sub(1)));
                } else {
                    eprintln!("   | \x1b[31m^\x1b[0m");
                }
                eprintln!("   |");
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
    println!("Volta programming language v0.2.0");
    println!();
    println!("USAGE:");
    println!("  volta <file.vlt> [args...]   compile and run a Volta program");
    println!("  volta --help                 show this message");
    println!("  volta --version              show version");
    println!();
    println!("EXAMPLES:");
    println!("  volta hello.vlt");
    println!("  volta script.vlt arg1 arg2");
    println!();
    println!("DOCS: https://github.com/V3lCryn/volta");
}

fn which(cmd: &str) -> bool {
    Command::new("which").arg(cmd).output().map(|o| o.status.success()).unwrap_or(false)
}
