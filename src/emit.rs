// volta/src/emit.rs

use crate::ast::*;
use std::collections::{HashMap, HashSet};

// ── Error type ────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct EmitError {
    pub msg: String,
}

impl EmitError {
    fn new(msg: impl Into<String>) -> Self { EmitError { msg: msg.into() } }
}

impl std::fmt::Display for EmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.msg)
    }
}

impl std::error::Error for EmitError {}

pub struct Emitter {
    out:             String,
    indent:          usize,
    fn_types:        HashMap<String, String>,
    var_types:       HashMap<String, String>,
    struct_defs:     HashMap<String, Vec<(String, String)>>,
    struct_names:    HashSet<String>,
    enum_defs:       HashMap<String, Vec<String>>,
    aliases:         HashMap<String, String>, // type alias name → target type string
    tmp_counter:     usize,
    in_result_fn:    bool,
    // ── Closure hoisting ────────────────────────────────────────────
    closure_buf:     String,  // pending closure C functions (emitted before forward decls)
    closure_counter: usize,   // sequential id for generated closures
    // ── Defer transform state ────────────────────────────────────────
    current_defers:  Vec<Expr>, // deferred exprs for the current function (LIFO)
    has_defer:       bool,      // true when inside a function that has defer statements
    defer_label_id:  usize,     // unique counter for _vdefer_N goto labels
}

impl Emitter {
    pub fn new() -> Self {
        Emitter {
            out: String::new(), indent: 0,
            fn_types: HashMap::new(), var_types: HashMap::new(),
            struct_defs: HashMap::new(), struct_names: HashSet::new(),
            enum_defs: HashMap::new(),
            aliases: HashMap::new(),
            tmp_counter: 0,
            in_result_fn: false,
            closure_buf: String::new(),
            closure_counter: 0,
            current_defers: Vec::new(),
            has_defer: false,
            defer_label_id: 0,
        }
    }

    fn tmp(&mut self) -> String {
        self.tmp_counter += 1;
        format!("_vt{}", self.tmp_counter)
    }

