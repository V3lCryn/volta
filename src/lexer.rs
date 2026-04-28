#![allow(dead_code)]
// volta/src/lexer.rs

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Literals
    Integer(i64),
    Float(f64),
    StringLit(String),
    Bool(bool),

    // Identifiers + keywords
    Ident(String),
    Let, Fn, Return,
    If, Else, ElseIf, While, For, In,
    End, Do, And, Or, Not, Nil,
    Break, Continue, Struct,
    Extern, Device, Import, As, Pub,
    Enum, Match, Packed, Const,

    // Operators
    Plus, Minus, Star, Slash, Percent,
    Eq, EqEq, BangEq,
    Lt, LtEq, Gt, GtEq,
    DotDot, DotDotEq, Dot, Arrow, FatArrow, At,
    PlusEq, MinusEq, StarEq, SlashEq,
    Ampersand, Pipe, Caret, Tilde,
    ShiftL, ShiftR,
    Question,

    // Delimiters
    LParen, RParen, LBrace, RBrace,
    LBracket, RBracket,
    Comma, Colon, ColonColon, Semicolon,
    Newline, Eof,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub line: usize,
    pub col:  usize,
}

impl Token {
    fn new(kind: TokenKind, line: usize, col: usize) -> Self {
        Token { kind, line, col }
    }
}

