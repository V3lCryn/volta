// volta/src/parser.rs

use crate::lexer::{Token, TokenKind};
use crate::ast::*;

pub struct Parser {
    tokens: Vec<Token>,
    pos:    usize,
}

#[derive(Debug)]
pub struct ParseError {
    pub msg:  String,
    pub line: usize,
    pub col:  usize,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "parse error (line {}:{}): {}", self.line, self.col, self.msg)
    }
}

impl std::error::Error for ParseError {}

type PR<T> = Result<T, ParseError>;

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self { Parser { tokens, pos: 0 } }

    fn peek(&self) -> &TokenKind { &self.tokens[self.pos].kind }
    fn peek_line(&self) -> usize { self.tokens[self.pos].line }
    fn peek_col(&self)  -> usize { self.tokens[self.pos].col  }

    fn advance(&mut self) -> &Token {
        let tok = &self.tokens[self.pos];
        if self.pos + 1 < self.tokens.len() { self.pos += 1; }
        tok
    }

    fn skip_newlines(&mut self) {
        while *self.peek() == TokenKind::Newline { self.advance(); }
    }

    fn check(&self, kind: &TokenKind) -> bool {
        std::mem::discriminant(self.peek()) == std::mem::discriminant(kind)
    }

    fn eat(&mut self, kind: &TokenKind) -> bool {
        if self.check(kind) { self.advance(); true } else { false }
    }

    fn expect(&mut self, kind: &TokenKind) -> PR<()> {
        self.skip_newlines();
        if self.check(kind) { self.advance(); Ok(()) }
        else { Err(ParseError {
            msg:  format!("expected {:?}, got {:?}", kind, self.peek()),
            line: self.peek_line(),
            col:  self.peek_col(),
        })}
    }

    fn expect_ident(&mut self) -> PR<String> {
        self.skip_newlines();
        if let TokenKind::Ident(name) = self.peek().clone() { self.advance(); Ok(name) }
        else { Err(ParseError {
            msg:  format!("expected identifier, got {:?}", self.peek()),
            line: self.peek_line(),
            col:  self.peek_col(),
        })}
    }

    fn expect_string(&mut self) -> PR<String> {
        self.skip_newlines();
        if let TokenKind::StringLit(s) = self.peek().clone() { self.advance(); Ok(s) }
        else { Err(ParseError {
            msg:  format!("expected string, got {:?}", self.peek()),
            line: self.peek_line(),
            col:  self.peek_col(),
        })}
    }

    // Parse a type name:
    //   ident          → plain type
    //   *T             → pointer to T (recursive, so **T works)
    //   [ident]        → dynamic typed array
    //   [ident; N]     → fixed-size stack array
    fn parse_type(&mut self) -> PR<String> {
        self.skip_newlines();
        if self.check(&TokenKind::Star) {
            self.advance();
            let inner = self.parse_type()?;
            return Ok(format!("*{}", inner));
        }
        if self.check(&TokenKind::LBracket) {
            self.advance();
            let elem = self.expect_ident()?;
            self.skip_newlines();
            if self.eat(&TokenKind::Semicolon) {
                // fixed-size: [i64; 8]
                self.skip_newlines();
                let size = if let TokenKind::Integer(n) = self.peek().clone() {
                    self.advance(); n as usize
                } else {
                    return Err(ParseError {
                        msg:  "expected array size (integer)".into(),
                        line: self.peek_line(),
                        col:  self.peek_col(),
                    });
                };
                self.expect(&TokenKind::RBracket)?;
                Ok(format!("[{};{}]", elem, size))
            } else {
                // dynamic: [i64]
                self.expect(&TokenKind::RBracket)?;
                Ok(format!("[{}]", elem))
            }
        } else {
            self.expect_ident()
        }
    }

    fn eat_newlines_and(&mut self, kind: &TokenKind) -> bool {
        let saved = self.pos;
        self.skip_newlines();
        if self.check(kind) { true } else { self.pos = saved; false }
    }

    fn at_end(&self) -> bool { matches!(self.peek(), TokenKind::Eof) }

    fn at_block_end(&self) -> bool {
        matches!(self.peek(), TokenKind::End | TokenKind::Else | TokenKind::ElseIf | TokenKind::Eof)
    }
}

