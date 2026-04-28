// volta/src/sema.rs
// Semantic analysis — type checking, undefined variable detection,
// return path checking. Runs after parsing, before emission.

use crate::ast::*;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum VType {
    Int, Float, Bool, Str, Nil, Ptr,
    Struct(String),
    Enum(String),
    Array(String),    // element type name, e.g. "i64", "str"
    Pointer(String),  // pointee type name, e.g. "i64" for *i64
    Result,
    Unknown,
}

impl VType {
    pub fn from_str(s: &str) -> Self {
        if s.starts_with('*') {
            return VType::Pointer(s[1..].to_string());
        }
        if s.starts_with('[') && s.ends_with(']') {
            let inner = &s[1..s.len()-1];
            if let Some(semi) = inner.find(';') {
                // Fixed-size array [i64;8] — track element type
                return VType::Array(inner[..semi].trim().to_string());
            }
            return VType::Array(inner.to_string());
        }
        match s {
            "i8"|"i16"|"i32"|"i64"|"u8"|"u16"|"u32"|"u64"|"int" => VType::Int,
            "f32"|"f64"|"float" => VType::Float,
            "bool"   => VType::Bool,
            "str"    => VType::Str,
            "nil"    => VType::Nil,
            "ptr"    => VType::Ptr,
            "Result" => VType::Result,
            other    => VType::Struct(other.to_string()),
        }
    }
    pub fn to_display(&self) -> String {
        match self {
            VType::Int          => "int".into(),
            VType::Float        => "float".into(),
            VType::Bool         => "bool".into(),
            VType::Str          => "str".into(),
            VType::Nil          => "nil".into(),
            VType::Ptr          => "ptr".into(),
            VType::Struct(s)    => s.clone(),
            VType::Enum(s)      => s.clone(),
            VType::Array(e)     => format!("[{}]", e),
            VType::Pointer(t)   => format!("*{}", t),
            VType::Result       => "Result".into(),
            VType::Unknown      => "?".into(),
        }
    }
}

#[derive(Debug)]
pub struct SemaError {
    pub msg:  String,
    pub line: usize,
    pub hint: String,
}

impl SemaError {
    fn new(msg: impl Into<String>, line: usize, hint: impl Into<String>) -> Self {
        SemaError { msg: msg.into(), line, hint: hint.into() }
    }
}

impl std::fmt::Display for SemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.msg)
    }
}

impl std::error::Error for SemaError {}

pub struct Checker {
    scopes:           Vec<HashMap<String, VType>>,
    fn_types:         HashMap<String, VType>,
    fn_param_counts:  HashMap<String, usize>,
    structs:          HashMap<String, Vec<(String, VType)>>,
    enums:            HashMap<String, Vec<String>>,
    pub errors:       Vec<SemaError>,
    pub warnings:     Vec<SemaError>,
    current_line:     usize,
    current_fn_ret:   Option<VType>,
}

