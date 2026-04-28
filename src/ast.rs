#![allow(dead_code)]
// volta/src/ast.rs

#[derive(Debug, Clone)]
pub enum Expr {
    Nil,
    Bool(bool),
    Integer(i64),
    Float(f64),
    StringLit(String),
    Ident(String),

    Array(Vec<Expr>),

    Range {
        start:     Box<Expr>,
        end:       Box<Expr>,
        inclusive: bool,
    },

    Cast {
        expr: Box<Expr>,
        ty:   String,
    },

    StructLit {
        name:   String,
        fields: Vec<(String, Expr)>,
    },

    BinOp {
        op:    BinOp,
        left:  Box<Expr>,
        right: Box<Expr>,
    },

    UnaryOp {
        op:   UnaryOp,
        expr: Box<Expr>,
    },

    Call {
        name: String,
        args: Vec<Expr>,
    },

    MethodCall {
        target: Box<Expr>,
        method: String,
        args:   Vec<Expr>,
    },

    Index {
        target: Box<Expr>,
        index:  Box<Expr>,
    },

    Field {
        target: Box<Expr>,
        field:  String,
    },

    // expr? — propagate error early
    Try(Box<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum BinOp {
    Add, Sub, Mul, Div, Mod,
    Eq, NotEq,
    Lt, LtEq, Gt, GtEq,
    And, Or,
    Concat,
    BitAnd, BitOr, BitXor,
    ShiftL, ShiftR,
}

#[derive(Debug, Clone)]
pub enum UnaryOp {
    Neg,
    Not,
    BitNot,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Let {
        name:  String,
        ty:    Option<String>,
        value: Expr,
        line:  usize,
    },

    Assign {
        target: AssignTarget,
        value:  Expr,
        line:   usize,
    },

    Return(Option<Expr>),
    Break,
    Continue,

    If {
        cond:      Expr,
        then_body: Vec<Stmt>,
        else_ifs:  Vec<(Expr, Vec<Stmt>)>,
        else_body: Option<Vec<Stmt>>,
        line:      usize,
    },

    While {
        cond: Expr,
        body: Vec<Stmt>,
        line: usize,
    },

    // for x in iterable
    For {
        var:  String,
        iter: Expr,
        body: Vec<Stmt>,
        line: usize,
    },

    // for i, x in arr (index + value)
    ForIndex {
        idx:  String,
        var:  String,
        iter: Expr,
        body: Vec<Stmt>,
        line: usize,
    },

    Const {
        name:  String,
        ty:    Option<String>,
        value: Expr,
        line:  usize,
    },

    ExprStmt(Expr),
    FnDef(FnDef),
    StructDef(StructDef),
    PackedStructDef(PackedStructDef),
    EnumDef(EnumDef),
    ExternBlock(ExternBlock),
    DeviceBlock(DeviceBlock),
    Match {
        expr: Expr,
        arms: Vec<MatchArm>,
        line: usize,
    },
}

#[derive(Debug, Clone)]
pub enum AssignTarget {
    Ident(String),
    Index(String, Box<Expr>),
    Field(Box<Expr>, String),
}

#[derive(Debug, Clone)]
pub struct FnDef {
    pub name:    String,
    pub params:  Vec<Param>,
    pub ret_ty:  Option<String>,
    pub body:    Vec<Stmt>,
    pub is_pub:  bool,
    pub line:    usize,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub ty:   Option<String>,
}

#[derive(Debug, Clone)]
pub struct StructDef {
    pub name:   String,
    pub fields: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct PackedStructDef {
    pub name:    String,
    pub base_ty: String,              // backing integer type: u8/u16/u32/u64
    pub fields:  Vec<(String, u8)>,   // (field_name, bit_width)
}

#[derive(Debug, Clone)]
pub struct ExternBlock {
    pub abi:  String,
    pub fns:  Vec<ExternFn>,
}

#[derive(Debug, Clone)]
pub struct ExternFn {
    pub name:   String,
    pub params: Vec<Param>,
    pub ret_ty: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DeviceBlock {
    pub name:    String,
    pub address: u64,
    pub regs:    Vec<Register>,
}

#[derive(Debug, Clone)]
pub struct Register {
    pub name: String,
    pub ty:   String,
}

#[derive(Debug, Clone)]
pub struct EnumDef {
    pub name:     String,
    pub variants: Vec<String>,
    pub line:     usize,
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pattern: MatchPattern,
    pub body:    Vec<Stmt>,
}

#[derive(Debug, Clone)]
pub enum MatchPattern {
    Variant { enum_name: String, variant: String },
    Wildcard,
}

#[derive(Debug, Clone)]
pub struct Program {
    pub stmts: Vec<Stmt>,
}