// ── Top-level ─────────────────────────────────────────────────────────────────

impl Parser {
    pub fn parse_program(&mut self) -> PR<Program> {
        let mut stmts = Vec::new();
        loop {
            self.skip_newlines();
            if self.at_end() { break; }
            stmts.push(self.parse_stmt()?);
        }
        Ok(Program { stmts })
    }

    fn parse_block(&mut self) -> PR<Vec<Stmt>> {
        let mut stmts = Vec::new();
        loop {
            self.skip_newlines();
            if self.at_block_end() { break; }
            stmts.push(self.parse_stmt()?);
        }
        Ok(stmts)
    }
}

// ── Statements ────────────────────────────────────────────────────────────────

impl Parser {
    fn parse_stmt(&mut self) -> PR<Stmt> {
        self.skip_newlines();
        match self.peek().clone() {
            TokenKind::Import   => { 
                // imports are resolved by the compiler before parsing
                // skip the import line token by token until newline
                self.advance(); // eat 'import'
                while !matches!(self.peek(), TokenKind::Newline | TokenKind::Eof) {
                    self.advance();
                }
                return self.parse_stmt(); // parse next statement
            }
            TokenKind::Let      => self.parse_let(),
            TokenKind::Const    => self.parse_const(),
            TokenKind::Return   => self.parse_return(),
            TokenKind::Break    => { self.advance(); Ok(Stmt::Break) }
            TokenKind::Continue => { self.advance(); Ok(Stmt::Continue) }
            TokenKind::If       => self.parse_if(),
            TokenKind::While    => self.parse_while(),
            TokenKind::For      => self.parse_for(),
            TokenKind::Fn      => self.parse_fn_def(false),
            TokenKind::Pub      => { self.advance(); self.parse_fn_def(true) }
            TokenKind::Struct   => self.parse_struct_def(),
            TokenKind::Packed   => self.parse_packed_struct_def(),
            TokenKind::Enum     => self.parse_enum_def(),
            TokenKind::Match    => self.parse_match(),
            TokenKind::At       => self.parse_at_block(),
            _                   => self.parse_expr_stmt(),
        }
    }

    fn parse_const(&mut self) -> PR<Stmt> {
        let line = self.peek_line();
        self.advance();
        let name = self.expect_ident()?;
        let ty = if self.eat(&TokenKind::Colon) { Some(self.parse_type()?) } else { None };
        self.expect(&TokenKind::Eq)?;
        let value = self.parse_expr()?;
        Ok(Stmt::Const { name, ty, value, line })
    }

    fn parse_let(&mut self) -> PR<Stmt> {
        let line = self.peek_line();
        self.advance();
        let name = self.expect_ident()?;
        let ty = if self.eat(&TokenKind::Colon) { Some(self.parse_type()?) } else { None };
        self.expect(&TokenKind::Eq)?;
        let value = self.parse_expr()?;
        Ok(Stmt::Let { name, ty, value, line })
    }

    fn parse_return(&mut self) -> PR<Stmt> {
        self.advance();
        self.skip_newlines();
        if matches!(self.peek(), TokenKind::End | TokenKind::Newline | TokenKind::Eof) {
            Ok(Stmt::Return(None))
        } else {
            Ok(Stmt::Return(Some(self.parse_expr()?)))
        }
    }