impl Checker {
    pub fn new() -> Self {
        let mut c = Checker {
            scopes: vec![HashMap::new()],
            fn_types: HashMap::new(),
            fn_param_counts: HashMap::new(),
            structs: HashMap::new(),
            enums: HashMap::new(),
            errors: Vec::new(),
            warnings: Vec::new(),
            current_line: 0,
            current_fn_ret: None,
        };
        // Register built-in functions
        c.fn_types.insert("Ok".into(),            VType::Result);
        c.fn_types.insert("Err".into(),           VType::Result);
        c.fn_types.insert("print".into(),        VType::Nil);
        c.fn_types.insert("input".into(),        VType::Str);
        c.fn_types.insert("int_to_str".into(),   VType::Str);
        c.fn_types.insert("float_to_str".into(), VType::Str);
        c.fn_types.insert("bool_to_str".into(),  VType::Str);
        c.fn_types.insert("str_len".into(),      VType::Int);
        c.fn_types.insert("str_eq".into(),       VType::Bool);
        c.fn_types.insert("str_contains".into(), VType::Bool);
        c.fn_types.insert("str_find".into(),     VType::Int);
        c.fn_types.insert("str_slice".into(),    VType::Str);
        c.fn_types.insert("str_replace".into(),  VType::Str);
        c.fn_types.insert("char_at".into(),      VType::Int);
        c.fn_types.insert("char_from".into(),    VType::Str);
        c.fn_types.insert("to_int".into(),       VType::Int);
        c.fn_types.insert("to_float".into(),     VType::Float);
        c.fn_types.insert("hex".into(),          VType::Str);
        c.fn_types.insert("hash_str".into(),     VType::Int);
        c.fn_types.insert("entropy".into(),      VType::Float);
        c.fn_types.insert("rot13".into(),        VType::Str);
        c.fn_types.insert("caesar".into(),       VType::Str);
        c.fn_types.insert("xor_str".into(),      VType::Str);
        c.fn_types.insert("xor_bytes".into(),    VType::Nil);
        c.fn_types.insert("bytes_to_hex".into(), VType::Str);
        c.fn_types.insert("is_printable".into(), VType::Bool);
        c.fn_types.insert("is_alpha".into(),     VType::Bool);
        c.fn_types.insert("is_digit_char".into(),VType::Bool);
        c.fn_types.insert("abs".into(),          VType::Int);
        c.fn_types.insert("max".into(),          VType::Int);
        c.fn_types.insert("min".into(),          VType::Int);
        c.fn_types.insert("pow".into(),          VType::Int);
        c.fn_types.insert("fsqrt".into(),        VType::Float);
        c.fn_types.insert("ffloor".into(),       VType::Float);
        c.fn_types.insert("fceil".into(),        VType::Float);
        c.fn_types.insert("arg_count".into(),    VType::Int);
        c.fn_types.insert("arg_get".into(),      VType::Str);
        c.fn_types.insert("sleep_ms".into(),     VType::Nil);
        c.fn_types.insert("len".into(),          VType::Int);
        c.fn_types.insert("str".into(),          VType::Str);
        c.fn_types.insert("arr_len".into(),      VType::Int);
        c.fn_types.insert("push".into(),         VType::Nil);
        c.fn_types.insert("pop".into(),          VType::Int);
        c.fn_types.insert("hex_dump".into(),     VType::Nil);
        c.fn_types.insert("xor_bytes".into(),    VType::Nil);
        c.fn_types.insert("bytes_to_hex".into(), VType::Str);
        c.fn_types.insert("str_to_hex".into(),   VType::Str);
        // strings stdlib
        c.fn_types.insert("str_upper".into(),       VType::Str);
        c.fn_types.insert("str_lower".into(),       VType::Str);
        c.fn_types.insert("str_reverse".into(),     VType::Str);
        c.fn_types.insert("str_repeat".into(),      VType::Str);
        c.fn_types.insert("str_pad_left".into(),    VType::Str);
        c.fn_types.insert("str_pad_right".into(),   VType::Str);
        c.fn_types.insert("str_starts_with".into(), VType::Bool);
        c.fn_types.insert("str_ends_with".into(),   VType::Bool);
        c.fn_types.insert("str_index_of".into(),    VType::Int);
        c.fn_types.insert("str_trim".into(),        VType::Str);
        c.fn_types.insert("str_split_at".into(),    VType::Str);
        // crypto stdlib
        c.fn_types.insert("xor_encrypt".into(),     VType::Str);
        c.fn_types.insert("str_to_hex_str".into(),  VType::Str);
        c.fn_types.insert("looks_base64".into(),    VType::Bool);
        c.fn_types.insert("looks_encrypted".into(), VType::Bool);
        c.fn_types.insert("is_b64_char".into(),     VType::Bool);
        // math stdlib
        c.fn_types.insert("is_prime".into(),        VType::Bool);
        c.fn_types.insert("fibonacci".into(),       VType::Int);
        c.fn_types.insert("gcd".into(),             VType::Int);
        c.fn_types.insert("factorial".into(),       VType::Int);
        c.fn_types.insert("clamp".into(),           VType::Int);
        // TCP sockets
        c.fn_types.insert("tcp_connect".into(),    VType::Int);
        c.fn_types.insert("tcp_listen".into(),     VType::Int);
        c.fn_types.insert("tcp_accept".into(),     VType::Int);
        c.fn_types.insert("tcp_send".into(),       VType::Bool);
        c.fn_types.insert("tcp_send_bytes".into(), VType::Bool);
        c.fn_types.insert("tcp_recv".into(),       VType::Str);
        c.fn_types.insert("tcp_recv_line".into(),  VType::Str);
        c.fn_types.insert("tcp_close".into(),      VType::Nil);
        c.fn_types.insert("tcp_ok".into(),         VType::Bool);
        c.fn_types.insert("tcp_peer_ip".into(),    VType::Str);
        // PostgreSQL
        c.fn_types.insert("pg_connect".into(),  VType::Bool);
        c.fn_types.insert("pg_close".into(),    VType::Nil);
        c.fn_types.insert("pg_ok".into(),       VType::Bool);
        c.fn_types.insert("pg_error".into(),    VType::Str);
        c.fn_types.insert("pg_exec".into(),     VType::Bool);
        c.fn_types.insert("pg_escape".into(),   VType::Str);
        c.fn_types.insert("pg_query".into(),    VType::Unknown);
        c.fn_types.insert("pg_rows".into(),     VType::Int);
        c.fn_types.insert("pg_value".into(),    VType::Str);
        c.fn_types.insert("pg_free".into(),     VType::Nil);
        // http stdlib
        c.fn_types.insert("http_get".into(),        VType::Str);
        c.fn_types.insert("http_post".into(),       VType::Str);
        c.fn_types.insert("http_status".into(),     VType::Str);
        c.fn_types.insert("http_body".into(),       VType::Str);
        c.fn_types.insert("http_host".into(),       VType::Str);
        c.fn_types.insert("http_path".into(),       VType::Str);
        // File I/O
        c.fn_types.insert("file_read".into(),     VType::Str);
        c.fn_types.insert("file_write".into(),    VType::Bool);
        c.fn_types.insert("file_append".into(),   VType::Bool);
        c.fn_types.insert("file_exists".into(),   VType::Bool);
        c.fn_types.insert("file_delete".into(),   VType::Bool);
        c.fn_types.insert("file_size".into(),     VType::Int);
        c.fn_types.insert("file_readline".into(), VType::Str);
        c
    }

