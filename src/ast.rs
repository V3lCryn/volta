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
    },

    Assign {
        target: AssignTarget,
        value:  Expr,
    },

    Return(Option<Expr>),
    Break,
    Continue,

    If {
        cond:      Expr,
        then_body: Vec<Stmt>,
        else_ifs:  Vec<(Expr, Vec<Stmt>)>,
        else_body: Option<Vec<Stmt>>,
    },

    While {
        cond: Expr,
        body: Vec<Stmt>,
    },

    // for x in iterable
    For {
        var:  String,
        iter: Expr,
        body: Vec<Stmt>,
    },

    // for i, x in arr (index + value)
    ForIndex {
        idx:  String,
        var:  String,
        iter: Expr,
        body: Vec<Stmt>,
    },

    ExprStmt(Expr),
    FnDef(FnDef),
    StructDef(StructDef),
    ExternBlock(ExternBlock),
    DeviceBlock(DeviceBlock),
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
pub struct Program {
    pub stmts: Vec<Stmt>,
}