    fn parse_if(&mut self) -> PR<Stmt> {
        let line = self.peek_line();
        self.advance();
        let cond = self.parse_expr()?;
        self.expect(&TokenKind::Do)?;
        let then_body = self.parse_block()?;
        let mut else_ifs = Vec::new();
        let mut else_body = None;
        loop {
            if self.eat_newlines_and(&TokenKind::ElseIf) {
                self.advance();
                let ei_cond = self.parse_expr()?;
                self.expect(&TokenKind::Do)?;
                else_ifs.push((ei_cond, self.parse_block()?));
            } else if self.eat_newlines_and(&TokenKind::Else) {
                self.advance(); // eat 'else'
                // Support "else if" as two words (same as elseif)
                self.skip_newlines();
                if self.check(&TokenKind::If) {
                    self.advance(); // eat 'if'
                    let ei_cond = self.parse_expr()?;
                    self.expect(&TokenKind::Do)?;
                    let ei_body = self.parse_block()?;
                    else_ifs.push((ei_cond, ei_body));
                    continue;
                }
                else_body = Some(self.parse_block()?);
                break;
            } else { break; }
        }
        self.expect(&TokenKind::End)?;
        Ok(Stmt::If { cond, then_body, else_ifs, else_body, line })
    }

    fn parse_while(&mut self) -> PR<Stmt> {
        let line = self.peek_line();
        self.advance();
        let cond = self.parse_expr()?;
        self.expect(&TokenKind::Do)?;
        let body = self.parse_block()?;
        self.expect(&TokenKind::End)?;
        Ok(Stmt::While { cond, body, line })
    }

    fn parse_for(&mut self) -> PR<Stmt> {
        let line = self.peek_line();
        self.advance(); // eat 'for'

        // Check for "for i, x in ..." (index + value form)
        let first_var = self.expect_ident()?;
        if self.eat(&TokenKind::Comma) {
            let second_var = self.expect_ident()?;
            self.expect(&TokenKind::In)?;
            let iter = self.parse_range_or_expr()?;
            self.expect(&TokenKind::Do)?;
            let body = self.parse_block()?;
            self.expect(&TokenKind::End)?;
            return Ok(Stmt::ForIndex { idx: first_var, var: second_var, iter, body, line });
        }

        self.expect(&TokenKind::In)?;
        let iter = self.parse_range_or_expr()?;
        self.expect(&TokenKind::Do)?;
        let body = self.parse_block()?;
        self.expect(&TokenKind::End)?;
        Ok(Stmt::For { var: first_var, iter, body, line })
    }

    fn parse_fn_def(&mut self, is_pub: bool) -> PR<Stmt> {
        let line = self.peek_line();
        self.expect(&TokenKind::Fn)?;
        let name = self.expect_ident()?;
        self.expect(&TokenKind::LParen)?;
        let params = self.parse_param_list()?;
        self.expect(&TokenKind::RParen)?;
        let ret_ty = if self.eat(&TokenKind::Arrow) { Some(self.parse_type()?) } else { None };
        self.eat(&TokenKind::Do);
        let body = self.parse_block()?;
        self.expect(&TokenKind::End)?;
        Ok(Stmt::FnDef(FnDef { name, params, ret_ty, body, is_pub, line }))
    }

    fn parse_struct_def(&mut self) -> PR<Stmt> {
        self.advance();
        let name = self.expect_ident()?;
        let mut fields = Vec::new();
        loop {
            self.skip_newlines();
            if matches!(self.peek(), TokenKind::End | TokenKind::Eof) { break; }
            let fname = self.expect_ident()?;
            self.expect(&TokenKind::Colon)?;
            let ftype = self.parse_type()?;
            fields.push((fname, ftype));
        }
        self.expect(&TokenKind::End)?;
        Ok(Stmt::StructDef(StructDef { name, fields }))
    }

    fn parse_param_list(&mut self) -> PR<Vec<Param>> {
        let mut params = Vec::new();
        self.skip_newlines();
        if self.check(&TokenKind::RParen) { return Ok(params); }
        loop {
            let name = self.expect_ident()?;
            let ty = if self.eat(&TokenKind::Colon) { Some(self.parse_type()?) } else { None };
            params.push(Param { name, ty });
            if !self.eat(&TokenKind::Comma) { break; }
        }
        Ok(params)
    }