pub struct Lexer<'a> {
    src:   &'a str,
    bytes: &'a [u8],
    pos:   usize,
    line:  usize,
    col:   usize,
    // store source lines for error reporting
    pub lines: Vec<String>,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        let lines = src.lines().map(|l| l.to_string()).collect();
        Lexer { src, bytes: src.as_bytes(), pos: 0, line: 1, col: 1, lines }
    }

    pub fn tokenize(&mut self) -> Result<Vec<Token>, LexError> {
        let mut tokens = Vec::new();
        loop {
            let tok = self.next_token()?;
            let is_eof = tok.kind == TokenKind::Eof;
            tokens.push(tok);
            if is_eof { break; }
        }
        Ok(tokens)
    }

    fn peek(&self)  -> Option<u8> { self.bytes.get(self.pos).copied() }
    fn peek2(&self) -> Option<u8> { self.bytes.get(self.pos + 1).copied() }

    fn advance(&mut self) -> Option<u8> {
        let ch = self.bytes.get(self.pos).copied()?;
        self.pos += 1;
        if ch == b'\n' { self.line += 1; self.col = 1; }
        else           { self.col  += 1; }
        Some(ch)
    }

    fn eat_if(&mut self, expected: u8) -> bool {
        if self.peek() == Some(expected) { self.advance(); true } else { false }
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            match self.peek() {
                Some(b' ') | Some(b'\t') | Some(b'\r') => { self.advance(); }
                Some(b'-') if self.peek2() == Some(b'-') => {
                    // Check for multiline comment: --[[
                    if self.bytes.get(self.pos + 2) == Some(&b'[') &&
                       self.bytes.get(self.pos + 3) == Some(&b'[') {
                        self.advance(); self.advance(); self.advance(); self.advance();
                        // scan for ]]
                        loop {
                            match self.advance() {
                                None => break,
                                Some(b']') if self.peek() == Some(b']') => {
                                    self.advance(); break;
                                }
                                _ => {}
                            }
                        }
                    } else {
                        // single line comment
                        while self.peek().is_some() && self.peek() != Some(b'\n') {
                            self.advance();
                        }
                    }
                }
                _ => break,
            }
        }
    }

    fn read_string(&mut self) -> Result<TokenKind, LexError> {
        let mut s = String::new();
        loop {
            match self.advance() {
                None | Some(b'\n') =>
                    return Err(LexError::new("unterminated string", self.line, self.col, &self.lines)),
                Some(b'"') => break,
                Some(b'\\') => match self.advance() {
                    Some(b'n')  => s.push('\n'),
                    Some(b't')  => s.push('\t'),
                    Some(b'r')  => s.push('\r'),
                    Some(b'"')  => s.push('"'),
                    Some(b'\\') => s.push('\\'),
                    Some(b'0')  => s.push('\0'),
                    Some(b'{')  => s.push('{'),
                    other => return Err(LexError::new(
                        &format!("unknown escape \\{}", other.map(|c| c as char).unwrap_or('?')),
                        self.line, self.col, &self.lines,
                    )),
                },
                Some(ch) => s.push(ch as char),
            }
        }
        Ok(TokenKind::StringLit(s))
    }

    fn read_number(&mut self, first: u8) -> Result<TokenKind, LexError> {
        let mut num = String::new();
        num.push(first as char);
        let mut is_float = false;
        // Hex: 0x...
        if first == b'0' && self.peek() == Some(b'x') {
            self.advance();
            let mut hex = String::new();
            while let Some(c) = self.peek() {
                if c.is_ascii_hexdigit() { hex.push(c as char); self.advance(); } else { break; }
            }
            return i64::from_str_radix(&hex, 16)
                .map(TokenKind::Integer)
                .map_err(|_| LexError::new(
                    &format!("invalid hex literal '0x{}'", hex),
                    self.line, self.col, &self.lines,
                ));
        }
        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() { num.push(ch as char); self.advance(); }
            else if ch == b'.' && self.peek2().map_or(false, |c| c.is_ascii_digit()) {
                is_float = true; num.push('.'); self.advance();
            } else { break; }
        }
        if is_float {
            num.parse::<f64>()
                .map(TokenKind::Float)
                .map_err(|_| LexError::new("invalid float literal", self.line, self.col, &self.lines))
        } else {
            num.parse::<i64>()
                .map(TokenKind::Integer)
                .map_err(|_| LexError::new("invalid integer literal", self.line, self.col, &self.lines))
        }
    }

    fn read_ident(&mut self, first: u8) -> TokenKind {
        let mut s = String::new();
        s.push(first as char);
        while let Some(ch) = self.peek() {
            if ch.is_ascii_alphanumeric() || ch == b'_' { s.push(ch as char); self.advance(); }
            else { break; }
        }
        match s.as_str() {
            "let"      => TokenKind::Let,
            "fn"       => TokenKind::Fn,
            "return"   => TokenKind::Return,
            "if"       => TokenKind::If,
            "else"     => TokenKind::Else,
            "elseif"   => TokenKind::ElseIf,
            "while"    => TokenKind::While,
            "for"      => TokenKind::For,
            "in"       => TokenKind::In,
            "end"      => TokenKind::End,
            "do"       => TokenKind::Do,
            "and"      => TokenKind::And,
            "or"       => TokenKind::Or,
            "not"      => TokenKind::Not,
            "nil"      => TokenKind::Nil,
            "true"     => TokenKind::Bool(true),
            "false"    => TokenKind::Bool(false),
            "break"    => TokenKind::Break,
            "continue" => TokenKind::Continue,
            "struct"   => TokenKind::Struct,
            "extern"   => TokenKind::Extern,
            "device"   => TokenKind::Device,
            "import"   => TokenKind::Import,
            "as"       => TokenKind::As,
            "pub"      => TokenKind::Pub,
            "enum"     => TokenKind::Enum,
            "match"    => TokenKind::Match,
            "packed"   => TokenKind::Packed,
            "const"    => TokenKind::Const,
            _          => TokenKind::Ident(s),
        }
    }

    fn next_token(&mut self) -> Result<Token, LexError> {
        self.skip_whitespace_and_comments();
        let line = self.line;
        let col  = self.col;

        let ch = match self.advance() {
            None     => return Ok(Token::new(TokenKind::Eof, line, col)),
            Some(ch) => ch,
        };

        let kind = match ch {
            b'\n' => TokenKind::Newline,
            b'"'  => self.read_string()?,
            b'@'  => TokenKind::At,
            b'#'  => { while self.peek().is_some() && self.peek() != Some(b'\n') { self.advance(); } return self.next_token(); }
            b'~'  => TokenKind::Tilde,
            b'^'  => TokenKind::Caret,
            b'&'  => TokenKind::Ampersand,
            b'|'  => TokenKind::Pipe,
            b'('  => TokenKind::LParen,
            b')'  => TokenKind::RParen,
            b'{'  => TokenKind::LBrace,
            b'}'  => TokenKind::RBrace,
            b'['  => TokenKind::LBracket,
            b']'  => TokenKind::RBracket,
            b','  => TokenKind::Comma,
            b';'  => TokenKind::Semicolon,
            b':'  => { if self.eat_if(b':') { TokenKind::ColonColon } else { TokenKind::Colon } }
            b'+'  => { if self.eat_if(b'=') { TokenKind::PlusEq  } else { TokenKind::Plus  } }
            b'*'  => { if self.eat_if(b'=') { TokenKind::StarEq  } else { TokenKind::Star  } }
            b'/'  => { if self.eat_if(b'=') { TokenKind::SlashEq } else { TokenKind::Slash } }
            b'%'  => TokenKind::Percent,
            b'-'  => { if self.eat_if(b'>') { TokenKind::Arrow }
                       else if self.eat_if(b'=') { TokenKind::MinusEq }
                       else { TokenKind::Minus } }
            b'='  => { if self.eat_if(b'=') { TokenKind::EqEq }
                       else if self.eat_if(b'>') { TokenKind::FatArrow }
                       else { TokenKind::Eq } }
            b'?'  => TokenKind::Question,
            b'!'  => { if self.eat_if(b'=') { TokenKind::BangEq }
                       else { return Err(LexError::new("expected '!='", line, col, &self.lines)); } }
            b'<'  => { if self.eat_if(b'=') { TokenKind::LtEq   }
                       else if self.eat_if(b'<') { TokenKind::ShiftL }
                       else { TokenKind::Lt } }
            b'>'  => { if self.eat_if(b'=') { TokenKind::GtEq   }
                       else if self.eat_if(b'>') { TokenKind::ShiftR }
                       else { TokenKind::Gt } }
            b'.'  => {
                if self.eat_if(b'.') {
                    if self.eat_if(b'.') { TokenKind::Ident("...".into()) }
                    else if self.eat_if(b'=') { TokenKind::DotDotEq }
                    else { TokenKind::DotDot }
                } else { TokenKind::Dot }
            }
            c if c.is_ascii_digit()                   => self.read_number(c)?,
            c if c.is_ascii_alphabetic() || c == b'_' => self.read_ident(c),
            other => return Err(LexError::new(
                &format!("unexpected character '{}'", other as char),
                line, col, &self.lines,
            )),
        };

        Ok(Token::new(kind, line, col))
    }
}

