// volta/src/emit.rs

use crate::ast::*;
use std::collections::{HashMap, HashSet};

pub struct Emitter {
    out:          String,
    indent:       usize,
    fn_types:     HashMap<String, String>,
    var_types:    HashMap<String, String>,
    struct_defs:  HashMap<String, Vec<(String, String)>>,
    struct_names: HashSet<String>,
    tmp_counter:  usize,
}

impl Emitter {
    pub fn new() -> Self {
        Emitter {
            out: String::new(), indent: 0,
            fn_types: HashMap::new(), var_types: HashMap::new(),
            struct_defs: HashMap::new(), struct_names: HashSet::new(),
            tmp_counter: 0,
        }
    }

    fn tmp(&mut self) -> String {
        self.tmp_counter += 1;
        format!("_vt{}", self.tmp_counter)
    }

    pub fn emit_program(mut self, prog: &Program) -> String {
        self.line("#include <stdio.h>");
        self.line("#include <stdint.h>");
        self.line("#include <string.h>");
        self.line("#include <stdbool.h>");
        self.line("#include <stdlib.h>");
        self.line("#include <stdarg.h>");
        self.line("#include <math.h>");
        self.line("");
        self.emit_runtime();

        // First pass: collect all type info
        for stmt in &prog.stmts {
            match stmt {
                Stmt::StructDef(s) => {
                    self.struct_defs.insert(s.name.clone(), s.fields.clone());
                    self.struct_names.insert(s.name.clone());
                }
                Stmt::FnDef(f) => {
                    let ret = self.resolve_ty(f.ret_ty.as_deref());
                    self.fn_types.insert(f.name.clone(), ret);
                }
                Stmt::ExternBlock(eb) => {
                    for f in &eb.fns {
                        let ret = self.resolve_ty(f.ret_ty.as_deref());
                        self.fn_types.insert(f.name.clone(), ret);
                    }
                }
                _ => {}
            }
        }

        // Struct definitions
        for stmt in &prog.stmts {
            if let Stmt::StructDef(s) = stmt { self.emit_struct_def(s); }
        }

        // Extern declarations
        for stmt in &prog.stmts {
            if let Stmt::ExternBlock(eb) = stmt { self.emit_extern_block(eb); }
        }

        // Device macros
        for stmt in &prog.stmts {
            if let Stmt::DeviceBlock(db) = stmt { self.emit_device_block(db); }
        }

        // Forward-declare all functions
        let mut has_fns = false;
        for stmt in &prog.stmts {
            if let Stmt::FnDef(f) = stmt {
                let ret = self.resolve_ty(f.ret_ty.as_deref());
                let params = self.emit_params(&f.params);
                let safe = Self::safe_name(&f.name);
                self.line(&format!("{} {}({});", ret, safe, params));
                has_fns = true;
            }
        }
        if has_fns { self.line(""); }

        // Function bodies
        for stmt in &prog.stmts {
            if let Stmt::FnDef(f) = stmt { self.emit_fn_def(f); self.line(""); }
        }

        // Top-level → main()
        let top: Vec<&Stmt> = prog.stmts.iter().filter(|s|
            !matches!(s, Stmt::FnDef(_)|Stmt::ExternBlock(_)|Stmt::DeviceBlock(_)|Stmt::StructDef(_))
        ).collect();

        if !top.is_empty() {
            self.line("int main(int argc, char** argv) {");
            self.indent += 1;
            self.iline("_argc=argc; _argv=argv;");
            for stmt in top { self.emit_stmt(stmt); }
            self.iline("return 0;");
            self.indent -= 1;
            self.line("}");
        }

        self.out
    }

    // ── Runtime ───────────────────────────────────────────────────────────────

