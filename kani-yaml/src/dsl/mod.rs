//! Tokenized Pratt parser for the declarative extraction expression language.

mod parseexpr;

use chumsky::prelude::SimpleSpan;
use kani_shared::ast::Op;
use std::ops::Range;

use self::parseexpr::ParseExpr;
pub use self::parseexpr::SpannedParseExpr;

pub(crate) const MAX_INPUT_BYTES: usize = 64 * 1024;
pub(crate) const MAX_TOKENS: usize = 16_384;
pub(crate) const MAX_NESTING: usize = 50;
pub(crate) const MAX_AST_NODES: usize = 10_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DslParseErrorKind {
    UnexpectedToken,
    UnexpectedEnd,
    UnterminatedString,
    UnterminatedComment,
    InvalidCharacter,
    LimitExceeded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DslParseError {
    pub kind: DslParseErrorKind,
    pub message: String,
    pub span: Range<usize>,
    pub help: Option<String>,
}

impl std::fmt::Display for DslParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

#[derive(Debug, Clone, PartialEq)]
enum TokenKind {
    Ident(String),
    Var(String),
    String(String),
    Number(f64),
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Comma,
    Dot,
    Colon,
    Semicolon,
    Eq,
    EqEq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Plus,
    Minus,
    Star,
    Slash,
    AndAnd,
    OrOr,
    Eof,
}

#[derive(Debug, Clone)]
struct Token {
    kind: TokenKind,
    span: Range<usize>,
}

pub fn parse(input: &str) -> Result<SpannedParseExpr, Vec<DslParseError>> {
    if input.len() > MAX_INPUT_BYTES {
        return Err(vec![limit_error(
            input.len(),
            MAX_INPUT_BYTES,
            "input bytes",
        )]);
    }
    let tokens = Lexer::new(input).tokenize()?;
    Parser::new(tokens).parse()
}

fn limit_error(actual: usize, limit: usize, what: &str) -> DslParseError {
    DslParseError {
        kind: DslParseErrorKind::LimitExceeded,
        span: 0..0,
        message: format!("DSL {what} limit of {limit} exceeded ({actual})"),
        help: Some("Split this expression into smaller fields or bindings.".into()),
    }
}

struct Lexer<'a> {
    input: &'a str,
    pos: usize,
    tokens: Vec<Token>,
}
impl<'a> Lexer<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input,
            pos: 0,
            tokens: Vec::new(),
        }
    }
    fn tokenize(mut self) -> Result<Vec<Token>, Vec<DslParseError>> {
        while self.pos < self.input.len() {
            self.skip_space_and_comments()?;
            if self.pos >= self.input.len() {
                break;
            }
            let start = self.pos;
            let ch = self.next_char().expect("in bounds");
            let kind = match ch {
                '(' => TokenKind::LParen,
                ')' => TokenKind::RParen,
                '[' => TokenKind::LBracket,
                ']' => TokenKind::RBracket,
                '{' => TokenKind::LBrace,
                '}' => TokenKind::RBrace,
                ',' => TokenKind::Comma,
                '.' => TokenKind::Dot,
                ':' => TokenKind::Colon,
                ';' => TokenKind::Semicolon,
                '+' => TokenKind::Plus,
                '-' => TokenKind::Minus,
                '*' => TokenKind::Star,
                '/' => TokenKind::Slash,
                '=' => {
                    if self.consume('=') {
                        TokenKind::EqEq
                    } else {
                        TokenKind::Eq
                    }
                }
                '!' => {
                    if self.consume('=') {
                        TokenKind::Ne
                    } else {
                        return Err(vec![self.error(
                            start..self.pos,
                            "expected '=' after '!'",
                            None,
                        )]);
                    }
                }
                '<' => {
                    if self.consume('=') {
                        TokenKind::Le
                    } else {
                        TokenKind::Lt
                    }
                }
                '>' => {
                    if self.consume('=') {
                        TokenKind::Ge
                    } else {
                        TokenKind::Gt
                    }
                }
                '&' => {
                    if self.consume('&') {
                        TokenKind::AndAnd
                    } else {
                        return Err(vec![self.error(
                            start..self.pos,
                            "expected '&' after '&'",
                            None,
                        )]);
                    }
                }
                '|' => {
                    if self.consume('|') {
                        TokenKind::OrOr
                    } else {
                        return Err(vec![self.error(
                            start..self.pos,
                            "expected '|' after '|'",
                            None,
                        )]);
                    }
                }
                '$' => TokenKind::Var(self.variable(start)?),
                '"' => TokenKind::String(self.string(start)?),
                c if is_ident_start(c) => TokenKind::Ident(self.ident(start)),
                c if c.is_ascii_digit() => TokenKind::Number(self.number(start)?),
                _ => {
                    return Err(vec![self.error_kind(
                        DslParseErrorKind::InvalidCharacter,
                        start..self.pos,
                        format!("invalid character '{ch}'"),
                        None,
                    )]);
                }
            };
            self.tokens.push(Token {
                kind,
                span: start..self.pos,
            });
            if self.tokens.len() > MAX_TOKENS {
                return Err(vec![limit_error(self.tokens.len(), MAX_TOKENS, "token")]);
            }
        }
        self.tokens.push(Token {
            kind: TokenKind::Eof,
            span: self.pos..self.pos,
        });
        Ok(self.tokens)
    }
    fn skip_space_and_comments(&mut self) -> Result<(), Vec<DslParseError>> {
        loop {
            while self.peek_char().is_some_and(char::is_whitespace) {
                self.next_char();
            }
            if self.remaining().starts_with("/*") {
                let start = self.pos;
                self.pos += 2;
                if let Some(end) = self.remaining().find("*/") {
                    self.pos += end + 2;
                } else {
                    return Err(vec![self.error_kind(
                        DslParseErrorKind::UnterminatedComment,
                        start..self.input.len(),
                        "unterminated block comment",
                        None,
                    )]);
                }
            } else {
                return Ok(());
            }
        }
    }
    fn string(&mut self, start: usize) -> Result<String, Vec<DslParseError>> {
        let mut out = String::new();
        loop {
            let Some(c) = self.next_char() else {
                return Err(vec![self.error_kind(
                    DslParseErrorKind::UnterminatedString,
                    start..self.pos,
                    "unterminated string literal",
                    None,
                )]);
            };
            match c {
                '"' => return Ok(out),
                '\\' => {
                    let Some(escaped) = self.next_char() else {
                        return Err(vec![self.error_kind(
                            DslParseErrorKind::UnterminatedString,
                            start..self.pos,
                            "unterminated string escape",
                            None,
                        )]);
                    };
                    match escaped {
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        'r' => out.push('\r'),
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        other => {
                            out.push('\\');
                            out.push(other);
                        }
                    }
                }
                other => out.push(other),
            }
        }
    }
    fn ident(&mut self, start: usize) -> String {
        while self.peek_char().is_some_and(is_ident_continue) {
            self.next_char();
        }
        self.input[start..self.pos].to_string()
    }
    fn variable(&mut self, start: usize) -> Result<String, Vec<DslParseError>> {
        let ident_start = self.pos;
        if !self.peek_char().is_some_and(is_ident_start) {
            return Err(vec![self.error(
                start..self.pos,
                "expected an identifier after '$'",
                None,
            )]);
        }
        self.next_char();
        while self.peek_char().is_some_and(is_ident_continue) {
            self.next_char();
        }
        Ok(format!("${}", &self.input[ident_start..self.pos]))
    }
    fn number(&mut self, start: usize) -> Result<f64, Vec<DslParseError>> {
        while self.peek_char().is_some_and(|c| c.is_ascii_digit()) {
            self.next_char();
        }
        if self.peek_char() == Some('.') {
            self.next_char();
            if !self.peek_char().is_some_and(|c| c.is_ascii_digit()) {
                return Err(vec![self.error(
                    start..self.pos,
                    "expected digits after decimal point",
                    None,
                )]);
            }
            while self.peek_char().is_some_and(|c| c.is_ascii_digit()) {
                self.next_char();
            }
        }
        self.input[start..self.pos]
            .parse()
            .map_err(|_| vec![self.error(start..self.pos, "invalid number", None)])
    }
    fn consume(&mut self, expected: char) -> bool {
        if self.peek_char() == Some(expected) {
            self.next_char();
            true
        } else {
            false
        }
    }
    fn remaining(&self) -> &'a str {
        &self.input[self.pos..]
    }
    fn peek_char(&self) -> Option<char> {
        self.remaining().chars().next()
    }
    fn next_char(&mut self) -> Option<char> {
        let c = self.peek_char()?;
        self.pos += c.len_utf8();
        Some(c)
    }
    fn error(
        &self,
        span: Range<usize>,
        message: impl Into<String>,
        help: Option<String>,
    ) -> DslParseError {
        self.error_kind(DslParseErrorKind::UnexpectedToken, span, message, help)
    }
    fn error_kind(
        &self,
        kind: DslParseErrorKind,
        span: Range<usize>,
        message: impl Into<String>,
        help: Option<String>,
    ) -> DslParseError {
        DslParseError {
            kind,
            message: message.into(),
            span,
            help,
        }
    }
}
fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}
fn is_ident_continue(c: char) -> bool {
    is_ident_start(c) || c.is_ascii_digit()
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    depth: usize,
    nodes: usize,
}
impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            depth: 0,
            nodes: 0,
        }
    }
    fn parse(mut self) -> Result<SpannedParseExpr, Vec<DslParseError>> {
        let start = self.peek().span.start;
        let expr = self.expr(0)?;
        if !matches!(self.peek().kind, TokenKind::Eof) {
            return Err(vec![self.unexpected("end of expression")]);
        }
        Ok(SpannedParseExpr(
            expr,
            SimpleSpan::from(start..self.peek().span.end),
        ))
    }
    fn expr(&mut self, min_bp: u8) -> Result<ParseExpr, Vec<DslParseError>> {
        self.enter()?;
        let result = self.expr_inner(min_bp);
        self.leave();
        result
    }
    fn expr_inner(&mut self, min_bp: u8) -> Result<ParseExpr, Vec<DslParseError>> {
        let mut lhs = self.prefix()?;
        while let Some((op, lbp, rbp)) = self.infix() {
            if lbp < min_bp {
                break;
            }
            self.advance();
            let rhs = self.expr(rbp)?;
            lhs = self.node(ParseExpr::BinaryOperation {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            })?;
        }
        Ok(lhs)
    }
    fn prefix(&mut self) -> Result<ParseExpr, Vec<DslParseError>> {
        let token = self.advance();
        let mut expr = match token.kind {
            TokenKind::String(s) => ParseExpr::Literal(s),
            TokenKind::Number(n) => ParseExpr::Number(n),
            TokenKind::Minus => match self.advance().kind {
                TokenKind::Number(n) => ParseExpr::Number(-n),
                _ => return Err(vec![self.unexpected("a number after '-'")]),
            },
            TokenKind::Var(name) => ParseExpr::Var(name),
            TokenKind::Ident(name) => match name.as_str() {
                "self" => ParseExpr::SelfRef,
                "true" => ParseExpr::Bool(true),
                "false" => ParseExpr::Bool(false),
                "null" => ParseExpr::Null,
                "let" => self.let_expr()?,
                "if" => self.if_expr()?,
                "merge" => self.merge_expr()?,
                "format" => self.format_expr()?,
                "dom" | "json" | "pref" | "scalar" => self.builtin_string(&name)?,
                "index" => {
                    self.expect(TokenKind::LParen, "'(' after index")?;
                    self.expect(TokenKind::RParen, "')' after index(")?;
                    ParseExpr::Index
                }
                _ => {
                    return Err(vec![self.error(
                        token.span,
                        format!("unexpected identifier '{name}'"),
                        None,
                    )]);
                }
            },
            TokenKind::LBracket => self.list_expr()?,
            TokenKind::LBrace => self.map_expr()?,
            TokenKind::LParen => {
                let value = self.expr(0)?;
                self.expect(TokenKind::RParen, "')' to close grouping")?;
                value
            }
            _ => return Err(vec![self.error(token.span, "expected an expression", None)]),
        };
        while matches!(self.peek().kind, TokenKind::Dot) {
            self.advance();
            expr = self.method(expr)?;
        }
        self.node(expr)
    }
    fn let_expr(&mut self) -> Result<ParseExpr, Vec<DslParseError>> {
        let name = match self.advance().kind {
            TokenKind::Var(v) => v,
            _ => return Err(vec![self.unexpected("a variable after let")]),
        };
        self.expect(TokenKind::Eq, "'=' after let variable")?;
        let value = self.expr(0)?;
        self.expect(TokenKind::Semicolon, "';' after let binding")?;
        let body = self.expr(0)?;
        self.node(ParseExpr::Let {
            name,
            value: Box::new(value),
            body: Box::new(body),
        })
    }
    fn if_expr(&mut self) -> Result<ParseExpr, Vec<DslParseError>> {
        let condition = self.expr(0)?;
        self.keyword("then")?;
        let then = self.expr(0)?;
        self.keyword("else")?;
        let else_ = self.expr(0)?;
        self.node(ParseExpr::If {
            condition: Box::new(condition),
            then: Box::new(then),
            else_: Box::new(else_),
        })
    }
    fn merge_expr(&mut self) -> Result<ParseExpr, Vec<DslParseError>> {
        self.expect(TokenKind::LParen, "'(' after merge")?;
        self.expect(TokenKind::LBracket, "'[' as merge argument")?;
        let values = self.expr_list(TokenKind::RBracket)?;
        self.expect(TokenKind::RParen, "')' after merge list")?;
        self.node(ParseExpr::Merge(values))
    }
    fn format_expr(&mut self) -> Result<ParseExpr, Vec<DslParseError>> {
        self.expect(TokenKind::LParen, "'(' after format")?;
        let template = match self.advance().kind {
            TokenKind::String(s) => s,
            _ => {
                return Err(vec![
                    self.unexpected("a string template as first format argument"),
                ]);
            }
        };
        let mut args = Vec::new();
        if matches!(self.peek().kind, TokenKind::Comma) {
            self.advance();
            args = self.expr_list(TokenKind::RParen)?;
        } else {
            self.expect(TokenKind::RParen, "')' after format template")?;
        }
        self.node(ParseExpr::Format { template, args })
    }
    fn builtin_string(&mut self, name: &str) -> Result<ParseExpr, Vec<DslParseError>> {
        self.expect(TokenKind::LParen, format!("'(' after {name}"))?;
        let value = match self.advance().kind {
            TokenKind::String(s) => s,
            _ => return Err(vec![self.unexpected("a string argument")]),
        };
        self.expect(TokenKind::RParen, "')' after string argument")?;
        Ok(match name {
            "dom" => ParseExpr::Dom(value),
            "json" => ParseExpr::Json(value),
            "scalar" => ParseExpr::Scalar(value),
            _ => ParseExpr::Pref(value),
        })
    }
    fn list_expr(&mut self) -> Result<ParseExpr, Vec<DslParseError>> {
        let values = self.expr_list(TokenKind::RBracket)?;
        self.node(ParseExpr::List(values))
    }
    fn map_expr(&mut self) -> Result<ParseExpr, Vec<DslParseError>> {
        let mut entries = Vec::new();
        if matches!(self.peek().kind, TokenKind::RBrace) {
            self.advance();
            return self.node(ParseExpr::MapLiteral(entries));
        }
        loop {
            let key = match self.advance().kind {
                TokenKind::String(s) => s,
                _ => return Err(vec![self.unexpected("a string map key")]),
            };
            self.expect(TokenKind::Colon, "':' after map key")?;
            let value = match self.advance().kind {
                TokenKind::String(s) => s,
                _ => return Err(vec![self.unexpected("a string map value")]),
            };
            entries.push((key, value));
            if matches!(self.peek().kind, TokenKind::Comma) {
                self.advance();
                if matches!(self.peek().kind, TokenKind::RBrace) {
                    self.advance();
                    break;
                }
            } else {
                self.expect(TokenKind::RBrace, "'}' after map entry")?;
                break;
            }
        }
        self.node(ParseExpr::MapLiteral(entries))
    }
    fn method(&mut self, target: ParseExpr) -> Result<ParseExpr, Vec<DslParseError>> {
        let start = self.peek().span.start;
        let name = match self.advance().kind {
            TokenKind::Ident(v) => v,
            _ => return Err(vec![self.unexpected("a method name after '.'")]),
        };
        let name = if name == "user" {
            self.expect(TokenKind::Dot, "'.' after user")?;
            let function = match self.advance().kind {
                TokenKind::Ident(v) => v,
                _ => return Err(vec![self.unexpected("a user function name")]),
            };
            format!("__user::{function}")
        } else {
            name
        };
        self.expect(TokenKind::LParen, "'(' after method name")?;
        let args = self.expr_list(TokenKind::RParen)?;
        self.node(ParseExpr::MethodCall {
            target: Box::new(target),
            name,
            args,
            span: SimpleSpan::from(start..self.peek().span.start),
        })
    }
    fn expr_list(&mut self, close: TokenKind) -> Result<Vec<ParseExpr>, Vec<DslParseError>> {
        let mut values = Vec::new();
        if same_kind(&self.peek().kind, &close) {
            self.advance();
            return Ok(values);
        }
        loop {
            values.push(self.expr(0)?);
            if matches!(self.peek().kind, TokenKind::Comma) {
                self.advance();
                if same_kind(&self.peek().kind, &close) {
                    self.advance();
                    break;
                }
            } else {
                self.expect(close, "a closing delimiter")?;
                break;
            }
        }
        Ok(values)
    }
    fn infix(&self) -> Option<(Op, u8, u8)> {
        match self.peek().kind {
            TokenKind::OrOr => Some((Op::Or, 1, 2)),
            TokenKind::AndAnd => Some((Op::And, 3, 4)),
            TokenKind::EqEq => Some((Op::Eq, 5, 6)),
            TokenKind::Ne => Some((Op::Ne, 5, 6)),
            TokenKind::Lt => Some((Op::Lt, 7, 8)),
            TokenKind::Gt => Some((Op::Gt, 7, 8)),
            TokenKind::Le => Some((Op::Le, 7, 8)),
            TokenKind::Ge => Some((Op::Ge, 7, 8)),
            TokenKind::Plus => Some((Op::Add, 9, 10)),
            TokenKind::Minus => Some((Op::Sub, 9, 10)),
            TokenKind::Star => Some((Op::Mul, 11, 12)),
            TokenKind::Slash => Some((Op::Div, 11, 12)),
            _ => None,
        }
    }
    fn keyword(&mut self, expected: &str) -> Result<(), Vec<DslParseError>> {
        match &self.peek().kind {
            TokenKind::Ident(s) if s == expected => {
                self.advance();
                Ok(())
            }
            _ => Err(vec![self.unexpected(format!("'{expected}'"))]),
        }
    }
    fn expect(
        &mut self,
        expected: TokenKind,
        message: impl Into<String>,
    ) -> Result<(), Vec<DslParseError>> {
        if same_kind(&self.peek().kind, &expected) {
            self.advance();
            Ok(())
        } else {
            Err(vec![self.error(self.peek().span.clone(), message, None)])
        }
    }
    fn enter(&mut self) -> Result<(), Vec<DslParseError>> {
        self.depth += 1;
        if self.depth > MAX_NESTING {
            return Err(vec![limit_error(self.depth, MAX_NESTING, "nesting")]);
        }
        Ok(())
    }
    fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }
    fn node(&mut self, value: ParseExpr) -> Result<ParseExpr, Vec<DslParseError>> {
        self.nodes += 1;
        if self.nodes > MAX_AST_NODES {
            Err(vec![limit_error(self.nodes, MAX_AST_NODES, "AST node")])
        } else {
            Ok(value)
        }
    }
    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }
    fn advance(&mut self) -> Token {
        let token = self.tokens[self.pos].clone();
        if !matches!(token.kind, TokenKind::Eof) {
            self.pos += 1;
        }
        token
    }
    fn unexpected(&self, expected: impl Into<String>) -> DslParseError {
        let found = match &self.peek().kind {
            TokenKind::Eof => "end of input".to_string(),
            other => format!("{other:?}"),
        };
        self.error(
            self.peek().span.clone(),
            format!("expected {}, found {found}", expected.into()),
            None,
        )
    }
    fn error(
        &self,
        span: Range<usize>,
        message: impl Into<String>,
        help: Option<String>,
    ) -> DslParseError {
        DslParseError {
            kind: if matches!(self.peek().kind, TokenKind::Eof) {
                DslParseErrorKind::UnexpectedEnd
            } else {
                DslParseErrorKind::UnexpectedToken
            },
            message: message.into(),
            span,
            help,
        }
    }
}
fn same_kind(a: &TokenKind, b: &TokenKind) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b)
}