#[derive(Debug)]
pub struct LexError {
    pub msg:  String,
    pub line: usize,
    pub col:  usize,
    pub src_line: String,
}

impl LexError {
    pub fn new(msg: &str, line: usize, col: usize, lines: &[String]) -> Self {
        let src_line = lines.get(line.saturating_sub(1)).cloned().unwrap_or_default();
        LexError { msg: msg.to_string(), line, col, src_line }
    }
}

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.msg)
    }
}

impl std::error::Error for LexError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex(src: &str) -> Vec<TokenKind> {
        Lexer::new(src).tokenize().unwrap().into_iter()
            .map(|t| t.kind)
            .filter(|k| *k != TokenKind::Newline && *k != TokenKind::Eof)
            .collect()
    }

    #[test]
    fn basic_tokens() {
        assert_eq!(lex("let x = 42"), vec![TokenKind::Let, TokenKind::Ident("x".into()), TokenKind::Eq, TokenKind::Integer(42)]);
    }
    #[test]
    fn string_concat() {
        assert_eq!(lex(r#""hello" .. " world""#), vec![TokenKind::StringLit("hello".into()), TokenKind::DotDot, TokenKind::StringLit(" world".into())]);
    }
    #[test]
    fn function_decl() {
        assert_eq!(lex("fn add(a, b) do"), vec![TokenKind::Fn, TokenKind::Ident("add".into()), TokenKind::LParen, TokenKind::Ident("a".into()), TokenKind::Comma, TokenKind::Ident("b".into()), TokenKind::RParen, TokenKind::Do]);
    }
    #[test]
    fn comment_skipped() {
        let t = lex("let x = 1 -- ignored\nlet y = 2");
        assert_eq!(t, vec![TokenKind::Let, TokenKind::Ident("x".into()), TokenKind::Eq, TokenKind::Integer(1), TokenKind::Let, TokenKind::Ident("y".into()), TokenKind::Eq, TokenKind::Integer(2)]);
    }
    #[test]
    fn multiline_comment() {
        let t = lex("let x = 1 --[[ this\nis\na comment ]] let y = 2");
        assert_eq!(t, vec![TokenKind::Let, TokenKind::Ident("x".into()), TokenKind::Eq, TokenKind::Integer(1), TokenKind::Let, TokenKind::Ident("y".into()), TokenKind::Eq, TokenKind::Integer(2)]);
    }
    #[test]
    fn arrow_and_at() {
        let t = lex("@extern fn malloc(n: u64) -> ptr");
        assert!(t.contains(&TokenKind::At) && t.contains(&TokenKind::Arrow));
    }
    #[test]
    fn float_literal()   { assert_eq!(lex("3.14"), vec![TokenKind::Float(3.14)]); }
    #[test]
    fn bool_keywords()   { assert_eq!(lex("true false"), vec![TokenKind::Bool(true), TokenKind::Bool(false)]); }
    #[test]
    fn new_keywords()    { assert_eq!(lex("break continue struct"), vec![TokenKind::Break, TokenKind::Continue, TokenKind::Struct]); }
    #[test]
    fn hex_literal()     { assert_eq!(lex("0xFF"), vec![TokenKind::Integer(255)]); }
    #[test]
    fn compound_assign() { assert_eq!(lex("x += 1"), vec![TokenKind::Ident("x".into()), TokenKind::PlusEq, TokenKind::Integer(1)]); }
    #[test]
    fn range_ops() {
        assert_eq!(lex("0..10"),  vec![TokenKind::Integer(0), TokenKind::DotDot,    TokenKind::Integer(10)]);
        assert_eq!(lex("0..=10"), vec![TokenKind::Integer(0), TokenKind::DotDotEq,  TokenKind::Integer(10)]);
    }
    #[test]
    fn escaped_quote() {
        assert_eq!(lex(r#""say \"hi\"""#), vec![TokenKind::StringLit("say \"hi\"".into())]);
    }
}