    fn emit_runtime(&mut self) {
        self.line("// ── Volta runtime ──────────────────────────────────────────");
        self.line("static char _vbuf[131072]; static int _vpos = 0;");
        // String concat
        self.line("static const char* _concat(const char* a, const char* b) {");
        self.line("    int la=(int)strlen(a),lb=(int)strlen(b); char* d=_vbuf+_vpos;");
        self.line("    if(_vpos+la+lb+1>131072)_vpos=0;");
        self.line("    memcpy(d,a,la); memcpy(d+la,b,lb); d[la+lb]='\\0'; _vpos+=la+lb+1; return d;");
        self.line("}");
        // print
        self.line("static void print(const char* s){puts(s);}");
        // int_to_str
        self.line("static const char* int_to_str(int64_t n){char*d=_vbuf+_vpos;int k=snprintf(d,32,\"%lld\",(long long)n);_vpos=(_vpos+k+1)%131072;return d;}");
        // float_to_str
        self.line("static const char* float_to_str(double n){char*d=_vbuf+_vpos;int k=snprintf(d,32,\"%g\",n);_vpos=(_vpos+k+1)%131072;return d;}");
        // bool_to_str
        self.line("static const char* bool_to_str(bool b){return b?\"true\":\"false\";}");
        // str_len
        self.line("static int64_t str_len(const char* s){return (int64_t)strlen(s);}");
        // str_eq
        self.line("static bool str_eq(const char* a,const char* b){return strcmp(a,b)==0;}");
        // str_contains
        self.line("static bool str_contains(const char* hay,const char* needle){return strstr(hay,needle)!=NULL;}");
        // to_int
        self.line("static int64_t to_int(const char* s){return (int64_t)atoll(s);}");
        // to_float
        self.line("static double to_float(const char* s){return atof(s);}");
        // input (read line from stdin)
        self.line("static const char* input(void){char*d=_vbuf+_vpos;if(!fgets(d,1024,stdin))return\"\";int n=(int)strlen(d);if(n>0&&d[n-1]=='\\n')d[n-1]='\\0';_vpos=(_vpos+n+1)%131072;return d;}");
        // math builtins
        self.line("static int64_t volta_abs(int64_t n){return n<0?-n:n;}");
        self.line("static int64_t volta_max(int64_t a,int64_t b){return a>b?a:b;}");
        self.line("static int64_t volta_min(int64_t a,int64_t b){return a<b?a:b;}");
        self.line("static int64_t volta_pow(int64_t b,int64_t e){int64_t r=1;while(e-->0)r*=b;return r;}");
        self.line("static double  fabs_v(double n){return fabs(n);}");
        self.line("static double  fsqrt(double n){return sqrt(n);}");
        self.line("static double  ffloor(double n){return floor(n);}");
        self.line("static double  fceil(double n){return ceil(n);}");
        // array helpers (dynamic arrays via malloc)
        self.line("typedef struct{void*data;int64_t len;int64_t cap;int64_t elem;}VArray;");
        self.line("static VArray _arr_new(int64_t elem){VArray a;a.data=malloc(8*elem);a.len=0;a.cap=8;a.elem=elem;return a;}");
        self.line("static void _arr_push(VArray*a,void*v){if(a->len>=a->cap){a->cap*=2;a->data=realloc(a->data,a->cap*a->elem);}memcpy((char*)a->data+a->len*a->elem,v,a->elem);a->len++;}");
        self.line("static int64_t arr_len(VArray a){return a.len;}");
        // ── Cyber / low-level builtins ──────────────────────────────
        self.line(r#"static const char* hex(int64_t n){char*d=_vbuf+_vpos;int k=snprintf(d,32,"0x%llx",(unsigned long long)n);_vpos=(_vpos+k+1)%131072;return d;}"#);
        self.line(r#"static void hex_dump(const void* ptr, int64_t len){"#);
        self.line(r#"    const unsigned char* p=(const unsigned char*)ptr;"#);
        self.line(r#"    for(int64_t i=0;i<len;i++){if(i%16==0&&i>0)printf("\n");printf("%02x ",p[i]);}printf("\n");}"#);
        self.line(r#"static const char* bytes_to_hex(const unsigned char* b,int64_t len){"#);
        self.line(r#"    char*d=_vbuf+_vpos;int pos=0;"#);
        self.line(r#"    for(int64_t i=0;i<len&&pos<131000;i++)pos+=snprintf(d+pos,8,"%02x",b[i]);"#);
        self.line(r#"    _vpos=(_vpos+pos+1)%131072;return d;}"#);
        self.line(r#"static void xor_bytes(unsigned char* buf,int64_t len,unsigned char key){for(int64_t i=0;i<len;i++)buf[i]^=key;}"#);
        self.line(r#"static const char* xor_str(const char* s,int64_t key){"#);
        self.line(r#"    int64_t len=(int64_t)strlen(s);char*d=_vbuf+_vpos;"#);
        self.line(r#"    for(int64_t i=0;i<len;i++)d[i]=(char)(s[i]^(char)key);"#);
        self.line(r#"    d[len]='\0';_vpos=(_vpos+len+1)%131072;return d;}"#);
        self.line(r#"static const char* rot13(const char* s){"#);
        self.line(r#"    int64_t len=(int64_t)strlen(s);char*d=_vbuf+_vpos;"#);
        self.line(r#"    for(int64_t i=0;i<len;i++){char c=s[i];"#);
        self.line(r#"        if(c>='a'&&c<='z')d[i]=(c-'a'+13)%26+'a';"#);
        self.line(r#"        else if(c>='A'&&c<='Z')d[i]=(c-'A'+13)%26+'A';"#);
        self.line(r#"        else d[i]=c;}"#);
        self.line(r#"    d[len]='\0';_vpos=(_vpos+len+1)%131072;return d;}"#);
        self.line(r#"static const char* caesar(const char* s,int64_t shift){"#);
        self.line(r#"    int64_t len=(int64_t)strlen(s);char*d=_vbuf+_vpos;shift=((shift%26)+26)%26;"#);
        self.line(r#"    for(int64_t i=0;i<len;i++){char c=s[i];"#);
        self.line(r#"        if(c>='a'&&c<='z')d[i]=(c-'a'+shift)%26+'a';"#);
        self.line(r#"        else if(c>='A'&&c<='Z')d[i]=(c-'A'+shift)%26+'A';"#);
        self.line(r#"        else d[i]=c;}"#);
        self.line(r#"    d[len]='\0';_vpos=(_vpos+len+1)%131072;return d;}"#);
        self.line(r#"static int64_t hash_str(const char* s){int64_t h=5381;int c;while((c=*s++))h=((h<<5)+h)+c;return h;}"#);
        self.line(r#"#include <ctype.h>"#);
        self.line(r#"static bool is_printable(int64_t c){return isprint((int)c)!=0;}"#);
        self.line(r#"static bool is_alpha(int64_t c){return isalpha((int)c)!=0;}"#);
        self.line(r#"static bool is_digit_char(int64_t c){return isdigit((int)c)!=0;}"#);
        self.line(r#"static int64_t char_at(const char* s,int64_t i){return (int64_t)(unsigned char)s[i];}"#);
        self.line(r#"static const char* char_from(int64_t n){char*d=_vbuf+_vpos;d[0]=(char)n;d[1]='\0';_vpos=(_vpos+2)%131072;return d;}"#);
        self.line(r#"static const char* str_slice(const char*s,int64_t start,int64_t len){"#);
        self.line(r#"    char*d=_vbuf+_vpos;int64_t slen=(int64_t)strlen(s);"#);
        self.line(r#"    if(start<0)start=0;if(start+len>slen)len=slen-start;if(len<0)len=0;"#);
        self.line(r#"    memcpy(d,s+start,len);d[len]='\0';_vpos=(_vpos+len+1)%131072;return d;}"#);
        self.line(r#"static int64_t str_find(const char*h,const char*n){const char*p=strstr(h,n);return p?p-h:-1;}"#);
        self.line(r#"static const char* str_replace(const char*s,const char*from,const char*to){"#);
        self.line(r#"    const char*p=strstr(s,from);if(!p)return s;"#);
        self.line(r#"    char*d=_vbuf+_vpos;int64_t pre=p-s,fl=strlen(from),tl=strlen(to);"#);
        self.line(r#"    memcpy(d,s,pre);memcpy(d+pre,to,tl);strcpy(d+pre+tl,p+fl);"#);
        self.line(r#"    int64_t total=pre+tl+strlen(p+fl);_vpos=(_vpos+total+1)%131072;return d;}"#);
        self.line(r#"static double entropy(const char* s){"#);
        self.line(r#"    int64_t freq[256]={0},len=(int64_t)strlen(s);if(len==0)return 0.0;"#);
        self.line(r#"    for(int64_t i=0;i<len;i++)freq[(unsigned char)s[i]]++;"#);
        self.line(r#"    double e=0.0;for(int i=0;i<256;i++){if(freq[i]>0){double p=(double)freq[i]/len;e-=p*log2(p);}}"#);
        self.line(r#"    return e;}"#);
        self.line(r#"#include <time.h>"#);
        self.line(r#"static void sleep_ms(int64_t ms){struct timespec t;t.tv_sec=ms/1000;t.tv_nsec=(ms%1000)*1000000;nanosleep(&t,NULL);}"#);
        self.line(r#"static int    _argc=0;"#);
        self.line(r#"static char** _argv=NULL;"#);
        self.line(r#"static int64_t  arg_count(void){return (int64_t)_argc;}"#);
        self.line(r#"static const char* arg_get(int64_t i){if(i<0||i>=_argc)return"";return _argv[i];}"#);
                self.line("// ───────────────────────────────────────────────────────────");
        self.line("");
    }

    // ── Type resolution ───────────────────────────────────────────────────────

    fn resolve_ty(&self, ty: Option<&str>) -> String {
        match ty {
            None | Some("nil")        => "void".into(),
            Some("i8")                => "int8_t".into(),
            Some("i16")               => "int16_t".into(),
            Some("i32")               => "int32_t".into(),
            Some("i64") | Some("int") => "int64_t".into(),
            Some("u8")                => "uint8_t".into(),
            Some("u16")               => "uint16_t".into(),
            Some("u32")               => "uint32_t".into(),
            Some("u64")               => "uint64_t".into(),
            Some("f32")               => "float".into(),
            Some("f64") | Some("float") => "double".into(),
            Some("bool")              => "bool".into(),
            Some("str")               => "const char*".into(),
            Some("ptr")               => "void*".into(),
            Some(name) if self.struct_names.contains(name) => name.to_string(),
            Some(other)               => other.to_string(), // pass through unknown
        }
    }

    fn infer_type(&self, expr: &Expr) -> String {
        match expr {
            Expr::Bool(_)                          => "bool".into(),
            Expr::Integer(_)                       => "int64_t".into(),
            Expr::Float(_)                         => "double".into(),
            Expr::StringLit(_)                     => "const char*".into(),
            Expr::Nil                              => "void*".into(),
            Expr::Cast { ty, .. }                  => self.resolve_ty(Some(ty)),
            Expr::BinOp { op: BinOp::Concat, .. } => "const char*".into(),
            Expr::BinOp { op: BinOp::Add, left,..}
            | Expr::BinOp { op: BinOp::Sub, left,..}
            | Expr::BinOp { op: BinOp::Mul, left,..}
            | Expr::BinOp { op: BinOp::Div, left,..} => self.infer_type(left),
            Expr::BinOp { op: BinOp::Eq, .. }
            | Expr::BinOp { op: BinOp::NotEq, .. }
            | Expr::BinOp { op: BinOp::Lt, .. }
            | Expr::BinOp { op: BinOp::Gt, .. }
            | Expr::BinOp { op: BinOp::And, .. }
            | Expr::BinOp { op: BinOp::Or, .. }   => "bool".into(),
            Expr::BinOp { .. }                    => "int64_t".into(),
            Expr::Ident(name)                     => {
                // Look up declared type from var_types
                match self.var_types.get(name).map(|s| s.as_str()) {
                    Some("f64"|"f32"|"float") => "double".into(),
                    Some("bool")              => "bool".into(),
                    Some("str")               => "const char*".into(),
                    Some("i8")                => "int8_t".into(),
                    Some("i16")               => "int16_t".into(),
                    Some("i32")               => "int32_t".into(),
                    Some("i64"|"int")         => "int64_t".into(),
                    Some(other) if self.struct_names.contains(other) => other.to_string(),
                    _                         => "int64_t".into(), // default numeric
                }
            }
            Expr::Call { name, .. }               => {
                self.fn_types.get(name).cloned()
                    .unwrap_or_else(|| "int64_t".into())
            }
            Expr::StructLit { name, .. }          => name.clone(),
            Expr::Array(_)                        => "VArray".into(),
            _ => "void*".into(),
        }
    }

    // ── Struct ────────────────────────────────────────────────────────────────

    fn emit_struct_def(&mut self, s: &StructDef) {
        self.line(&format!("typedef struct {} {{", s.name));
        for (fname, fty) in &s.fields {
            let cty = self.resolve_ty(Some(fty));
            self.line(&format!("    {} {};", cty, fname));
        }
        self.line(&format!("}} {};\n", s.name));
    }

    // ── Extern ────────────────────────────────────────────────────────────────

    fn emit_extern_block(&mut self, eb: &ExternBlock) {
        const SKIP: &[&str] = &["printf","puts","malloc","free","memcpy","strlen","scanf","fgets","atoi","atof","atoll","sqrt","floor","ceil","fabs"];
        for f in &eb.fns {
            if SKIP.contains(&f.name.as_str()) { continue; }
            let ret = self.resolve_ty(f.ret_ty.as_deref());
            let params = self.emit_params(&f.params);
            self.line(&format!("extern {} {}({});", ret, f.name, params));
        }
        self.line("");
    }

    // ── Device ────────────────────────────────────────────────────────────────

    fn emit_device_block(&mut self, db: &DeviceBlock) {
        self.line(&format!("// @device \"{}\" at {:#x}", db.name, db.address));
        let mut offset = 0u64;
        for reg in &db.regs {
            let cty = self.resolve_ty(Some(&reg.ty));
            let mname = format!("{}_{}", db.name.to_uppercase(), reg.name.to_uppercase());
            self.line(&format!("#define {} (*((volatile {}*)({:#x}UL+{}UL)))", mname, cty, db.address, offset));
            offset += size_of_ty(&reg.ty);
        }
        self.line("");
    }

    // ── Function ──────────────────────────────────────────────────────────────

    fn emit_fn_def(&mut self, f: &FnDef) {
        let ret = self.resolve_ty(f.ret_ty.as_deref());
        let params = self.emit_params(&f.params);
        let safe = Self::safe_name(&f.name);
        self.line(&format!("{} {}({}) {{", ret, safe, params));
        self.indent += 1;
        for p in &f.params {
            if let Some(ty) = &p.ty {
                self.var_types.insert(p.name.clone(), ty.clone());
            }
        }
        for stmt in &f.body { self.emit_stmt(stmt); }
        self.indent -= 1;
        self.line("}");
    }

    // ── Statements ────────────────────────────────────────────────────────────

    fn emit_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { name, ty, value } => {
                let c_ty = if let Some(t) = ty {
                    self.resolve_ty(Some(t))
                } else {
                    self.infer_type(value)
                };
                if let Some(t) = ty {
                    self.var_types.insert(name.clone(), t.clone());
                } else {
                    // For Cast, track the cast target type directly
                    if let Expr::Cast { ty: cast_ty, .. } = value {
                        self.var_types.insert(name.clone(), cast_ty.clone());
                    } else {
                        // Track inferred type so coerce_str works
                        let inferred = self.infer_type(value);
                        let volta_ty = match inferred.as_str() {
                            "const char*" => "str",
                            "double"      => "f64",
                            "float"       => "f32",
                            "bool"        => "bool",
                            _             => "i64",
                        };
                        self.var_types.insert(name.clone(), volta_ty.to_string());
                    }
                }

                // Special case: array literal → VArray
                if let Expr::Array(elems) = value {
                    if elems.is_empty() {
                        self.iline(&format!("VArray {} = _arr_new(sizeof(int64_t));", name));
                    } else {
                        let elem_ty = self.infer_type(&elems[0]);
                        self.iline(&format!("VArray {} = _arr_new(sizeof({}));", name, elem_ty));
                        for el in elems {
                            let v = self.emit_expr(el);
                            let tmp = self.tmp();
                            self.iline(&format!("{} {} = {};", elem_ty, tmp, v));
                            self.iline(&format!("_arr_push(&{}, &{});", name, tmp));
                        }
                    }
                    return;
                }

                let val = self.emit_expr(value);
                self.iline(&format!("{} {} = {};", c_ty, name, val));
            }

            Stmt::Assign { target, value } => {
                let val = self.emit_expr(value);
                match target {
                    AssignTarget::Ident(n)       => self.iline(&format!("{} = {};", n, val)),
                    AssignTarget::Index(n, idx)  => {
                        let i = self.emit_expr(idx);
                        // Check if it's a VArray
                        self.iline(&format!("((int64_t*){}.data)[{}] = {};", n, i, val));
                    }
                    AssignTarget::Field(obj, fld) => {
                        let o = self.emit_expr(obj);
                        self.iline(&format!("{}.{} = {};", o, fld, val));
                    }
                }
            }

            Stmt::Return(None)       => self.iline("return;"),
            Stmt::Return(Some(expr)) => { let v = self.emit_expr(expr); self.iline(&format!("return {};", v)); }
            Stmt::Break              => self.iline("break;"),
            Stmt::Continue           => self.iline("continue;"),

            Stmt::If { cond, then_body, else_ifs, else_body } => {
                let c = self.emit_cond(cond);
                self.iline(&format!("if ({}) {{", c));
                self.indent += 1; for s in then_body { self.emit_stmt(s); } self.indent -= 1;
                for (ei_cond, ei_body) in else_ifs {
                    let ec = self.emit_cond(ei_cond);
                    self.iline(&format!("}} else if ({}) {{", ec));
                    self.indent += 1; for s in ei_body { self.emit_stmt(s); } self.indent -= 1;
                }
                if let Some(eb) = else_body {
                    self.iline("} else {");
                    self.indent += 1; for s in eb { self.emit_stmt(s); } self.indent -= 1;
                }
                self.iline("}");
            }

            Stmt::While { cond, body } => {
                let c = self.emit_cond(cond);
                self.iline(&format!("while ({}) {{", c));
                self.indent += 1; for s in body { self.emit_stmt(s); } self.indent -= 1;
                self.iline("}");
            }

            // for x in range (0..n or 0..=n)
            Stmt::For { var, iter, body } => {
                match iter {
                    Expr::Range { start, end, inclusive } => {
                        let s = self.emit_expr(start);
                        let e = self.emit_expr(end);
                        let op = if *inclusive { "<=" } else { "<" };
                        self.var_types.insert(var.clone(), "i64".into());
                        self.iline(&format!("for (int64_t {v}={s}; {v}{op}{e}; {v}++) {{", v=var, s=s, op=op, e=e));
                    }
                    Expr::Ident(arr_name) => {
                        // for x in array — iterate VArray elements
                        let tmp = self.tmp();
                        self.iline(&format!("for (int64_t {tmp}=0; {tmp}<{arr}.len; {tmp}++) {{", tmp=tmp, arr=arr_name));
                        self.indent += 1;
                        self.iline(&format!("int64_t {var} = ((int64_t*){arr}.data)[{tmp}];", var=var, arr=arr_name, tmp=tmp));
                        self.indent -= 1;
                        // we'll close after body
                        for s in body { self.emit_stmt(s); }
                        self.iline("}");
                        return;
                    }
                    other => {
                        let it = self.emit_expr(other);
                        self.iline(&format!("for (int64_t {v}=0; {v}<{it}; {v}++) {{", v=var, it=it));
                    }
                }
                self.indent += 1; for s in body { self.emit_stmt(s); } self.indent -= 1;
                self.iline("}");
            }

            // for i, x in array
            Stmt::ForIndex { idx, var, iter, body } => {
                let arr = self.emit_expr(iter);
                self.iline(&format!("for (int64_t {i}=0; {i}<{arr}.len; {i}++) {{", i=idx, arr=arr));
                self.indent += 1;
                self.iline(&format!("int64_t {v} = ((int64_t*){arr}.data)[{i}];", v=var, arr=arr, i=idx));
                for s in body { self.emit_stmt(s); }
                self.indent -= 1;
                self.iline("}");
            }

            Stmt::ExprStmt(expr) => { let e = self.emit_expr(expr); self.iline(&format!("{};", e)); }
            Stmt::FnDef(_) | Stmt::ExternBlock(_) | Stmt::DeviceBlock(_) | Stmt::StructDef(_) => {}
        }
    }

    // ── Expressions ───────────────────────────────────────────────────────────

    fn emit_interpolated(&self, s: &str) -> String {
        let mut parts: Vec<String> = Vec::new();
        let mut current = String::new();
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '{' {
                if !current.is_empty() {
                    parts.push(format!("\"{}\"", escape_str(&current)));
                    current.clear();
                }
                let mut expr_src = String::new();
                for inner in chars.by_ref() {
                    if inner == '}' { break; }
                    expr_src.push(inner);
                }
                let expr_src = expr_src.trim().to_string();
                if expr_src.is_empty() {
                    parts.push("\"\"".to_string());
                } else {
                    // Check if it's a known string variable or str-returning call
                    let is_str = self.var_types.get(&expr_src)
                        .map(|t| t == "str")
                        .unwrap_or(false)
                        || self.fn_types.get(&expr_src)
                        .map(|t| t == "const char*")
                        .unwrap_or(false)
                        || expr_src.starts_with('"');
                    if is_str {
                        parts.push(expr_src);
                    } else {
                        // Check if it looks like a float var
                        let is_float = self.var_types.get(&expr_src)
                            .map(|t| t == "f64" || t == "f32" || t == "float")
                            .unwrap_or(false);
                        if is_float {
                            parts.push(format!("float_to_str({})", expr_src));
                        } else {
                            parts.push(format!("int_to_str({})", expr_src));
                        }
                    }
                }
            } else {
                current.push(c);
            }
        }
        if !current.is_empty() {
            parts.push(format!("\"{}\"", escape_str(&current)));
        }
        if parts.is_empty() { return "\"\"".to_string(); }
        if parts.len() == 1 { return parts.remove(0); }
        let mut result = parts.remove(0);
        for part in parts {
            result = format!("_concat({}, {})", result, part);
        }
        result
    }

    fn emit_cond(&self, expr: &Expr) -> String {
        let s = self.emit_expr(expr);
        // strip one layer of outer parens for cleaner C output
        if s.starts_with('(') && s.ends_with(')') { s[1..s.len()-1].to_string() } else { s }
    }

    fn emit_expr(&self, expr: &Expr) -> String {
        match expr {
            Expr::Nil            => "NULL".into(),
            Expr::Bool(true)     => "true".into(),
            Expr::Bool(false)    => "false".into(),
            Expr::Integer(n)     => format!("{}", n),
            Expr::Float(f)       => {
                if f.fract() == 0.0 { format!("{:.1}", f) } else { format!("{}", f) }
            }
            Expr::StringLit(s)   => {
                if s.contains('{') && s.contains('}') {
                    self.emit_interpolated(s)
                } else {
                    format!("\"{}\"", escape_str(s))
                }
            }
            Expr::Ident(name)    => Self::safe_name(name),

            Expr::Cast { expr, ty } => {
                let e = self.emit_expr(expr);
                let ct = match ty.as_str() {
                    "i8"=>"int8_t","i16"=>"int16_t","i32"=>"int32_t","i64"|"int"=>"int64_t",
                    "u8"=>"uint8_t","u16"=>"uint16_t","u32"=>"uint32_t","u64"=>"uint64_t",
                    "f32"=>"float","f64"|"float"=>"double",
                    "bool"=>"bool","str"=>"const char*","ptr"=>"void*",
                    other => other,
                };
                format!("({})({})", ct, e)
            }

            Expr::Range { start, end, inclusive } => {
                // Ranges are only meaningful in for loops; bare range → error at runtime
                let s = self.emit_expr(start);
                let e = self.emit_expr(end);
                let op = if *inclusive { "<=" } else { "<" };
                format!("/* range {s}..{op}{e} */0")
            }

            Expr::Array(_) => "/* array */NULL".into(), // handled in emit_stmt

            Expr::StructLit { name, fields } => {
                let fs: Vec<String> = fields.iter()
                    .map(|(fn_, fv)| format!(".{} = {}", fn_, self.emit_expr(fv)))
                    .collect();
                format!("({}){{ {} }}", name, fs.join(", "))
            }

            Expr::BinOp { op, left, right } => {
                let l = self.emit_expr(left);
                let r = self.emit_expr(right);
                // String equality must use strcmp, not ==
                let l_is_str = matches!(self.infer_type(left).as_str(), "const char*");
                let r_is_str = matches!(self.infer_type(right).as_str(), "const char*");
                match op {
                    BinOp::Concat => format!("_concat({}, {})", l, r),
                    BinOp::And    => format!("({} && {})", l, r),
                    BinOp::Or     => format!("({} || {})", l, r),
                    BinOp::BitAnd => format!("({} & {})", l, r),
                    BinOp::BitOr  => format!("({} | {})", l, r),
                    BinOp::BitXor => format!("({} ^ {})", l, r),
                    BinOp::ShiftL => format!("({} << {})", l, r),
                    BinOp::ShiftR => format!("({} >> {})", l, r),
                    BinOp::Eq if l_is_str || r_is_str =>
                        format!("(strcmp({}, {}) == 0)", l, r),
                    BinOp::NotEq if l_is_str || r_is_str =>
                        format!("(strcmp({}, {}) != 0)", l, r),
                    _             => format!("({} {} {})", l, binop_sym(op), r),
                }
            }

            Expr::UnaryOp { op, expr } => {
                let e = self.emit_expr(expr);
                match op {
                    UnaryOp::Neg    => format!("(-{})", e),
                    UnaryOp::Not    => format!("(!{})", e),
                    UnaryOp::BitNot => format!("(~{})", e),
                }
            }

            Expr::Call { name, args } => {
                // Built-in print with auto-coerce
                if name == "print" && args.len() == 1 {
                    return format!("print({})", self.coerce_str(&args[0]));
                }
                if name == "str" && args.len() == 1 {
                    return self.coerce_str(&args[0]);
                }
                if name == "len" && args.len() == 1 {
                    let a = self.emit_expr(&args[0]);
                    return format!("arr_len({})", a);
                }
                if name == "push" && args.len() == 2 {
                    let arr = self.emit_expr(&args[0]);
                    let val = self.emit_expr(&args[1]);
                    let tmp = format!("_ptmp_{}", arr.replace('.', "_"));
                    return format!("({{ int64_t {tmp}={val}; _arr_push(&{arr},&{tmp}); }})");
                }
                let a: Vec<String> = args.iter().map(|a| self.emit_expr(a)).collect();
                format!("{}({})", Self::safe_name(name), a.join(", "))
            }

            Expr::MethodCall { target, method, args } => {
                let t = self.emit_expr(target);
                let a: Vec<String> = args.iter().map(|a| self.emit_expr(a)).collect();
                format!("{}.{}({})", t, method, a.join(", "))
            }

            Expr::Field { target, field }      => format!("{}.{}", self.emit_expr(target), field),
            Expr::Index { target, index }      => {
                let t = self.emit_expr(target);
                let i = self.emit_expr(index);
                // VArray access
                format!("((int64_t*){}.data)[{}]", t, i)
            }
        }
    }

    fn coerce_str(&self, expr: &Expr) -> String {
        match expr {
            Expr::StringLit(_)  => self.emit_expr(expr),
            Expr::Bool(_)       => format!("bool_to_str({})", self.emit_expr(expr)),
            Expr::Float(_)      => format!("float_to_str({})", self.emit_expr(expr)),
            Expr::Integer(_)    => format!("int_to_str({})", self.emit_expr(expr)),
            Expr::Cast { ty, .. } if ty == "f64" || ty == "f32" || ty == "float"
                                => format!("float_to_str({})", self.emit_expr(expr)),
            Expr::Ident(name) => {
                // Look up var type from declared type or fn return type
                let ty = self.var_types.get(name).map(|s| s.as_str()).unwrap_or("");
                match ty {
                    "f64"|"f32"|"float" => format!("float_to_str({})", name),
                    "bool"              => format!("bool_to_str({})", name),
                    "str"               => name.clone(),
                    // unknown or numeric — safe to int_to_str
                    _                   => format!("int_to_str({})", name),
                }
            }
            Expr::BinOp { op: BinOp::Concat, .. } => self.emit_expr(expr), // already a string
            Expr::Call { name, .. } => {
                // Already a str-returning builtin — don't double-wrap
                if matches!(name.as_str(), "int_to_str"|"float_to_str"|"bool_to_str"|"str_len") {
                    return self.emit_expr(expr);
                }
                let ret = self.fn_types.get(name.as_str()).map(|s| s.as_str()).unwrap_or("");
                match ret {
                    "double"|"float" => format!("float_to_str({})", self.emit_expr(expr)),
                    "bool"           => format!("bool_to_str({})", self.emit_expr(expr)),
                    "const char*"    => self.emit_expr(expr),
                    _                => format!("int_to_str({})", self.emit_expr(expr)),
                }
            }
            _ => {
                // For compound expressions, emit as-is and wrap in int_to_str
                // The user can always call str() explicitly
                let e = self.emit_expr(expr);
                format!("int_to_str({})", e)
            }
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn line(&mut self, s: &str)  { self.out.push_str(s); self.out.push('\n'); }
    fn iline(&mut self, s: &str) {
        let pad = "    ".repeat(self.indent);
        self.out.push_str(&pad); self.out.push_str(s); self.out.push('\n');
    }

    fn emit_params(&self, params: &[Param]) -> String {
        if params.is_empty() { return "void".into(); }
        params.iter().map(|p| {
            let ty = self.resolve_ty(p.ty.as_deref());
            let ty = if ty == "void" { "int64_t".to_string() } else { ty };
            format!("{} {}", ty, p.name)
        }).collect::<Vec<_>>().join(", ")
    }

    // C stdlib names that conflict if redefined by user
    fn safe_name(name: &str) -> String {
        const CONFLICTS: &[&str] = &[
            "abs","max","min","pow","log","sqrt","ceil","floor","round",
            "sin","cos","tan","exp","atoi","atof","rand","time","exit",
        ];
        if CONFLICTS.contains(&name) { format!("volta_{}", name) } else { name.to_string() }
    }
}

// ── Pure helpers ──────────────────────────────────────────────────────────────

fn binop_sym(op: &BinOp) -> &'static str {
    match op {
        BinOp::Add=>"+" ,BinOp::Sub=>"-" ,BinOp::Mul=>"*",
        BinOp::Div=>"/" ,BinOp::Mod=>"%" ,
        BinOp::Eq=>"==",BinOp::NotEq=>"!=",
        BinOp::Lt=>"<" ,BinOp::LtEq=>"<=",
        BinOp::Gt=>">" ,BinOp::GtEq=>">=",
        _ => "??",
    }
}

fn size_of_ty(ty: &str) -> u64 {
    match ty { "u8"|"i8"=>1,"u16"|"i16"=>2,"u32"|"i32"|"f32"=>4,_=>8 }
}

fn escape_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\t', "\\t")
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn compile(src: &str) -> String {
        let tokens = Lexer::new(src).tokenize().unwrap();
        let prog   = Parser::new(tokens).parse_program().unwrap();
        Emitter::new().emit_program(&prog)
    }

    #[test] fn emits_let()           { assert!(compile("let x = 42").contains("int64_t x = 42;")); }
    #[test] fn emits_let_typed()     { assert!(compile("let x: i32 = 42").contains("int32_t x = 42;")); }
    #[test] fn emits_fn()            { let o=compile("fn add(a: i64, b: i64) -> i64\n  return a + b\nend"); assert!(o.contains("int64_t add(")); }
    #[test] fn emits_struct()        { let o=compile("struct Point\n  x: i64\n  y: i64\nend"); assert!(o.contains("typedef struct Point")); }
    #[test] fn emits_string_concat() { assert!(compile(r#"let s = "hello" .. " world""#).contains("_concat(")); }
    #[test] fn emits_if()            { assert!(compile("if x == 1 do\n  let y = 2\nend").contains("if (x == 1)")); }
    #[test] fn emits_break_continue(){ let o=compile("while true do\n  break\n  continue\nend"); assert!(o.contains("break;")); assert!(o.contains("continue;")); }
    #[test] fn emits_range_for()     { let o=compile("for i in 0..10 do\n  print(int_to_str(i))\nend"); assert!(o.contains("for (int64_t i=") && o.contains("i++"), "got: {}", &o[o.find("for").unwrap_or(0)..o.find("for").unwrap_or(0)+80]); }
    #[test] fn emits_cast()          { assert!(compile("let x = 42 as f64").contains("(double)(42)")); }
    #[test] fn emits_bitops()        { assert!(compile("let x = 0xFF & 0x0F").contains("& ")); }
    #[test] fn full_program_compiles(){ let o=compile("fn greet(who: str) -> str\n  return \"Hello, \" .. who\nend\nlet msg = greet(\"Volta\")"); assert!(o.contains("int main(")); }
}