    fn parse_packed_struct_def(&mut self) -> PR<Stmt> {
        self.advance(); // eat 'packed'
        self.expect(&TokenKind::Struct)?;
        let name = self.expect_ident()?;
        self.expect(&TokenKind::Colon)?;
        let base_ty = self.expect_ident()?;
        let mut fields = Vec::new();
        loop {
            self.skip_newlines();
            if matches!(self.peek(), TokenKind::End | TokenKind::Eof) { break; }
            let fname = self.expect_ident()?;
            self.expect(&TokenKind::Colon)?;
            let bits = if let TokenKind::Integer(n) = self.peek().clone() {
                self.advance();
                n as u8
            } else {
                return Err(ParseError {
                    msg: "expected bit width (integer)".into(),
                    line: self.peek_line(),
                    col:  self.peek_col(),
                });
            };
            fields.push((fname, bits));
        }
        self.expect(&TokenKind::End)?;
        Ok(Stmt::PackedStructDef(PackedStructDef { name, base_ty, fields }))
    }

    fn parse_enum_def(&mut self) -> PR<Stmt> {
        let line = self.peek_line();
        self.advance(); // eat 'enum'
        let name = self.expect_ident()?;
        let mut variants = Vec::new();
        loop {
            self.skip_newlines();
            if matches!(self.peek(), TokenKind::End | TokenKind::Eof) { break; }
            variants.push(self.expect_ident()?);
        }
        self.expect(&TokenKind::End)?;
        Ok(Stmt::EnumDef(EnumDef { name, variants, line }))
    }

    fn parse_match(&mut self) -> PR<Stmt> {
        let line = self.peek_line();
        self.advance(); // eat 'match'
        let expr = self.parse_expr()?;
        self.expect(&TokenKind::Do)?;
        let mut arms = Vec::new();
        loop {
            self.skip_newlines();
            if matches!(self.peek(), TokenKind::End | TokenKind::Eof) { break; }
            let pattern = self.parse_match_pattern()?;
            self.expect(&TokenKind::FatArrow)?;
            // Body: 'do' block or single expression
            let body = if self.eat(&TokenKind::Do) {
                let stmts = self.parse_block()?;
                self.expect(&TokenKind::End)?;
                stmts
            } else {
                vec![Stmt::ExprStmt(self.parse_expr()?)]
            };
            arms.push(MatchArm { pattern, body });
        }
        self.expect(&TokenKind::End)?;
        Ok(Stmt::Match { expr, arms, line })
    }

    fn parse_match_pattern(&mut self) -> PR<MatchPattern> {
        self.skip_newlines();
        let name = self.expect_ident()?;
        if name == "_" {
            return Ok(MatchPattern::Wildcard);
        }
        self.expect(&TokenKind::Dot)?;
        let variant = self.expect_ident()?;
        Ok(MatchPattern::Variant { enum_name: name, variant })
    }

    fn parse_at_block(&mut self) -> PR<Stmt> {
        self.advance();
        self.skip_newlines();
        match self.peek().clone() {
            TokenKind::Extern => { self.advance(); self.parse_extern_block() }
            TokenKind::Device => { self.advance(); self.parse_device_block() }
            TokenKind::Ident(ref s) if s == "extern" => { self.advance(); self.parse_extern_block() }
            TokenKind::Ident(ref s) if s == "device" => { self.advance(); self.parse_device_block() }
            other => Err(ParseError { msg: format!("unknown @ block: {:?}", other), line: self.peek_line(), col: self.peek_col() }),
        }
    }

