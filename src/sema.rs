// volta/src/sema.rs
// Semantic analysis — type checking, undefined variable detection,
// return path checking. Runs after parsing, before emission.

use crate::ast::*;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum VType {
    Int, Float, Bool, Str, Nil, Ptr,
    Struct(String),
    Unknown, // inference failed — not an error
}

impl VType {
    pub fn from_str(s: &str) -> Self {
        match s {
            "i8"|"i16"|"i32"|"i64"|"u8"|"u16"|"u32"|"u64"|"int" => VType::Int,
            "f32"|"f64"|"float" => VType::Float,
            "bool"  => VType::Bool,
            "str"   => VType::Str,
            "nil"   => VType::Nil,
            "ptr"   => VType::Ptr,
            other   => VType::Struct(other.to_string()),
        }
    }
    pub fn to_display(&self) -> &str {
        match self {
            VType::Int      => "int",
            VType::Float    => "float",
            VType::Bool     => "bool",
            VType::Str      => "str",
            VType::Nil      => "nil",
            VType::Ptr      => "ptr",
            VType::Struct(s)=> s,
            VType::Unknown  => "?",
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
    // stack of scopes: each is a map from name -> type 
    scopes:   Vec<HashMap<String, VType>>,
    fn_types: HashMap<String, VType>,
    structs:  HashMap<String, Vec<(String, VType)>>,
    pub errors: Vec<SemaError>,
    pub warnings: Vec<SemaError>,
}

impl Checker {
    pub fn new() -> Self {
        let mut c = Checker {
            scopes: vec![HashMap::new()],
            fn_types: HashMap::new(),
            structs: HashMap::new(),
            errors: Vec::new(),
            warnings: Vec::new(),
        };
        // Register built-in functions
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
        // First pass: register all function signatures
        for stmt in &prog.stmts {
            match stmt {
                Stmt::FnDef(f) => {
                    let ret = f.ret_ty.as_deref().map(VType::from_str).unwrap_or(VType::Nil);
                    self.fn_types.insert(f.name.clone(), ret);
                }
                Stmt::StructDef(s) => {
                    let fields = s.fields.iter()
                        .map(|(n, t)| (n.clone(), VType::from_str(t)))
                        .collect();
                    self.structs.insert(s.name.clone(), fields);
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
            Stmt::Let { name, ty, value } => {
                let val_ty = self.check_expr(value);
                if let Some(ann) = ty {
                    let ann_ty = VType::from_str(ann);
                    if ann_ty != VType::Unknown && val_ty != VType::Unknown
                       && ann_ty != val_ty && val_ty != VType::Nil {
                        // Allow int -> float coercion
                        let ok = matches!((&ann_ty, &val_ty), (VType::Float, VType::Int) | (VType::Int, VType::Float));
                        if !ok {
                            self.errors.push(SemaError::new(
                                format!("type mismatch: '{}' declared as '{}' but got '{}'",
                                    name, ann_ty.to_display(), val_ty.to_display()),
                                0,
                                format!("change the type annotation or the value"),
                            ));
                        }
                    }
                    self.define(name, ann_ty);
                } else {
                    self.define(name, val_ty);
                }
            }
            Stmt::Assign { target, value } => {
                let val_ty = self.check_expr(value);
                if let AssignTarget::Ident(name) = target {
                    if self.lookup(name).is_none() {
                        self.errors.push(SemaError::new(
                            format!("assignment to undefined variable '{}'", name),
                            0,
                            format!("declare it first with: let {} = ...", name),
                        ));
                    }
                    // Update type in scope
                    for scope in self.scopes.iter_mut().rev() {
                        if scope.contains_key(name) {
                            scope.insert(name.clone(), val_ty.clone());
                            break;
                        }
                    }
                }
            }
            Stmt::FnDef(f) => {
                self.push_scope();
                for p in &f.params {
                    let ty = p.ty.as_deref().map(VType::from_str).unwrap_or(VType::Unknown);
                    self.define(&p.name, ty);
                }
                for s in &f.body { self.check_stmt(s); }
                self.pop_scope();
            }
            Stmt::If { cond, then_body, else_ifs, else_body } => {
                let cty = self.check_expr(cond);
                if cty != VType::Bool && cty != VType::Unknown {
                    self.warnings.push(SemaError::new(
                        format!("condition has type '{}', expected 'bool'", cty.to_display()),
                        0, String::from("wrap in a comparison like: x != 0"),
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
            Stmt::While { cond, body } => {
                self.check_expr(cond);
                self.push_scope(); for s in body { self.check_stmt(s); } self.pop_scope();
            }
            Stmt::For { var, iter, body } => {
                self.check_expr(iter);
                self.push_scope();
                self.define(var, VType::Int);
                for s in body { self.check_stmt(s); }
                self.pop_scope();
            }
            Stmt::ForIndex { idx, var, iter, body } => {
                self.check_expr(iter);
                self.push_scope();
                self.define(idx, VType::Int);
                self.define(var, VType::Int);
                for s in body { self.check_stmt(s); }
                self.pop_scope();
            }
            Stmt::ExprStmt(e) => { self.check_expr(e); }
            Stmt::Return(Some(e)) => { self.check_expr(e); }
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
                        0,
                        format!("declare it with: let {} = ...", name),
                    ));
                    VType::Unknown
                }
            }

            Expr::BinOp { op, left, right } => {
                let lt = self.check_expr(left);
                let rt = self.check_expr(right);
                match op {
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
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
                }
            }

            Expr::Call { name, args } => {
                for a in args { self.check_expr(a); }
                if let Some(t) = self.fn_types.get(name) {
                    t.clone()
                } else {
                    self.errors.push(SemaError::new(
                        format!("call to undefined function '{}'", name),
                        0,
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
                let t = self.check_expr(target);
                if let VType::Struct(sname) = &t {
                    if let Some(fields) = self.structs.get(sname.as_str()) {
                        if let Some((_, ft)) = fields.iter().find(|(n, _)| n == field) {
                            return ft.clone();
                        } else {
                            self.errors.push(SemaError::new(
                                format!("struct '{}' has no field '{}'", sname, field),
                                0, String::new(),
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

            _ => { VType::Unknown }
        }
    }
}