    pub fn check_program(&mut self, prog: &Program) {
        // First pass: register all type signatures
        for stmt in &prog.stmts {
            match stmt {
                Stmt::Const { name, ty, value, line } => {
                    let val_ty = if let Some(ann) = ty {
                        VType::from_str(ann)
                    } else {
                        match value {
                            Expr::Integer(_)   => VType::Int,
                            Expr::Float(_)     => VType::Float,
                            Expr::Bool(_)      => VType::Bool,
                            Expr::StringLit(_) => VType::Str,
                            _                  => VType::Unknown,
                        }
                    };
                    self.current_line = *line;
                    self.define(name, val_ty);
                }
                Stmt::FnDef(f) => {
                    let ret = f.ret_ty.as_deref().map(VType::from_str).unwrap_or(VType::Nil);
                    self.fn_types.insert(f.name.clone(), ret);
                    self.fn_param_counts.insert(f.name.clone(), f.params.len());
                }
                Stmt::StructDef(s) => {
                    let fields = s.fields.iter()
                        .map(|(n, t)| (n.clone(), VType::from_str(t)))
                        .collect();
                    self.structs.insert(s.name.clone(), fields);
                }
                Stmt::PackedStructDef(ps) => {
                    let fields = ps.fields.iter()
                        .map(|(n, _)| (n.clone(), VType::Int))
                        .collect();
                    self.structs.insert(ps.name.clone(), fields);
                }
                Stmt::EnumDef(e) => {
                    self.enums.insert(e.name.clone(), e.variants.clone());
                }
                Stmt::ExternBlock(eb) => {
                    for f in &eb.fns {
                        let ret = f.ret_ty.as_deref().map(VType::from_str).unwrap_or(VType::Nil);
                        self.fn_types.insert(f.name.clone(), ret);
                    }
                }
                _ => {}
            }
        }
        // Second pass: check bodies
        for stmt in &prog.stmts { self.check_stmt(stmt); }
    }

    fn push_scope(&mut self) { self.scopes.push(HashMap::new()); }
    fn pop_scope(&mut self)  { self.scopes.pop(); }