    pub fn emit_program(mut self, prog: &Program) -> Result<String, EmitError> {
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
        // Register runtime struct types so resolve_ty recognises them
        self.struct_names.insert("VArena".into());
        self.struct_names.insert("VClosure".into());
        // Register memory / arena builtins so infer_type works on their calls
        self.fn_types.insert("alloc".into(),           "void*".into());
        self.fn_types.insert("free".into(),            "void".into());
        self.fn_types.insert("arena_new".into(),       "VArena".into());
        self.fn_types.insert("arena_alloc".into(),     "void*".into());
        self.fn_types.insert("arena_reset".into(),     "void".into());
        self.fn_types.insert("arena_free_all".into(),  "void".into());

        for stmt in &prog.stmts {
            // Collect type aliases first so resolve_ty can use them
            if let Stmt::TypeAlias { name, target, .. } = stmt {
                self.aliases.insert(name.clone(), target.clone());
                // Register as a known name so it's not treated as unknown
                self.struct_names.insert(name.clone());
            }
        }
        for stmt in &prog.stmts {
            match stmt {
                Stmt::StructDef(s) => {
                    self.struct_defs.insert(s.name.clone(), s.fields.clone());
                    self.struct_names.insert(s.name.clone());
                }
                Stmt::PackedStructDef(ps) => {
                    let fields: Vec<(String, String)> = ps.fields.iter()
                        .map(|(n, _)| (n.clone(), ps.base_ty.clone()))
                        .collect();
                    self.struct_defs.insert(ps.name.clone(), fields);
                    self.struct_names.insert(ps.name.clone());
                }
                Stmt::EnumDef(e) => {
                    self.enum_defs.insert(e.name.clone(), e.variants.clone());
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

        // ── Closure pre-scan: must happen BEFORE any emit_expr calls so that
        // closure counter IDs assigned here match those used during emission.
        self.closure_counter = 0;
        self.prescan_for_closures(prog);
        if !self.closure_buf.is_empty() {
            let cbuf = std::mem::take(&mut self.closure_buf);
            self.out.push_str(&cbuf);
            self.line("");
        }
        self.closure_counter = 0; // reset — main passes will re-number in same order

        // Constants (static const)
        let mut had_const = false;
        for stmt in &prog.stmts {
            if let Stmt::Const { name, ty, value, .. } = stmt {
                let c_ty = if let Some(t) = ty { self.resolve_ty(Some(t)) }
                           else { self.infer_type(value) };
                let val = self.emit_expr(value);
                self.line(&format!("static const {} {} = {};", c_ty, name, val));
                // Register so other code can reference it
                if let Some(t) = ty {
                    self.var_types.insert(name.clone(), t.clone());
                }
                had_const = true;
            }
        }
        if had_const { self.line(""); }

        // Type alias typedefs
        let mut had_alias = false;
        for stmt in &prog.stmts {
            if let Stmt::TypeAlias { name, target, .. } = stmt {
                let c_ty = self.resolve_ty(Some(target));
                self.line(&format!("typedef {} {};", c_ty, name));
                had_alias = true;
            }
        }
        if had_alias { self.line(""); }

        // Struct definitions
        for stmt in &prog.stmts {
            if let Stmt::StructDef(s) = stmt { self.emit_struct_def(s); }
        }

        // Packed struct definitions
        for stmt in &prog.stmts {
            if let Stmt::PackedStructDef(ps) = stmt { self.emit_packed_struct_def(ps); }
        }

        // Enum definitions
        for stmt in &prog.stmts {
            if let Stmt::EnumDef(e) = stmt { self.emit_enum_def(e); }
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
            !matches!(s, Stmt::FnDef(_)|Stmt::ExternBlock(_)|Stmt::DeviceBlock(_)
                       |Stmt::StructDef(_)|Stmt::PackedStructDef(_)|Stmt::EnumDef(_)
                       |Stmt::Const { .. }|Stmt::TypeAlias { .. })
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

        Ok(self.out)
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
        self.line(r#"static const char* float_to_str(double n){char*d=_vbuf+_vpos;int k;if(n==(double)(int64_t)n)k=snprintf(d,32,"%.1f",n);else k=snprintf(d,32,"%g",n);_vpos=(_vpos+k+1)%131072;return d;}"#);
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
        self.line(r#"typedef struct{void**data;int64_t len;int64_t cap;}VArray;"#);
        self.line(r#"static VArray _arr_new(int64_t cap){VArray a;cap=cap<8?8:cap;a.data=(void**)malloc(cap*sizeof(void*));a.len=0;a.cap=cap;return a;}"#);
        self.line(r#"static void _arr_push(VArray*a,void*v){if(a->len>=a->cap){a->cap*=2;a->data=(void**)realloc(a->data,a->cap*sizeof(void*));}a->data[a->len++]=v;}"#);
        self.line(r#"static void* _arr_pop(VArray*a){return a->len>0?a->data[--a->len]:NULL;}"#);
        self.line(r#"static void* _arr_get(VArray*a,int64_t i){return(i>=0&&i<a->len)?a->data[i]:NULL;}"#);
        self.line(r#"static void _arr_set(VArray*a,int64_t i,void*v){if(i>=0&&i<a->len)a->data[i]=v;}"#);
        self.line(r#"static int64_t arr_len(VArray a){return a.len;}"#);
        self.line(r#"#define AGET_INT(a,i) ((int64_t)(intptr_t)_arr_get(&(a),(i)))"#);
        self.line(r#"#define AGET_STR(a,i) ((const char*)_arr_get(&(a),(i)))"#);
        self.line(r#"#define ASET_INT(a,i,v) _arr_set(&(a),(i),(void*)(intptr_t)(v))"#);
        self.line(r#"#define ASET_STR(a,i,v) _arr_set(&(a),(i),(void*)(v))"#);
        self.line(r#"#define APUSH_INT(a,v) _arr_push(&(a),(void*)(intptr_t)(v))"#);
        self.line(r#"#define APUSH_STR(a,v) _arr_push(&(a),(void*)(v))"#);
        // Float arrays: store double bits as int64 inside void*
        self.line(r#"static double _aget_flt(VArray*a,int64_t i){int64_t _bi=(int64_t)(intptr_t)_arr_get(a,i);double _bf;memcpy(&_bf,&_bi,8);return _bf;}"#);
        self.line(r#"static void _aset_flt(VArray*a,int64_t i,double v){int64_t _bi;memcpy(&_bi,&v,8);_arr_set(a,i,(void*)(intptr_t)_bi);}"#);
        self.line(r#"static void _apush_flt(VArray*a,double v){int64_t _bi;memcpy(&_bi,&v,8);_arr_push(a,(void*)(intptr_t)_bi);}"#);
        self.line(r#"#define AGET_FLT(a,i) _aget_flt(&(a),(i))"#);
        self.line(r#"#define ASET_FLT(a,i,v) _aset_flt(&(a),(i),(v))"#);
        self.line(r#"#define APUSH_FLT(a,v) _apush_flt(&(a),(v))"#);
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
        // ── File I/O ──────────────────────────────────────────────
        self.line(r#"static const char* file_read(const char* path){FILE*f=fopen(path,"r");if(!f)return "";fseek(f,0,SEEK_END);long sz=ftell(f);rewind(f);char*buf=(char*)malloc(sz+1);if(!buf){fclose(f);return "";}fread(buf,1,sz,f);buf[sz]=0;fclose(f);char*d=_vbuf+_vpos;if(sz<65000){memcpy(d,buf,sz+1);_vpos=(_vpos+sz+1)%131072;}free(buf);return d;}"#);
        self.line(r#"static bool file_write(const char* path,const char* data){FILE*f=fopen(path,"w");if(!f)return false;fputs(data,f);fclose(f);return true;}"#);
        self.line(r#"static bool file_append(const char* path,const char* data){FILE*f=fopen(path,"a");if(!f)return false;fputs(data,f);fclose(f);return true;}"#);
        self.line(r#"static bool file_exists(const char* path){FILE*f=fopen(path,"r");if(!f)return false;fclose(f);return true;}"#);
        self.line(r#"static bool file_delete(const char* path){return remove(path)==0;}"#);
        self.line(r#"static int64_t file_size(const char* path){FILE*f=fopen(path,"r");if(!f)return -1;fseek(f,0,SEEK_END);long sz=ftell(f);fclose(f);return (int64_t)sz;}"#);
        self.line(r#"static const char* file_readline(const char* path,int64_t n){FILE*f=fopen(path,"r");if(!f)return "";char line[4096];int64_t i=0;while(i<=n&&fgets(line,sizeof(line),f)){if(i==n){fclose(f);char*d=_vbuf+_vpos;int len=strlen(line);if(len>0&&line[len-1]=='\n')line[--len]=0;memcpy(d,line,len+1);_vpos=(_vpos+len+1)%131072;return d;}i++;}fclose(f);return "";}"#);
        self.line(r#"#include <sys/socket.h>"#);
        self.line(r#"#include <netinet/in.h>"#);
        self.line(r#"#include <arpa/inet.h>"#);
        self.line(r#"#include <netdb.h>"#);
        self.line(r#"#include <unistd.h>"#);
        self.line(r#"static int64_t tcp_connect(const char* host,int64_t port){struct addrinfo hints={0},*res;hints.ai_family=AF_UNSPEC;hints.ai_socktype=SOCK_STREAM;char ps[16];snprintf(ps,16,"%lld",(long long)port);if(getaddrinfo(host,ps,&hints,&res)!=0)return -1;int fd=socket(res->ai_family,res->ai_socktype,res->ai_protocol);if(fd<0){freeaddrinfo(res);return -1;}if(connect(fd,res->ai_addr,res->ai_addrlen)!=0){close(fd);freeaddrinfo(res);return -1;}freeaddrinfo(res);return (int64_t)fd;}"#);
        self.line(r#"static int64_t tcp_listen(int64_t port){int fd=socket(AF_INET,SOCK_STREAM,0);if(fd<0)return -1;int opt=1;setsockopt(fd,SOL_SOCKET,SO_REUSEADDR,&opt,sizeof(opt));struct sockaddr_in addr={0};addr.sin_family=AF_INET;addr.sin_addr.s_addr=INADDR_ANY;addr.sin_port=htons((uint16_t)port);if(bind(fd,(struct sockaddr*)&addr,sizeof(addr))<0){close(fd);return -1;}if(listen(fd,10)<0){close(fd);return -1;}return (int64_t)fd;}"#);
        self.line(r#"static int64_t tcp_accept(int64_t sfd){struct sockaddr_in a={0};socklen_t l=sizeof(a);return (int64_t)accept((int)sfd,(struct sockaddr*)&a,&l);}"#);
        self.line(r#"static bool tcp_send(int64_t fd,const char* data){size_t len=strlen(data);return send((int)fd,data,len,0)==(ssize_t)len;}"#);
        self.line(r#"static const char* tcp_recv(int64_t fd){char*d=_vbuf+_vpos;int64_t total=0;char tmp[4096];ssize_t n;while((n=recv((int)fd,tmp,sizeof(tmp)-1,0))>0){if(total+n>=65000)break;memcpy(d+total,tmp,n);total+=n;}d[total]=0;_vpos=(_vpos+total+1)%131072;return d;}"#);
        self.line(r#"static const char* tcp_recv_line(int64_t fd){char*d=_vbuf+_vpos;int64_t i=0;char c;while(recv((int)fd,&c,1,0)==1&&i<4094){if(c=='\n')break;if(c!='\r')d[i++]=c;}d[i]=0;_vpos=(_vpos+i+1)%131072;return d;}"#);
        self.line(r#"static void tcp_close(int64_t fd){close((int)fd);}"#);
        self.line(r#"static bool tcp_ok(int64_t fd){return fd>=0;}"#);
        self.line(r#"static const char* tcp_peer_ip(int64_t fd){struct sockaddr_in a={0};socklen_t l=sizeof(a);getpeername((int)fd,(struct sockaddr*)&a,&l);char*d=_vbuf+_vpos;inet_ntop(AF_INET,&a.sin_addr,d,64);_vpos=(_vpos+64)%131072;return d;}"#);
        // ── PostgreSQL via libpq ─────────────────────────────────────
        self.line(r#"#include <libpq-fe.h>"#);
        self.line(r#"static PGconn* _pg_conn=NULL;"#);
        self.line(r#"static bool pg_connect(const char* connstr){_pg_conn=PQconnectdb(connstr);return PQstatus(_pg_conn)==CONNECTION_OK;}"#);
        self.line(r#"static void pg_close(void){if(_pg_conn){PQfinish(_pg_conn);_pg_conn=NULL;}}"#);
        self.line(r#"static bool pg_ok(void){return _pg_conn&&PQstatus(_pg_conn)==CONNECTION_OK;}"#);
        self.line(r#"static const char* pg_error(void){return _pg_conn?PQerrorMessage(_pg_conn):"no connection";}"#);
        self.line(r#"typedef struct{PGresult*res;int rows;int cols;}VPGResult;"#);
        self.line(r#"static PGresult* _pg_res=NULL;"#);
        self.line(r#"static int64_t pg_query(const char* sql){if(_pg_res){PQclear(_pg_res);_pg_res=NULL;}if(!_pg_conn)return 0;_pg_res=PQexec(_pg_conn,sql);if(!_pg_res)return 0;return (int64_t)PQntuples(_pg_res);}"#);
        self.line(r#"static const char* pg_value(int64_t row,int64_t col){if(!_pg_res)return "";return PQgetvalue(_pg_res,(int)row,(int)col);}"#);
        self.line(r#"static int64_t pg_rows(void){return _pg_res?(int64_t)PQntuples(_pg_res):0;}"#);
        self.line(r#"static void pg_free(void){if(_pg_res){PQclear(_pg_res);_pg_res=NULL;}}"#);
        self.line(r#"static bool pg_exec(const char* sql){if(!_pg_conn)return false;PGresult*r=PQexec(_pg_conn,sql);bool ok=PQresultStatus(r)==PGRES_COMMAND_OK||PQresultStatus(r)==PGRES_TUPLES_OK;PQclear(r);return ok;}"#);
        self.line(r#"static const char* pg_escape(const char* s){static char buf[8192];size_t err;PQescapeStringConn(_pg_conn,buf,s,strlen(s),NULL);(void)err;return buf;}"#);
        // ── Arena allocator ──────────────────────────────────────────
        self.line(r#"typedef struct{char*buf;int64_t cap;int64_t pos;}VArena;"#);
        self.line(r#"static VArena arena_new(int64_t cap){VArena a;a.buf=(char*)malloc((size_t)cap);a.cap=cap;a.pos=0;return a;}"#);
        self.line(r#"static void* arena_alloc(VArena*a,int64_t size){if(a->pos+size>a->cap)return NULL;void*p=a->buf+a->pos;a->pos+=size;return p;}"#);
        self.line(r#"static void arena_reset(VArena*a){a->pos=0;}"#);
        self.line(r#"static void arena_free_all(VArena*a){free(a->buf);a->buf=NULL;a->cap=0;a->pos=0;}"#);
        // ── Closure fat pointer (capturing closures) ─────────────────
        self.line(r#"typedef struct{void*env;void*fn;}VClosure;"#);
        // ── Result type ──────────────────────────────────────────────
        self.line(r#"typedef struct{bool ok;int64_t ival;const char* sval;double fval;const char* err;}VResult;"#);
        self.line(r#"static VResult _volt_ok_i(int64_t v){VResult r;r.ok=true;r.ival=v;r.sval=NULL;r.fval=0;r.err=NULL;return r;}"#);
        self.line(r#"static VResult _volt_ok_s(const char* v){VResult r;r.ok=true;r.ival=0;r.sval=v;r.fval=0;r.err=NULL;return r;}"#);
        self.line(r#"static VResult _volt_ok_f(double v){VResult r;r.ok=true;r.ival=0;r.sval=NULL;r.fval=v;r.err=NULL;return r;}"#);
        self.line(r#"static VResult _volt_err(const char* e){VResult r;r.ok=false;r.ival=0;r.sval=NULL;r.fval=0;r.err=e;return r;}"#);
        self.line("// ───────────────────────────────────────────────────────────");
        self.line("");
    }

    // ── Type resolution ───────────────────────────────────────────────────────

    fn resolve_ty(&self, ty: Option<&str>) -> String {
        // Resolve type aliases first
        if let Some(name) = ty {
            if let Some(target) = self.aliases.get(name) {
                return self.resolve_ty(Some(target));
            }
        }
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
            Some("Result")            => "VResult".into(),
            Some(fp) if fp.starts_with("fn(") => {
                // Function pointer used in a generic type position (e.g. struct field)
                // Emit as void* — proper syntax handled in Let/params
                "void*".into()
            }
            Some(ptr) if ptr.starts_with('*') => {
                // Pointer type: *i64 → int64_t*, **u8 → uint8_t**, *str → const char**
                let inner = &ptr[1..];
                let base  = self.resolve_ty(Some(inner));
                format!("{}*", base)
            }
            Some(arr) if arr.starts_with('[') && arr.contains(';') => {
                // Fixed-size array [i64;8] — decays to pointer when used as param type
                let inner = &arr[1..arr.len()-1];
                let elem  = inner.split(';').next().unwrap_or("i64").trim();
                format!("{}*", self.resolve_ty(Some(elem)))
            }
            Some(arr) if arr.starts_with('[') => "VArray".into(),
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
                    Some(t) if t.starts_with('[') => "VArray".into(),
                    Some(other) if self.struct_names.contains(other) => other.to_string(),
                    _                         => "int64_t".into(), // default numeric
                }
            }
            Expr::Call { name, .. }               => {
                // Check known str-returning builtins first
                const STR_CALLS: &[&str] = &[
                    "int_to_str","float_to_str","bool_to_str","str_upper","str_lower",
                    "str_reverse","str_repeat","str_pad_left","str_pad_right","str_slice",
                    "str_replace","char_from","rot13","caesar","xor_str","hex",
                    "bytes_to_hex","arg_get","input","xor_encrypt","str_to_hex_str",
                    "greet","repeat_str","file_read","file_readline",
                ];
                if STR_CALLS.contains(&name.as_str()) {
                    return "const char*".into();
                }
                self.fn_types.get(name).cloned()
                    .unwrap_or_else(|| "int64_t".into())
            }
            Expr::StructLit { name, .. }          => name.clone(),
            Expr::Array(_)                        => "VArray".into(),
            Expr::Field { .. }                    => "int64_t".into(),
            Expr::Index { .. }                    => "int64_t".into(),
            Expr::UnaryOp { op: UnaryOp::Ref, expr } => {
                // &x → pointer to x's type
                let inner = self.infer_type(expr);
                format!("{}*", inner)
            }
            Expr::UnaryOp { op: UnaryOp::Deref, expr } => {
                // *p → element type (strip one pointer level from p's type)
                let ptr_ty = self.infer_type(expr);
                if ptr_ty.ends_with('*') {
                    ptr_ty[..ptr_ty.len()-1].trim().to_string()
                } else {
                    "int64_t".into()
                }
            }
            _ => "int64_t".into(), // safe default for unknown
        }
    }

    // ── Closure pre-scan ──────────────────────────────────────────────────────

    /// Walk the whole program looking for Expr::Closure nodes (depth-first,
    /// same order as emit_expr), emit each one as a static C function into
    /// self.closure_buf, and assign it a name _vclosure_N.
    ///
    /// IMPORTANT: must visit closures in the SAME ORDER as emit_program so
    /// that the closure_counter IDs match up.
    /// emit_program order: constants → fn bodies → top-level stmts (main).
    fn prescan_for_closures(&mut self, prog: &Program) {
        // 1. Constant initialisers (same order as emit_program's const loop)
        for stmt in &prog.stmts {
            if let Stmt::Const { value, .. } = stmt {
                self.prescan_expr(value);
            }
        }
        // 2. Function bodies
        for stmt in &prog.stmts {
            if let Stmt::FnDef(f) = stmt {
                for s in &f.body { self.prescan_stmt(s); }
            }
        }
        // 3. Top-level (main) stmts — everything that isn't a fn/type def
        for stmt in &prog.stmts {
            match stmt {
                Stmt::FnDef(_) | Stmt::ExternBlock(_) | Stmt::DeviceBlock(_)
                | Stmt::StructDef(_) | Stmt::PackedStructDef(_) | Stmt::EnumDef(_)
                | Stmt::Const { .. } | Stmt::TypeAlias { .. } => {}
                _ => self.prescan_stmt(stmt),
            }
        }
    }

    fn prescan_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::FnDef(f) => {
                for s in &f.body { self.prescan_stmt(s); }
            }
            Stmt::Let { value, .. } => self.prescan_expr(value),
            Stmt::Const { value, .. } => self.prescan_expr(value),
            Stmt::Assign { value, .. } => self.prescan_expr(value),
            Stmt::Return(Some(e)) => self.prescan_expr(e),
            Stmt::ExprStmt(e) => self.prescan_expr(e),
            Stmt::Defer { expr, .. } => self.prescan_expr(expr),
            Stmt::If { cond, then_body, else_ifs, else_body, .. } => {
                self.prescan_expr(cond);
                for s in then_body { self.prescan_stmt(s); }
                for (ec, eb) in else_ifs { self.prescan_expr(ec); for s in eb { self.prescan_stmt(s); } }
                if let Some(eb) = else_body { for s in eb { self.prescan_stmt(s); } }
            }
            Stmt::While { cond, body, .. } => {
                self.prescan_expr(cond);
                for s in body { self.prescan_stmt(s); }
            }
            Stmt::For { iter, body, .. } | Stmt::ForIndex { iter, body, .. } => {
                self.prescan_expr(iter);
                for s in body { self.prescan_stmt(s); }
            }
            Stmt::Match { expr, arms, .. } => {
                self.prescan_expr(expr);
                for arm in arms { for s in &arm.body { self.prescan_stmt(s); } }
            }
            _ => {}
        }
    }

    fn prescan_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Closure { params, ret_ty, body } => {
                self.closure_counter += 1;
                let name = format!("_vclosure_{}", self.closure_counter);
                // Emit this closure's C function into closure_buf
                let p = params.clone();
                let r = ret_ty.clone();
                let b = body.clone();
                self.emit_closure_to_buf(&p, r.as_deref(), &b, &name);
                // Do NOT recurse into body — nested closures unsupported in v0.5
            }
            Expr::BinOp { left, right, .. } => {
                self.prescan_expr(left); self.prescan_expr(right);
            }
            Expr::UnaryOp { expr, .. } => self.prescan_expr(expr),
            Expr::Call { args, .. } => { for a in args { self.prescan_expr(a); } }
            Expr::MethodCall { target, args, .. } => {
                self.prescan_expr(target);
                for a in args { self.prescan_expr(a); }
            }
            Expr::Field { target, .. } => self.prescan_expr(target),
            Expr::Index { target, index } => {
                self.prescan_expr(target); self.prescan_expr(index);
            }
            Expr::StructLit { fields, .. } => {
                for (_, v) in fields { self.prescan_expr(v); }
            }
            Expr::Array(elems) => { for e in elems { self.prescan_expr(e); } }
            Expr::Cast { expr, .. } => self.prescan_expr(expr),
            Expr::Try(e) => self.prescan_expr(e),
            Expr::Range { start, end, .. } => {
                self.prescan_expr(start); self.prescan_expr(end);
            }
            _ => {}
        }
    }

    /// Emit a closure as a static C function into self.closure_buf using the
    /// buffer-swap trick (temporarily redirect self.out).
    fn emit_closure_to_buf(&mut self, params: &[Param], ret_ty: Option<&str>, body: &[Stmt], name: &str) {
        // Redirect output so closure code lands in closure_buf, not self.out
        let saved_out    = std::mem::take(&mut self.out);
        let saved_indent = self.indent;
        let saved_vtypes = self.var_types.clone();
        let saved_result = self.in_result_fn;

        for p in params {
            if let Some(ty) = &p.ty {
                self.var_types.insert(p.name.clone(), ty.clone());
            }
        }
        self.in_result_fn = ret_ty == Some("Result");

        let ret        = self.resolve_ty(ret_ty);
        let params_str = self.emit_params(params);
        self.indent = 0;
        self.line(&format!("static {} {}({}) {{", ret, name, params_str));
        self.indent = 1;
        // Re-use the same defer machinery — closures support defer too.
        self.emit_body_with_defers(body, &ret.clone());
        self.indent = 0;
        self.line("}");

        // Harvest generated code and restore state
        let closure_code  = std::mem::take(&mut self.out);
        self.out          = saved_out;
        self.indent       = saved_indent;
        self.var_types    = saved_vtypes;
        self.in_result_fn = saved_result;
        self.closure_buf.push_str(&closure_code);
        self.closure_buf.push('\n');
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

    // ── Packed struct ─────────────────────────────────────────────────────────

    fn emit_packed_struct_def(&mut self, ps: &PackedStructDef) {
        let base_cty = match ps.base_ty.as_str() {
            "u8"  => "uint8_t",
            "u16" => "uint16_t",
            "u32" => "uint32_t",
            "u64" => "uint64_t",
            "i8"  => "int8_t",
            "i16" => "int16_t",
            "i32" => "int32_t",
            "i64" => "int64_t",
            other => other,
        };
        self.line(&format!("typedef struct __attribute__((packed)) {{"));
        for (fname, bits) in &ps.fields {
            self.line(&format!("    {} {} : {};", base_cty, fname, bits));
        }
        self.line(&format!("}} {};\n", ps.name));
    }

    // ── Enum ──────────────────────────────────────────────────────────────────

    fn emit_enum_def(&mut self, e: &EnumDef) {
        self.line(&format!("typedef int64_t {};", e.name));
        for (i, variant) in e.variants.iter().enumerate() {
            let macro_name = format!("{}_{}", e.name.to_uppercase(), variant.to_uppercase());
            self.line(&format!("#define {} {}", macro_name, i));
        }
        self.line("");
    }

    // ── Extern ────────────────────────────────────────────────────────────────

    fn emit_extern_block(&mut self, eb: &ExternBlock) {
        const SKIP: &[&str] = &["printf","puts","malloc","free","memcpy","strlen","scanf","fgets","atoi","atof","atoll","sqrt","floor","ceil","fabs","getenv","system","exit","putchar","getchar","fopen","fclose","fread","fwrite","rand","srand","time","abort"];
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
        let ret    = self.resolve_ty(f.ret_ty.as_deref());
        let params = self.emit_params(&f.params);
        let safe   = Self::safe_name(&f.name);
        self.line(&format!("{} {}({}) {{", ret, safe, params));
        self.indent += 1;
        let prev_result_fn = self.in_result_fn;
        self.in_result_fn  = f.ret_ty.as_deref() == Some("Result");
        for p in &f.params {
            if let Some(ty) = &p.ty {
                self.var_types.insert(p.name.clone(), ty.clone());
            }
        }
        self.emit_body_with_defers(&f.body, &ret.clone());
        self.in_result_fn = prev_result_fn;
        self.indent -= 1;
        self.line("}");
    }

    /// Emit a list of statements with defer-transform support.
    ///
    /// If the body contains `Stmt::Defer` nodes (at any nesting depth), the
    /// function:
    ///  1. Emits a zero-initialised `_vdefer_ret` variable for non-void fns.
    ///  2. Transforms every `Stmt::Return` into a store + `goto _vdefer_N`.
    ///  3. Emits the cleanup label after the body, runs deferred exprs LIFO,
    ///     then emits the actual `return _vdefer_ret;` (or just falls off for void).
    ///
    /// When there are no defers the body is emitted as-is — zero overhead.
    fn emit_body_with_defers(&mut self, stmts: &[Stmt], ret_c: &str) {
        let defers    = collect_defers(stmts);
        let has_defer = !defers.is_empty();

        // Save and update defer context
        let prev_defers    = std::mem::take(&mut self.current_defers);
        let prev_has_defer = self.has_defer;

        let label_id = if has_defer {
            self.defer_label_id += 1;
            self.defer_label_id
        } else {
            0
        };

        if has_defer {
            self.has_defer      = true;
            self.current_defers = defers;
            // Allocate a slot to capture the return value
            if ret_c != "void" {
                let init = default_value_for_c_type(ret_c);
                self.iline(&format!("{} _vdefer_ret = {};", ret_c, init));
            }
        }

        for stmt in stmts { self.emit_stmt(stmt); }

        if has_defer {
            // Both early returns (via goto) and natural fallthrough land here.
            self.iline(&format!("_vdefer_{}: ;", label_id));
            // Run deferred expressions in LIFO order (snapshot to avoid borrow issue)
            let defers_snap = self.current_defers.clone();
            for expr in defers_snap.iter().rev() {
                let e = self.emit_expr(expr);
                self.iline(&format!("{};", e));
            }
            if ret_c != "void" {
                self.iline("return _vdefer_ret;");
            }
        }

        // Restore context
        self.current_defers = prev_defers;
        self.has_defer      = prev_has_defer;
    }

    // ── Statements ────────────────────────────────────────────────────────────

    fn emit_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { name, ty, value, .. } => {
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
                            "VArray"      => "VArray",
                            s if s.starts_with("struct ") => s,
                            _ => "i64",
                        };
                        self.var_types.insert(name.clone(), volta_ty.to_string());
                    }
                }

                // Special case: function pointer variable
                if let Some(ty_str) = ty {
                    if ty_str.starts_with("fn(") {
                        if let Some((inner_params, ret)) = parse_fn_ptr_ty(ty_str) {
                            let ret_c = self.resolve_ty(Some(&ret));
                            let pc: Vec<String> = inner_params.iter()
                                .map(|t| self.resolve_ty(Some(t))).collect();
                            let val = self.emit_expr(value);
                            self.iline(&format!("{} (*{})({}) = {};", ret_c, name, pc.join(", "), val));
                            self.var_types.insert(name.clone(), ty_str.clone());
                            // Register return type so calls through this var infer correctly
                            self.fn_types.insert(name.clone(), ret_c);
                            return;
                        }
                    }
                }

                // Special case: let p: *T = alloc(n) → (T*)calloc(n, sizeof(T))
                if let Some(ty_str) = ty {
                    if ty_str.starts_with('*') {
                        if let Expr::Call { name: call_name, args } = value {
                            if call_name == "alloc" && args.len() == 1 {
                                let inner_ty = &ty_str[1..];
                                let inner_c  = self.resolve_ty(Some(inner_ty));
                                let c_ptr    = format!("{}*", inner_c);
                                let n_expr   = self.emit_expr(&args[0]);
                                self.iline(&format!("{} {} = ({})calloc({}, sizeof({}));",
                                    c_ptr, name, c_ptr, n_expr, inner_c));
                                self.var_types.insert(name.clone(), ty_str.clone());
                                return;
                            }
                        }
                    }
                }

                // Special case: fixed-size array [T; N]
                if let Some(ty_str) = ty {
                    if ty_str.starts_with('[') && ty_str.contains(';') {
                        let inner  = &ty_str[1..ty_str.len()-1];
                        let parts: Vec<&str> = inner.splitn(2, ';').collect();
                        let elem   = parts[0].trim();
                        let size   = parts.get(1).unwrap_or(&"0").trim();
                        let elem_c = self.resolve_ty(Some(elem));
                        self.iline(&format!("{} {}[{}] = {{0}};", elem_c, name, size));
                        self.var_types.insert(name.clone(), ty_str.clone());
                        return;
                    }
                }

                // Special case: expr? in let → emit try-unwrap
                if let Expr::Try(inner) = value {
                    let inner_c = self.emit_expr(inner);
                    let field = match c_ty.as_str() {
                        "const char*" => "sval",
                        "double" | "float" => "fval",
                        _ => "ival",
                    };
                    if self.in_result_fn {
                        self.iline(&format!(
                            "{c} {n};{{VResult _vt={e};if(!_vt.ok){{return _vt;}}{n}=_vt.{f};}}",
                            c=c_ty, n=name, e=inner_c, f=field
                        ));
                    } else {
                        self.iline(&format!(
                            "{c} {n};{{VResult _vt={e};if(!_vt.ok){{fprintf(stderr,\"error: %s\\n\",_vt.err);exit(1);}}{n}=_vt.{f};}}",
                            c=c_ty, n=name, e=inner_c, f=field
                        ));
                    }
                    return;
                }

                // Special case: array literal → VArray
                if let Expr::Array(elems) = value {
                    let cap = if elems.is_empty() { 8 } else { elems.len().max(8) };
                    self.iline(&format!("VArray {} = _arr_new({});", name, cap));
                    // Determine push macro from type annotation or element inference
                    let ann_ty = ty.as_deref().unwrap_or("");
                    for el in elems {
                        let v = self.emit_expr(el);
                        let push_macro = match ann_ty {
                            "[str]"                       => "APUSH_STR",
                            "[f64]" | "[f32]" | "[float]" => "APUSH_FLT",
                            t if t.starts_with('[')       => "APUSH_INT",
                            _ => {
                                if self.infer_type(el) == "const char*" { "APUSH_STR" }
                                else { "APUSH_INT" }
                            }
                        };
                        self.iline(&format!("{}({}, {});", push_macro, name, v));
                    }
                    return;
                }

                let val = self.emit_expr(value);
                self.iline(&format!("{} {} = {};", c_ty, name, val));
            }

            Stmt::Assign { target, value, .. } => {
                let val = self.emit_expr(value);
                match target {
                    AssignTarget::Ident(n)       => self.iline(&format!("{} = {};", n, val)),
                    AssignTarget::Index(n, idx) => {
                        let i = self.emit_expr(idx);
                        let set_macro = match self.var_types.get(n.as_str()).map(|s| s.as_str()).unwrap_or("") {
                            "[str]"                       => "ASET_STR",
                            "[f64]" | "[f32]" | "[float]" => "ASET_FLT",
                            _                             => "ASET_INT",
                        };
                        self.iline(&format!("{}({}, {}, {});", set_macro, n, i, val));
                    }
                    AssignTarget::Field(obj, fld) => {
                        let o = self.emit_expr(obj);
                        let is_ptr = if let Expr::Ident(n) = obj.as_ref() {
                            self.var_types.get(n.as_str()).map(|t| t.starts_with('*')).unwrap_or(false)
                        } else { false };
                        if is_ptr {
                            self.iline(&format!("{}->{} = {};", o, fld, val));
                        } else {
                            self.iline(&format!("{}.{} = {};", o, fld, val));
                        }
                    }
                    AssignTarget::Deref(ptr_expr) => {
                        let p = self.emit_expr(ptr_expr);
                        self.iline(&format!("*({}) = {};", p, val));
                    }
                }
            }

            Stmt::Return(None) => {
                if self.has_defer {
                    // Jump to cleanup block; void functions just fall off after it.
                    self.iline(&format!("goto _vdefer_{};", self.defer_label_id));
                } else {
                    self.iline("return;");
                }
            }
            Stmt::Return(Some(expr)) => {
                let v = self.emit_expr(expr);
                if self.has_defer {
                    // Store return value, then jump to cleanup block.
                    self.iline(&format!("_vdefer_ret = {};", v));
                    self.iline(&format!("goto _vdefer_{};", self.defer_label_id));
                } else {
                    self.iline(&format!("return {};", v));
                }
            }
            // Defer expressions are collected by emit_body_with_defers; nothing
            // to emit at the point where 'defer' appears in the source.
            Stmt::Defer { .. } => {}
            Stmt::Break              => self.iline("break;"),
            Stmt::Continue           => self.iline("continue;"),

            Stmt::If { cond, then_body, else_ifs, else_body, .. } => {
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

            Stmt::While { cond, body, .. } => {
                let c = self.emit_cond(cond);
                self.iline(&format!("while ({}) {{", c));
                self.indent += 1; for s in body { self.emit_stmt(s); } self.indent -= 1;
                self.iline("}");
            }

            // for x in range (0..n or 0..=n)
            Stmt::For { var, iter, body, .. } => {
                match iter {
                    Expr::Range { start, end, inclusive } => {
                        let s = self.emit_expr(start);
                        let e = self.emit_expr(end);
                        let op = if *inclusive { "<=" } else { "<" };
                        self.var_types.insert(var.clone(), "i64".into());
                        self.iline(&format!("for (int64_t {v}={s}; {v}{op}{e}; {v}++) {{", v=var, s=s, op=op, e=e));
                    }
                    Expr::Ident(arr_name) => {
                        let tmp = self.tmp();
                        self.var_types.insert(var.clone(), "i64".into());
                        self.iline(&format!("for (int64_t {tmp}=0; {tmp}<{arr}.len; {tmp}++) {{", tmp=tmp, arr=arr_name));
                        self.indent += 1;
                        self.iline(&format!("int64_t {var} = AGET_INT({arr}, {tmp});", var=var, arr=arr_name, tmp=tmp));
                        for s in body { self.emit_stmt(s); }
                        self.indent -= 1;
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
            Stmt::ForIndex { idx, var, iter, body, .. } => {
                let arr = self.emit_expr(iter);
                self.iline(&format!("for (int64_t {i}=0; {i}<{arr}.len; {i}++) {{", i=idx, arr=arr));
                self.indent += 1;
                self.iline(&format!("int64_t {v} = AGET_INT({arr}, {i});", v=var, arr=arr, i=idx));
                for s in body { self.emit_stmt(s); }
                self.indent -= 1;
                self.iline("}");
            }

            Stmt::ExprStmt(expr) => { let e = self.emit_expr(expr); self.iline(&format!("{};", e)); }

            Stmt::Match { expr, arms, .. } => {
                let e = self.emit_expr(expr);
                let mut first = true;
                for arm in arms {
                    match &arm.pattern {
                        MatchPattern::Wildcard => {
                            // Wildcard must not be first (would shadow all arms above it)
                            self.iline("} else {");
                        }
                        pat => {
                            let cond = self.match_pattern_cond(pat, &e);
                            if first {
                                self.iline(&format!("if ({}) {{", cond));
                                first = false;
                            } else {
                                self.iline(&format!("}} else if ({}) {{", cond));
                            }
                        }
                    }
                    self.indent += 1;
                    for s in &arm.body { self.emit_stmt(s); }
                    self.indent -= 1;
                }
                if !arms.is_empty() { self.iline("}"); }
            }

            Stmt::FnDef(_) | Stmt::ExternBlock(_) | Stmt::DeviceBlock(_)
            | Stmt::StructDef(_) | Stmt::PackedStructDef(_) | Stmt::EnumDef(_)
            | Stmt::Const { .. } | Stmt::TypeAlias { .. } => {}
        }
    }

    // ── Match helpers ─────────────────────────────────────────────────────────

    /// Build the C boolean condition for a single match pattern.
    /// `subject` is the already-emitted C expression being matched on.
    fn match_pattern_cond(&self, pat: &MatchPattern, subject: &str) -> String {
        match pat {
            MatchPattern::Variant { enum_name, variant } => {
                let macro_name = format!("{}_{}", enum_name.to_uppercase(), variant.to_uppercase());
                format!("{} == {}", subject, macro_name)
            }
            MatchPattern::Integer(n) => format!("{} == {}", subject, n),
            MatchPattern::Bool(b)    => format!("{} == {}", subject, if *b { "true" } else { "false" }),
            MatchPattern::Str(s)     => format!("strcmp({}, \"{}\") == 0", subject, escape_str(s)),
            MatchPattern::Range { start, end, inclusive } => {
                let op = if *inclusive { "<=" } else { "<" };
                format!("{s} >= {lo} && {s} {op} {hi}", s = subject, lo = start, op = op, hi = end)
            }
            MatchPattern::Wildcard => unreachable!("wildcard handled as else branch"),
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
                    // Determine the type of the interpolated expression
                    let is_str = self.var_types.get(&expr_src).map(|t| t == "str").unwrap_or(false)
                        || expr_src.starts_with('"');
                    // Check if it's a known str-returning function call like str_upper(x)
                    let func_name = expr_src.split('(').next().unwrap_or("").trim();
                    const STR_FNS: &[&str] = &[
                        "int_to_str","float_to_str","bool_to_str","str_upper","str_lower",
                        "str_reverse","str_repeat","str_slice","str_replace","char_from",
                        "rot13","caesar","xor_str","hex","bytes_to_hex","str_to_hex_str",
                        "arg_get","input","xor_encrypt","str_to_hex",
                    ];
                    const BOOL_FNS: &[&str] = &[
                        "is_prime","is_even","is_odd","looks_base64","looks_encrypted",
                        "is_b64_char","str_eq","str_contains","str_ends_with","str_starts_with",
                        "is_printable","is_alpha","is_digit_char",
                    ];
                    const FLOAT_FNS: &[&str] = &[
                        "entropy","fsqrt","ffloor","fceil","float_to_str","to_float",
                    ];
                    let is_str_fn = STR_FNS.contains(&func_name)
                        || self.fn_types.get(func_name).map(|t| t == "const char*").unwrap_or(false);
                    let is_bool_fn = BOOL_FNS.contains(&func_name)
                        || self.fn_types.get(func_name).map(|t| t == "bool").unwrap_or(false);
                    let is_float_fn = FLOAT_FNS.contains(&func_name)
                        || self.fn_types.get(func_name).map(|t| t == "double" || t == "float").unwrap_or(false);
                    let is_bool = self.var_types.get(&expr_src).map(|t| t == "bool").unwrap_or(false) || is_bool_fn;
                    let is_float = self.var_types.get(&expr_src).map(|t| t == "f64" || t == "f32" || t == "float").unwrap_or(false) || is_float_fn;
                    // Handle array indexing: nums[i] -> AGET_INT(nums, i)
                    let expr_emit = if let Some(bracket) = expr_src.find('[') {
                        let close    = expr_src.rfind(']').unwrap_or(expr_src.len());
                        let arr_name = expr_src[..bracket].trim().to_string();
                        let idx_part = expr_src[bracket+1..close].trim().to_string();
                        format!("AGET_INT({}, {})", arr_name, idx_part)
                    } else {
                        expr_src.clone()
                    };
                    if is_str || is_str_fn {
                        parts.push(expr_emit);
                    } else if is_bool {
                        parts.push(format!("bool_to_str({})", expr_emit));
                    } else if is_float {
                        parts.push(format!("float_to_str({})", expr_emit));
                    } else {
                        parts.push(format!("int_to_str({})", expr_emit));
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

    fn emit_cond(&mut self, expr: &Expr) -> String {
        let s = self.emit_expr(expr);
        if s.starts_with('(') && s.ends_with(')') { s[1..s.len()-1].to_string() } else { s }
    }

    fn emit_expr(&mut self, expr: &Expr) -> String {
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
                let mut fs: Vec<String> = Vec::new();
                for (fn_, fv) in fields {
                    let v = self.emit_expr(fv);
                    fs.push(format!(".{} = {}", fn_, v));
                }
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
                    UnaryOp::Ref    => format!("(&({}))", e),
                    UnaryOp::Deref  => format!("(*({}))", e),
                }
            }

            Expr::Try(inner) => {
                let e = self.emit_expr(inner);
                if self.in_result_fn {
                    format!("({{VResult _vt={e};if(!_vt.ok){{return _vt;}}_vt.ival;}})", e=e)
                } else {
                    format!("({{VResult _vt={e};if(!_vt.ok){{fprintf(stderr,\"error: %s\\n\",_vt.err);exit(1);}}_vt.ival;}})", e=e)
                }
            }

            Expr::Call { name, args } => {
                // Ok(val) — Result constructor
                if name == "Ok" && args.len() == 1 {
                    let a = self.emit_expr(&args[0]);
                    let ty = self.infer_type(&args[0]);
                    return match ty.as_str() {
                        "const char*" => format!("_volt_ok_s({})", a),
                        "double" | "float" => format!("_volt_ok_f({})", a),
                        _ => format!("_volt_ok_i({})", a),
                    };
                }
                // Err(msg) — Result error constructor
                if name == "Err" && args.len() == 1 {
                    let a = self.emit_expr(&args[0]);
                    return format!("_volt_err({})", a);
                }
                // Built-in print — accepts any type, auto-converts
                if name == "print" {
                    if args.is_empty() {
                        return "print(\"\")".to_string();
                    }
                    let mut parts: Vec<String> = Vec::new();
                    for a in args { let s = self.coerce_str(a); parts.push(s); }
                    if parts.len() == 1 {
                        return format!("print({})", parts[0]);
                    }
                    let mut joined = parts[0].clone();
                    for p in &parts[1..] {
                        joined = format!("_concat(_concat({}, \" \"), {})", joined, p);
                    }
                    return format!("print({})", joined);
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
                    // Use element type from annotation when available
                    let elem_ty = if let Expr::Ident(arr_name) = &args[0] {
                        self.var_types.get(arr_name.as_str()).map(|s| s.as_str()).unwrap_or("")
                    } else { "" };
                    return match elem_ty {
                        "[str]"                       => format!("APUSH_STR({}, {})", arr, val),
                        "[f64]" | "[f32]" | "[float]" => format!("APUSH_FLT({}, {})", arr, val),
                        t if t.starts_with('[')       => format!("APUSH_INT({}, {})", arr, val),
                        _ => {
                            // Fallback: infer from value type
                            if self.infer_type(&args[1]) == "const char*" {
                                format!("APUSH_STR({}, {})", arr, val)
                            } else {
                                format!("APUSH_INT({}, {})", arr, val)
                            }
                        }
                    };
                }
                if name == "pop" && args.len() == 1 {
                    let arr = self.emit_expr(&args[0]);
                    return format!("((int64_t)(intptr_t)_arr_pop(&{}))", arr);
                }
                // Memory builtins
                if name == "alloc" && args.len() == 1 {
                    let n = self.emit_expr(&args[0]);
                    return format!("malloc({})", n);
                }
                if name == "free" && args.len() == 1 {
                    let p = self.emit_expr(&args[0]);
                    return format!("free({})", p);
                }
                let mut a: Vec<String> = Vec::new();
                for arg in args { a.push(self.emit_expr(arg)); }
                format!("{}({})", Self::safe_name(name), a.join(", "))
            }

            Expr::MethodCall { target, method, args } => {
                if method == "unwrap" && args.is_empty() {
                    let t = self.emit_expr(target);
                    return format!(
                        "({{VResult _vt={t};if(!_vt.ok){{fprintf(stderr,\"error: %s\\n\",_vt.err);exit(1);}}_vt.ival;}})",
                        t=t
                    );
                }
                if method == "unwrap_str" && args.is_empty() {
                    let t = self.emit_expr(target);
                    return format!(
                        "({{VResult _vt={t};if(!_vt.ok){{fprintf(stderr,\"error: %s\\n\",_vt.err);exit(1);}}_vt.sval;}})",
                        t=t
                    );
                }
                if method == "is_ok" && args.is_empty() {
                    let t = self.emit_expr(target);
                    return format!("({}.ok)", t);
                }
                if method == "is_err" && args.is_empty() {
                    let t = self.emit_expr(target);
                    return format!("(!{}.ok)", t);
                }
                if method == "unwrap_err" && args.is_empty() {
                    let t = self.emit_expr(target);
                    return format!("({}.err)", t);
                }
                let t = self.emit_expr(target);
                let mut a: Vec<String> = Vec::new();
                for arg in args { a.push(self.emit_expr(arg)); }
                format!("{}.{}({})", t, method, a.join(", "))
            }

            Expr::Field { target, field } => {
                if let Expr::Ident(name) = target.as_ref() {
                    if self.enum_defs.contains_key(name.as_str()) {
                        return format!("{}_{}", name.to_uppercase(), field.to_uppercase());
                    }
                    // Auto-deref: p.field where p is *Struct → p->field
                    if self.var_types.get(name.as_str()).map(|t| t.starts_with('*')).unwrap_or(false) {
                        return format!("{}->{}", name, field);
                    }
                }
                let t = self.emit_expr(target);
                // If target is already a deref expression (*p).field, C handles it fine
                format!("{}.{}", t, field)
            }
            Expr::Index { target, index } => {
                let t = self.emit_expr(target);
                let i = self.emit_expr(index);
                let arr_ty = if let Expr::Ident(arr_name) = target.as_ref() {
                    self.var_types.get(arr_name.as_str()).map(|s| s.as_str()).unwrap_or("")
                } else { "" };
                // Fixed-size stack array → direct C index
                if arr_ty.starts_with('[') && arr_ty.contains(';') {
                    return format!("{}[{}]", t, i);
                }
                match arr_ty {
                    "[str]"                       => format!("AGET_STR({}, {})", t, i),
                    "[f64]" | "[f32]" | "[float]" => format!("AGET_FLT({}, {})", t, i),
                    _                             => format!("AGET_INT({}, {})", t, i),
                }
            }

            // Closure literal — pre-scan already emitted the C function;
            // just return the name (counter increments in same DFS order).
            Expr::Closure { .. } => {
                self.closure_counter += 1;
                format!("_vclosure_{}", self.closure_counter)
            }
        }
    }

    fn coerce_str(&mut self, expr: &Expr) -> String {
        match expr {
            Expr::StringLit(_)  => self.emit_expr(expr),
            Expr::Bool(_)       => format!("bool_to_str({})", self.emit_expr(expr)),
            Expr::Float(_)      => format!("float_to_str({})", self.emit_expr(expr)),
            Expr::Integer(_)    => format!("int_to_str({})", self.emit_expr(expr)),
            Expr::Cast { ty, .. } if ty == "f64" || ty == "f32" || ty == "float"
                                => format!("float_to_str({})", self.emit_expr(expr)),
            Expr::Ident(name) => {
                let ty = self.var_types.get(name).map(|s| s.as_str()).unwrap_or("");
                match ty {
                    "f64"|"f32"|"float" => format!("float_to_str({})", name),
                    "bool"              => format!("bool_to_str({})", name),
                    "str"               => name.clone(),
                    _                   => format!("int_to_str({})", name),
                }
            }
            Expr::BinOp { op: BinOp::Concat, .. } => self.emit_expr(expr), // already a string
            Expr::Call { name, .. } => {
                // Already returns a string — pass through
                const STR_BUILTINS: &[&str] = &[
                    "int_to_str","float_to_str","bool_to_str","str_upper","str_lower",
                    "str_reverse","str_repeat","str_pad_left","str_pad_right","str_slice",
                    "str_replace","char_from","rot13","caesar","xor_str","hex",
                    "bytes_to_hex","str_to_hex_str","arg_get","input","xor_encrypt",
                    "str_to_hex","greet","repeat","file_read","file_readline",
                ];
                if STR_BUILTINS.contains(&name.as_str()) {
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
            Expr::BinOp { op: BinOp::Add, left, .. } => {
                // Propagate type from left side
                match self.infer_type(left).as_str() {
                    "double" | "float" => format!("float_to_str({})", self.emit_expr(expr)),
                    "const char*"      => self.emit_expr(expr),
                    _                  => format!("int_to_str({})", self.emit_expr(expr)),
                }
            }
            Expr::BinOp { .. } => {
                format!("int_to_str({})", self.emit_expr(expr))
            }
            _ => {
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
            if let Some(ty_str) = &p.ty {
                if ty_str.starts_with("fn(") {
                    if let Some((inner_params, ret)) = parse_fn_ptr_ty(ty_str) {
                        let ret_c = self.resolve_ty(Some(&ret));
                        let pc: Vec<String> = inner_params.iter()
                            .map(|t| self.resolve_ty(Some(t))).collect();
                        return format!("{} (*{})({})", ret_c, p.name, pc.join(", "));
                    }
                }
            }
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

// Parse "fn(i64,str)->bool" → (["i64","str"], "bool")
fn parse_fn_ptr_ty(ty: &str) -> Option<(Vec<String>, String)> {
    if !ty.starts_with("fn(") { return None; }
    let rest = &ty[3..];
    let mut depth = 1usize;
    let mut close = rest.len();
    for (i, c) in rest.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => { depth -= 1; if depth == 0 { close = i; break; } }
            _ => {}
        }
    }
    let params_str = &rest[..close];
    let params: Vec<String> = if params_str.is_empty() {
        vec![]
    } else {
        params_str.split(',').map(|s| s.trim().to_string()).collect()
    };
    let after = &rest[close+1..];
    let ret = if after.starts_with("->") { after[2..].to_string() } else { "nil".to_string() };
    Some((params, ret))
}

/// Recursively collect all `Stmt::Defer` expressions from a statement list.
/// The order matches source order; callers reverse for LIFO execution.
fn collect_defers(stmts: &[Stmt]) -> Vec<Expr> {
    let mut out = Vec::new();
    collect_defers_rec(stmts, &mut out);
    out
}

fn collect_defers_rec(stmts: &[Stmt], out: &mut Vec<Expr>) {
    for stmt in stmts {
        match stmt {
            Stmt::Defer { expr, .. } => out.push(expr.clone()),
            Stmt::If { then_body, else_ifs, else_body, .. } => {
                collect_defers_rec(then_body, out);
                for (_, b) in else_ifs { collect_defers_rec(b, out); }
                if let Some(b) = else_body { collect_defers_rec(b, out); }
            }
            Stmt::While { body, .. }
            | Stmt::For { body, .. }
            | Stmt::ForIndex { body, .. } => collect_defers_rec(body, out),
            Stmt::Match { arms, .. } => {
                for arm in arms { collect_defers_rec(&arm.body, out); }
            }
            _ => {}
        }
    }
}

/// Return a safe zero initialiser for a C type string.
/// Used to pre-initialise the `_vdefer_ret` variable before any goto.
fn default_value_for_c_type(c_ty: &str) -> &'static str {
    if c_ty.ends_with('*') { "NULL" }
    else { "{0}" }  // valid C99 zero-initialiser for scalars and structs alike
}

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
    let mut r = String::new();
    for c in s.chars() {
        match c {
            '\\' => r.push_str("\\\\"),
            '"'    => r.push_str("\\\""),
            '\n'  => r.push_str("\\n"),
            '\r'  => r.push_str("\\r"),
            '\t'  => r.push_str("\\t"),
            '\0'  => r.push_str("\\0"),
            c      => r.push(c),
        }
    }
    r
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
        Emitter::new().emit_program(&prog).unwrap()
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