    fn parse_extern_block(&mut self) -> PR<Stmt> {
        let abi = self.expect_string()?;
        self.expect(&TokenKind::Do)?;
        let mut fns = Vec::new();
        loop {
            self.skip_newlines();
            if matches!(self.peek(), TokenKind::End | TokenKind::Eof) { break; }
            self.expect(&TokenKind::Fn)?;
            let name = self.expect_ident()?;
            self.expect(&TokenKind::LParen)?;
            let params = self.parse_param_list()?;
            self.expect(&TokenKind::RParen)?;
            let ret_ty = if self.eat(&TokenKind::Arrow) { Some(self.parse_type()?) } else { None };
            fns.push(ExternFn { name, params, ret_ty });
        }
        self.expect(&TokenKind::End)?;
        Ok(Stmt::ExternBlock(ExternBlock { abi, fns }))
    }

    fn parse_device_block(&mut self) -> PR<Stmt> {
        let name = self.expect_string()?;
        self.expect(&TokenKind::At)?;
        let address = if let TokenKind::Integer(n) = self.peek().clone() { self.advance(); n as u64 }
        else { return Err(ParseError { msg: "expected address".into(), line: self.peek_line(), col: self.peek_col() }); };
        self.expect(&TokenKind::Do)?;
        let mut regs = Vec::new();
        loop {
            self.skip_newlines();
            if matches!(self.peek(), TokenKind::End | TokenKind::Eof) { break; }
            let kw = self.expect_ident()?;
            if kw != "reg" { return Err(ParseError { msg: "expected 'reg'".into(), line: self.peek_line(), col: self.peek_col() }); }
            let rname = self.expect_ident()?;
            self.expect(&TokenKind::Colon)?;
            let ty = self.expect_ident()?;
            regs.push(Register { name: rname, ty });
        }
        self.expect(&TokenKind::End)?;
        Ok(Stmt::DeviceBlock(DeviceBlock { name, address, regs }))
    }

    fn parse_expr_stmt(&mut self) -> PR<Stmt> {
        let line = self.peek_line();
        let expr = self.parse_expr()?;

        // compound assignments: x += 1, x -= 1, x *= 2, x /= 2
        let compound = match self.peek() {
            TokenKind::PlusEq  => Some(BinOp::Add),
            TokenKind::MinusEq => Some(BinOp::Sub),
            TokenKind::StarEq  => Some(BinOp::Mul),
            TokenKind::SlashEq => Some(BinOp::Div),
            _ => None,
        };
        if let Some(op) = compound {
            self.advance();
            let rhs = self.parse_expr()?;
            if let Expr::Ident(name) = &expr {
                let value = Expr::BinOp { op, left: Box::new(Expr::Ident(name.clone())), right: Box::new(rhs) };
                return Ok(Stmt::Assign { target: AssignTarget::Ident(name.clone()), value, line });
            }
        }

        if self.eat(&TokenKind::Eq) {
            let value = self.parse_expr()?;
            let target = match expr {
                Expr::Ident(name)             => AssignTarget::Ident(name),
                Expr::Index { target, index } => {
                    if let Expr::Ident(name) = *target {
                        AssignTarget::Index(name, index)
                    } else {
                        return Err(ParseError { msg: "invalid assignment target".into(), line: self.peek_line(), col: self.peek_col() });
                    }
                }
                Expr::Field { target, field }                              => AssignTarget::Field(target, field),
                Expr::UnaryOp { op: UnaryOp::Deref, expr: ptr_expr }      => AssignTarget::Deref(ptr_expr),
                _ => return Err(ParseError { msg: "invalid assignment target".into(), line: self.peek_line(), col: self.peek_col() }),
            };
            return Ok(Stmt::Assign { target, value, line });
        }

        Ok(Stmt::ExprStmt(expr))
    }
}

// ── Expressions ───────────────────────────────────────────────────────────────

impl Parser {
    // Parse a range (only valid in for loops) or a regular expr
    fn parse_range_or_expr(&mut self) -> PR<Expr> {
        // For range detection, use parse_prec (not parse_concat) so 0..10 is not concat
        let start = self.parse_prec(0)?;
        if self.check(&TokenKind::DotDot) {
            self.advance();
            // Check for ..= (inclusive)
            let inclusive = self.eat(&TokenKind::Eq);
            let end = self.parse_prec(0)?;
            return Ok(Expr::Range { start: Box::new(start), end: Box::new(end), inclusive });
        }
        if self.check(&TokenKind::DotDotEq) {
            self.advance();
            let end = self.parse_prec(0)?;
            return Ok(Expr::Range { start: Box::new(start), end: Box::new(end), inclusive: true });
        }
        // Not a range — check if it is a concat expression
        self.parse_concat_from(start)
    }