    fn define(&mut self, name: &str, ty: VType) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string(), ty);
        }
    }

    fn lookup(&self, name: &str) -> Option<&VType> {
        for scope in self.scopes.iter().rev() {
            if let Some(t) = scope.get(name) { return Some(t); }
        }
        None
    }

    fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { name, ty, value, line } => {
                self.current_line = *line;
                let val_ty = self.check_expr(value);
                if let Some(ann) = ty {
                    let ann_ty = VType::from_str(ann);
                    // Array annotations are always compatible with array literals
                    let is_array_ann = matches!(&ann_ty, VType::Array(_));
                    if !is_array_ann
                       && ann_ty != VType::Unknown && val_ty != VType::Unknown
                       && ann_ty != val_ty && val_ty != VType::Nil {
                        let ok = matches!((&ann_ty, &val_ty), (VType::Float, VType::Int) | (VType::Int, VType::Float));
                        if !ok {
                            self.errors.push(SemaError::new(
                                format!("type mismatch: '{}' declared as '{}' but got '{}'",
                                    name, ann_ty.to_display(), val_ty.to_display()),
                                self.current_line,
                                "change the type annotation or the value",
                            ));
                        }
                    }
                    self.define(name, ann_ty);
                } else {
                    self.define(name, val_ty);
                }
            }
            Stmt::Assign { target, value, line } => {
                self.current_line = *line;
                let val_ty = self.check_expr(value);
                match target {
                    AssignTarget::Ident(name) => {
                        if self.lookup(name).is_none() {
                            self.errors.push(SemaError::new(
                                format!("assignment to undefined variable '{}'", name),
                                self.current_line,
                                format!("declare it first with: let {} = ...", name),
                            ));
                        }
                        for scope in self.scopes.iter_mut().rev() {
                            if scope.contains_key(name) {
                                scope.insert(name.clone(), val_ty.clone());
                                break;
                            }
                        }
                    }
                    AssignTarget::Deref(ptr_expr) => {
                        let ptr_ty = self.check_expr(ptr_expr);
                        if !matches!(ptr_ty, VType::Pointer(_) | VType::Ptr | VType::Unknown) {
                            self.errors.push(SemaError::new(
                                format!("cannot assign through non-pointer type '{}'", ptr_ty.to_display()),
                                self.current_line,
                                "use a pointer (*T) as the dereference target",
                            ));
                        }
                    }
                    _ => { self.check_expr(value); }
                }
            }
            Stmt::Const { name, ty, value, line } => {
                self.current_line = *line;
                let val_ty = self.check_expr(value);
                if let Some(ann) = ty {
                    let ann_ty = VType::from_str(ann);
                    if ann_ty != VType::Unknown && val_ty != VType::Unknown
                       && ann_ty != val_ty && val_ty != VType::Nil {
                        let ok = matches!((&ann_ty, &val_ty), (VType::Float, VType::Int) | (VType::Int, VType::Float));
                        if !ok {
                            self.errors.push(SemaError::new(
                                format!("const '{}': type '{}' doesn't match value type '{}'",
                                    name, ann_ty.to_display(), val_ty.to_display()),
                                self.current_line, "change the annotation or the value",
                            ));
                        }
                    }
                }
            }
            Stmt::FnDef(f) => {
                self.current_line = f.line;
                self.push_scope();
                let prev_ret = self.current_fn_ret.take();
                self.current_fn_ret = f.ret_ty.as_deref().map(VType::from_str);
                for p in &f.params {
                    let ty = p.ty.as_deref().map(VType::from_str).unwrap_or(VType::Unknown);
                    self.define(&p.name, ty);
                }
                for s in &f.body { self.check_stmt(s); }
                self.current_fn_ret = prev_ret;
                self.pop_scope();
            }
            Stmt::If { cond, then_body, else_ifs, else_body, line } => {
                self.current_line = *line;
                let cty = self.check_expr(cond);
                if cty != VType::Bool && cty != VType::Unknown {
                    self.warnings.push(SemaError::new(
                        format!("condition has type '{}', expected 'bool'", cty.to_display()),
                        self.current_line,
                        "wrap in a comparison like: x != 0",
                    ));
                }
                self.push_scope(); for s in then_body { self.check_stmt(s); } self.pop_scope();
                for (ec, eb) in else_ifs {
                    self.check_expr(ec);
                    self.push_scope(); for s in eb { self.check_stmt(s); } self.pop_scope();
                }
                if let Some(eb) = else_body {
                    self.push_scope(); for s in eb { self.check_stmt(s); } self.pop_scope();
                }
            }
            Stmt::While { cond, body, line } => {
                self.current_line = *line;
                self.check_expr(cond);
                self.push_scope(); for s in body { self.check_stmt(s); } self.pop_scope();
            }
            Stmt::For { var, iter, body, line } => {
                self.current_line = *line;
                self.check_expr(iter);
                self.push_scope();
                self.define(var, VType::Int);
                for s in body { self.check_stmt(s); }
                self.pop_scope();
            }
            Stmt::ForIndex { idx, var, iter, body, line } => {
                self.current_line = *line;
                self.check_expr(iter);
                self.push_scope();
                self.define(idx, VType::Int);
                self.define(var, VType::Int);
                for s in body { self.check_stmt(s); }
                self.pop_scope();
            }
            Stmt::ExprStmt(e) => { self.check_expr(e); }
            Stmt::Return(Some(e)) => {
                let ret_ty = self.check_expr(e);
                if let Some(expected) = &self.current_fn_ret.clone() {
                    if ret_ty != VType::Unknown && *expected != VType::Unknown
                       && ret_ty != *expected {
                        let ok = matches!((expected, &ret_ty),
                            (VType::Float, VType::Int) | (VType::Int, VType::Float) |
                            (VType::Result, _) | (_, VType::Result));
                        if !ok {
                            self.errors.push(SemaError::new(
                                format!("return type mismatch: expected '{}', got '{}'",
                                    expected.to_display(), ret_ty.to_display()),
                                self.current_line,
                                format!("function declared to return '{}'", expected.to_display()),
                            ));
                        }
                    }
                }
            }
            Stmt::Return(None) => {}
            Stmt::EnumDef(_) | Stmt::PackedStructDef(_) => {} // registered in first pass
            Stmt::Match { expr, arms, line } => {
                self.current_line = *line;
                let match_ty = self.check_expr(expr);
                // Validate each arm pattern against the matched enum
                for arm in arms {
                    if let MatchPattern::Variant { enum_name, variant } = &arm.pattern {
                        if let Some(variants) = self.enums.get(enum_name) {
                            if !variants.contains(variant) {
                                self.errors.push(SemaError::new(
                                    format!("enum '{}' has no variant '{}'", enum_name, variant),
                                    self.current_line,
                                    format!("valid variants: {}", variants.join(", ")),
                                ));
                            }
                        } else if match_ty != VType::Unknown {
                            self.errors.push(SemaError::new(
                                format!("'{}' is not a known enum", enum_name),
                                self.current_line, "",
                            ));
                        }
                    }
                    self.push_scope();
                    for s in &arm.body { self.check_stmt(s); }
                    self.pop_scope();
                }
            }
            _ => {}
        }
    }

    fn check_expr(&mut self, expr: &Expr) -> VType {
        match expr {
            Expr::Nil          => VType::Nil,
            Expr::Bool(_)      => VType::Bool,
            Expr::Integer(_)   => VType::Int,
            Expr::Float(_)     => VType::Float,
            Expr::StringLit(_) => VType::Str,

            Expr::Ident(name) => {
                // Check built-in names first
                match name.as_str() {
                    "int_to_str"|"float_to_str"|"bool_to_str"|"str_len"|"str_eq"|
                    "str_contains"|"str_find"|"str_slice"|"str_replace"|"char_at"|
                    "char_from"|"hex"|"hash_str"|"rot13"|"caesar"|"xor_str"|
                    "entropy"|"print"|"input"|"arg_get"|"arg_count" => return VType::Unknown,
                    _ => {}
                }
                if let Some(ty) = self.lookup(name) {
                    ty.clone()
                } else if self.fn_types.contains_key(name.as_str()) {
                    VType::Unknown
                } else {
                    self.errors.push(SemaError::new(
                        format!("undefined variable '{}'", name),
                        self.current_line,
                        format!("declare it with: let {} = ...", name),
                    ));
                    VType::Unknown
                }
            }

            Expr::BinOp { op, left, right } => {
                let lt = self.check_expr(left);
                let rt = self.check_expr(right);
                match op {
                    BinOp::Add | BinOp::Sub => {
                        // Pointer arithmetic: *T + int → *T
                        if matches!(&lt, VType::Pointer(_)) { lt }
                        else if matches!(&rt, VType::Pointer(_)) { rt }
                        else if lt == VType::Float || rt == VType::Float { VType::Float }
                        else { VType::Int }
                    }
                    BinOp::Mul | BinOp::Div | BinOp::Mod => {
                        if lt == VType::Float || rt == VType::Float { VType::Float }
                        else { VType::Int }
                    }
                    BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::LtEq |
                    BinOp::Gt | BinOp::GtEq | BinOp::And | BinOp::Or => VType::Bool,
                    BinOp::Concat => VType::Str,
                    _ => VType::Int,
                }
            }

            Expr::UnaryOp { op, expr } => {
                let t = self.check_expr(expr);
                match op {
                    UnaryOp::Not    => VType::Bool,
                    UnaryOp::Neg    => t,
                    UnaryOp::BitNot => VType::Int,
                    UnaryOp::Ref    => {
                        // &x has type *T where x has type T
                        VType::Pointer(t.to_display())
                    }
                    UnaryOp::Deref  => {
                        // *p has type T where p has type *T
                        match t {
                            VType::Pointer(inner) => VType::from_str(&inner),
                            VType::Ptr            => VType::Unknown, // void* deref
                            VType::Unknown        => VType::Unknown,
                            other => {
                                self.errors.push(SemaError::new(
                                    format!("cannot dereference non-pointer type '{}'", other.to_display()),
                                    self.current_line,
                                    "only pointer types (*T) can be dereferenced",
                                ));
                                VType::Unknown
                            }
                        }
                    }
                }
            }

            Expr::Call { name, args } => {
                // push(arr, val) — validate element type when arr is typed
                if name == "push" && args.len() == 2 {
                    let arr_ty = self.check_expr(&args[0]);
                    let val_ty = self.check_expr(&args[1]);
                    if let VType::Array(elem) = &arr_ty {
                        let expected = VType::from_str(elem);
                        if val_ty != VType::Unknown && expected != VType::Unknown && val_ty != expected {
                            let ok = matches!((&expected, &val_ty), (VType::Float, VType::Int) | (VType::Int, VType::Float));
                            if !ok {
                                self.errors.push(SemaError::new(
                                    format!("type mismatch: array is '[{}]' but pushed value is '{}'",
                                        elem, val_ty.to_display()),
                                    self.current_line,
                                    format!("push a '{}' value", elem),
                                ));
                            }
                        }
                    }
                    return VType::Nil;
                }
                for a in args { self.check_expr(a); }
                // Validate arg count for user-defined functions
                if let Some(&expected) = self.fn_param_counts.get(name) {
                    if args.len() != expected {
                        self.errors.push(SemaError::new(
                            format!("'{}' expects {} argument(s), got {}",
                                name, expected, args.len()),
                            self.current_line,
                            format!("check the definition of '{}'", name),
                        ));
                    }
                }
                if let Some(t) = self.fn_types.get(name) {
                    t.clone()
                } else {
                    self.errors.push(SemaError::new(
                        format!("call to undefined function '{}'", name),
                        self.current_line,
                        format!("define it with: fn {}(...) ... end", name),
                    ));
                    VType::Unknown
                }
            }

            Expr::Cast { expr, ty } => {
                self.check_expr(expr);
                VType::from_str(ty)
            }

            Expr::Field { target, field } => {
                // Check for enum variant access: EnumName.Variant
                if let Expr::Ident(name) = target.as_ref() {
                    if let Some(variants) = self.enums.get(name) {
                        if variants.contains(field) {
                            return VType::Enum(name.clone());
                        } else {
                            self.errors.push(SemaError::new(
                                format!("enum '{}' has no variant '{}'", name, field),
                                self.current_line,
                                format!("valid variants: {}", variants.join(", ")),
                            ));
                            return VType::Unknown;
                        }
                    }
                }
                // Struct field access
                let t = self.check_expr(target);
                if let VType::Struct(sname) = &t {
                    if let Some(fields) = self.structs.get(sname.as_str()) {
                        if let Some((_, ft)) = fields.iter().find(|(n, _)| n == field) {
                            return ft.clone();
                        } else {
                            self.errors.push(SemaError::new(
                                format!("struct '{}' has no field '{}'", sname, field),
                                self.current_line, "",
                            ));
                        }
                    }
                }
                VType::Unknown
            }

            Expr::StructLit { name, fields } => {
                for (_, v) in fields { self.check_expr(v); }
                VType::Struct(name.clone())
            }

            Expr::Try(inner) => {
                let t = self.check_expr(inner);
                if t != VType::Result && t != VType::Unknown {
                    self.errors.push(SemaError::new(
                        format!("'?' applied to non-Result type '{}'", t.to_display()),
                        self.current_line,
                        "only use '?' on expressions that return Result",
                    ));
                }
                VType::Unknown
            }

            Expr::Index { target, index } => {
                self.check_expr(index);
                let arr_ty = self.check_expr(target);
                if let VType::Array(elem) = arr_ty {
                    VType::from_str(&elem)
                } else {
                    VType::Unknown
                }
            }

            _ => { VType::Unknown }
        }
    }
}