    fn parse_concat_from(&mut self, mut left: Expr) -> PR<Expr> {
        while self.check(&TokenKind::DotDot) {
            self.advance();
            let right = self.parse_prec(0)?;
            left = Expr::BinOp { op: BinOp::Concat, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    pub fn parse_expr(&mut self) -> PR<Expr> { self.parse_concat() }

    // Handle .. (string concat) at lowest precedence, above everything else
    fn parse_concat(&mut self) -> PR<Expr> {
        let mut left = self.parse_prec(0)?;
        // Handle postfix 'as' cast here too
        if self.check(&TokenKind::As) {
            self.advance();
            let ty = self.expect_ident()?;
            left = Expr::Cast { expr: Box::new(left), ty };
        }
        // Now handle .. concat (can chain: "a" .. "b" .. "c")
        while self.check(&TokenKind::DotDot) {
            self.advance();
            let mut right = self.parse_prec(0)?;
            if self.check(&TokenKind::As) {
                self.advance();
                let ty = self.expect_ident()?;
                right = Expr::Cast { expr: Box::new(right), ty };
            }
            left = Expr::BinOp { op: BinOp::Concat, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_prec(&mut self, min_prec: u8) -> PR<Expr> {
        let mut left = self.parse_unary()?;
        loop {
            self.skip_newlines();
            let (prec, right_assoc) = match self.peek() {
                TokenKind::Or          => (1, false),
                TokenKind::And         => (2, false),
                TokenKind::EqEq | TokenKind::BangEq |
                TokenKind::Lt  | TokenKind::LtEq |
                TokenKind::Gt  | TokenKind::GtEq  => (3, false),
                TokenKind::Pipe        => (4, false),
                TokenKind::Caret       => (5, false),
                TokenKind::Ampersand   => (6, false),
                TokenKind::ShiftL | TokenKind::ShiftR => (7, false),
                TokenKind::Plus | TokenKind::Minus  => (9, false),
                TokenKind::Star | TokenKind::Slash | TokenKind::Percent => (10, false),
                _ => break,
            };
            if prec < min_prec { break; }
            let op_tok = self.advance().kind.clone();

            let op = tok_to_binop(&op_tok);
            let next = if right_assoc { prec } else { prec + 1 };
            let right = self.parse_prec(next)?;
            left = Expr::BinOp { op, left: Box::new(left), right: Box::new(right) };
        }

        Ok(left)
    }

    fn parse_unary(&mut self) -> PR<Expr> {
        self.skip_newlines();
        match self.peek().clone() {
            TokenKind::Minus     => { self.advance(); Ok(Expr::UnaryOp { op: UnaryOp::Neg,    expr: Box::new(self.parse_unary()?) }) }
            TokenKind::Not       => { self.advance(); Ok(Expr::UnaryOp { op: UnaryOp::Not,    expr: Box::new(self.parse_unary()?) }) }
            TokenKind::Tilde     => { self.advance(); Ok(Expr::UnaryOp { op: UnaryOp::BitNot, expr: Box::new(self.parse_unary()?) }) }
            TokenKind::Ampersand => { self.advance(); Ok(Expr::UnaryOp { op: UnaryOp::Ref,    expr: Box::new(self.parse_unary()?) }) }
            TokenKind::Star      => { self.advance(); Ok(Expr::UnaryOp { op: UnaryOp::Deref,  expr: Box::new(self.parse_unary()?) }) }
            _                    => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> PR<Expr> {
        let mut expr = self.parse_primary()?;
        loop {
            match self.peek().clone() {
                TokenKind::LParen => {
                    self.advance();
                    let args = self.parse_arg_list()?;
                    self.expect(&TokenKind::RParen)?;
                    match expr {
                        Expr::Ident(name)             => expr = Expr::Call { name, args },
                        Expr::Field { target, field } => expr = Expr::MethodCall { target, method: field, args },
                        _ => return Err(ParseError { msg: "can only call named functions".into(), line: self.peek_line(), col: self.peek_col() }),
                    }
                }
                TokenKind::Dot => {
                    self.advance();
                    let field = self.expect_ident()?;
                    expr = Expr::Field { target: Box::new(expr), field };
                }
                TokenKind::LBracket => {
                    self.advance();
                    let index = self.parse_expr()?;
                    self.expect(&TokenKind::RBracket)?;
                    expr = Expr::Index { target: Box::new(expr), index: Box::new(index) };
                }
                TokenKind::Question => {
                    self.advance();
                    expr = Expr::Try(Box::new(expr));
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> PR<Expr> {
        self.skip_newlines();
        match self.peek().clone() {
            TokenKind::Nil          => { self.advance(); Ok(Expr::Nil) }
            TokenKind::Bool(b)      => { self.advance(); Ok(Expr::Bool(b)) }
            TokenKind::Integer(n)   => { self.advance(); Ok(Expr::Integer(n)) }
            TokenKind::Float(f)     => { self.advance(); Ok(Expr::Float(f)) }
            TokenKind::StringLit(s) => { self.advance(); Ok(Expr::StringLit(s)) }
            TokenKind::LBracket     => self.parse_array(),
            TokenKind::Ident(name)  => {
                self.advance();
                if self.eat_newlines_and(&TokenKind::LBrace) {
                    self.advance();
                    let mut fields = Vec::new();
                    loop {
                        self.skip_newlines();
                        if self.check(&TokenKind::RBrace) { break; }
                        let fname = self.expect_ident()?;
                        self.expect(&TokenKind::Colon)?;
                        let fval = self.parse_expr()?;
                        fields.push((fname, fval));
                        self.eat(&TokenKind::Comma);
                    }
                    self.expect(&TokenKind::RBrace)?;
                    Ok(Expr::StructLit { name, fields })
                } else {
                    Ok(Expr::Ident(name))
                }
            }
            TokenKind::LParen => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(&TokenKind::RParen)?;
                Ok(expr)
            }
            other => Err(ParseError { msg: format!("unexpected token: {:?}", other), line: self.peek_line(), col: self.peek_col() }),
        }
    }

    fn parse_array(&mut self) -> PR<Expr> {
        self.advance();
        let mut elems = Vec::new();
        self.skip_newlines();
        if !self.check(&TokenKind::RBracket) {
            loop {
                elems.push(self.parse_expr()?);
                if !self.eat(&TokenKind::Comma) { break; }
                self.skip_newlines();
            }
        }
        self.expect(&TokenKind::RBracket)?;
        Ok(Expr::Array(elems))
    }

    fn parse_arg_list(&mut self) -> PR<Vec<Expr>> {
        let mut args = Vec::new();
        self.skip_newlines();
        if self.check(&TokenKind::RParen) { return Ok(args); }
        loop {
            args.push(self.parse_expr()?);
            if !self.eat(&TokenKind::Comma) { break; }
            self.skip_newlines();
        }
        Ok(args)
    }
}

fn tok_to_binop(tok: &TokenKind) -> BinOp {
    match tok {
        TokenKind::Plus      => BinOp::Add,
        TokenKind::Minus     => BinOp::Sub,
        TokenKind::Star      => BinOp::Mul,
        TokenKind::Slash     => BinOp::Div,
        TokenKind::Percent   => BinOp::Mod,
        TokenKind::EqEq      => BinOp::Eq,
        TokenKind::BangEq    => BinOp::NotEq,
        TokenKind::Lt        => BinOp::Lt,
        TokenKind::LtEq      => BinOp::LtEq,
        TokenKind::Gt        => BinOp::Gt,
        TokenKind::GtEq      => BinOp::GtEq,
        TokenKind::And       => BinOp::And,
        TokenKind::Or        => BinOp::Or,
        TokenKind::DotDot    => BinOp::Concat,
        TokenKind::Ampersand => BinOp::BitAnd,
        TokenKind::Pipe      => BinOp::BitOr,
        TokenKind::Caret     => BinOp::BitXor,
        TokenKind::ShiftL    => BinOp::ShiftL,
        TokenKind::ShiftR    => BinOp::ShiftR,
        _ => unreachable!("tok_to_binop called on non-operator token: {:?}", tok),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    fn parse(src: &str) -> Program {
        let tokens = Lexer::new(src).tokenize().unwrap();
        Parser::new(tokens).parse_program().unwrap()
    }

    #[test]
    fn let_stmt()          { assert!(matches!(&parse("let x = 42").stmts[0], Stmt::Let { name, .. } if name == "x")); }
    #[test]
    fn let_typed()         { assert!(matches!(&parse("let x: i64 = 42").stmts[0], Stmt::Let { ty: Some(t), .. } if t == "i64")); }
    #[test]
    fn fn_def()            { assert!(matches!(&parse("fn add(a, b)\n  return a + b\nend").stmts[0], Stmt::FnDef(f) if f.name == "add")); }
    #[test]
    fn if_elseif_else()    { assert!(matches!(&parse("if x == 1 do\n  let a = 1\nelseif x == 2 do\n  let b = 2\nelse\n  let c = 3\nend").stmts[0], Stmt::If { else_ifs, else_body: Some(_), .. } if !else_ifs.is_empty())); }
    #[test]
    fn call_expr()         { assert!(matches!(&parse("greet(\"world\")").stmts[0], Stmt::ExprStmt(Expr::Call { name, .. }) if name == "greet")); }
    #[test]
    fn binary_precedence() {
        if let Stmt::Let { value: Expr::BinOp { op, right, .. }, .. } = &parse("let x = 2 + 3 * 4").stmts[0] {
            assert_eq!(*op, BinOp::Add);
            assert!(matches!(right.as_ref(), Expr::BinOp { op: BinOp::Mul, .. }));
        } else { panic!(); }
    }
    #[test]
    fn while_loop()        { assert!(matches!(&parse("while x > 0 do\n  x = x - 1\nend").stmts[0], Stmt::While { .. })); }
    #[test]
    fn array_literal()     { assert!(matches!(&parse("let a = [1, 2, 3]").stmts[0], Stmt::Let { value: Expr::Array(_), .. })); }
    #[test]
    fn struct_def()        { assert!(matches!(&parse("struct Point\n  x: i64\n  y: i64\nend").stmts[0], Stmt::StructDef(s) if s.name == "Point")); }
    #[test]
    fn break_continue() {
        if let Stmt::While { body, .. } = &parse("while true do\n  break\nend").stmts[0] {
            assert!(matches!(&body[0], Stmt::Break));
        } else { panic!(); }
    }
    #[test]
    fn range_expr() {
        assert!(matches!(&parse("for i in 0..10 do\n  print(int_to_str(i))\nend").stmts[0], Stmt::For { .. }));
    }
    #[test]
    fn for_index() {
        assert!(matches!(&parse("for i, x in items do\n  print(int_to_str(i))\nend").stmts[0], Stmt::ForIndex { .. }));
    }
    #[test]
    fn cast_expr() {
        assert!(matches!(&parse("let x = 42 as f64").stmts[0], Stmt::Let { value: Expr::Cast { .. }, .. }));
    }
    #[test]
    fn full_program() {
        let src = "let name = \"Volta\"\nfn greet(who) do\n  return \"Hello, \" .. who\nend\nlet msg = greet(name)";
        assert_eq!(parse(src).stmts.len(), 3);
    }
}
