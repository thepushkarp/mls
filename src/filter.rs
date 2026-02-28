/// Filter expression parser and evaluator.
///
/// Supports a minimal expression language for filtering media entries:
///   `duration_ms > 60000`
///   `media.video.width >= 1920`
///   `media.audio.codec.name == "aac"`
///   `media.kind == "av" && duration_ms > 300000`
///
/// Operators: `== != > >= < <=` `&& || !` `()`
/// Field paths are dot-separated and resolved against `MediaEntry` JSON.
use crate::types::MediaEntry;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum FilterError {
    #[error("parse error at position {pos}: {msg}")]
    Parse { pos: usize, msg: String },
    #[error("evaluation error: {0}")]
    Eval(String),
}

/// A parsed filter expression ready for evaluation.
#[derive(Debug, Clone)]
pub struct Filter {
    expr: Expr,
}

#[derive(Debug, Clone)]
enum Expr {
    Compare {
        field: String,
        op: CmpOp,
        value: Value,
    },
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
}

#[derive(Debug, Clone, Copy)]
enum CmpOp {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
}

#[derive(Debug, Clone)]
enum Value {
    Str(String),
    Num(f64),
}

/// Tokenizer
#[derive(Debug, Clone)]
enum Token {
    Ident(String),
    Str(String),
    Num(f64),
    Op(CmpOp),
    And,
    Or,
    Not,
    LParen,
    RParen,
    Eof,
}

struct Lexer {
    chars: Vec<char>,
    pos: usize,
}

impl Lexer {
    fn new(input: &str) -> Self {
        Self {
            chars: input.chars().collect(),
            pos: 0,
        }
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.chars.len() && self.chars[self.pos].is_whitespace() {
            self.pos += 1;
        }
    }

    fn next_token(&mut self) -> Result<Token, FilterError> {
        self.skip_whitespace();

        if self.pos >= self.chars.len() {
            return Ok(Token::Eof);
        }

        let ch = self.chars[self.pos];

        // String literal
        if ch == '"' || ch == '\'' {
            return self.read_string(ch);
        }

        // Number
        if ch.is_ascii_digit() || (ch == '-' && self.peek_is_digit()) {
            return self.read_number();
        }

        // Operators
        if ch == '=' && self.peek_char() == Some('=') {
            self.pos += 2;
            return Ok(Token::Op(CmpOp::Eq));
        }
        if ch == '!' && self.peek_char() == Some('=') {
            self.pos += 2;
            return Ok(Token::Op(CmpOp::Ne));
        }
        if ch == '>' && self.peek_char() == Some('=') {
            self.pos += 2;
            return Ok(Token::Op(CmpOp::Ge));
        }
        if ch == '<' && self.peek_char() == Some('=') {
            self.pos += 2;
            return Ok(Token::Op(CmpOp::Le));
        }
        if ch == '>' {
            self.pos += 1;
            return Ok(Token::Op(CmpOp::Gt));
        }
        if ch == '<' {
            self.pos += 1;
            return Ok(Token::Op(CmpOp::Lt));
        }

        // Logical
        if ch == '&' && self.peek_char() == Some('&') {
            self.pos += 2;
            return Ok(Token::And);
        }
        if ch == '|' && self.peek_char() == Some('|') {
            self.pos += 2;
            return Ok(Token::Or);
        }
        if ch == '!' {
            self.pos += 1;
            return Ok(Token::Not);
        }

        // Parens
        if ch == '(' {
            self.pos += 1;
            return Ok(Token::LParen);
        }
        if ch == ')' {
            self.pos += 1;
            return Ok(Token::RParen);
        }

        // Identifier (field path)
        if ch.is_alphabetic() || ch == '_' {
            return self.read_ident();
        }

        Err(FilterError::Parse {
            pos: self.pos,
            msg: format!("unexpected character: '{ch}'"),
        })
    }

    fn peek_char(&self) -> Option<char> {
        self.chars.get(self.pos + 1).copied()
    }

    fn peek_is_digit(&self) -> bool {
        self.chars
            .get(self.pos + 1)
            .is_some_and(char::is_ascii_digit)
    }

    fn read_string(&mut self, quote: char) -> Result<Token, FilterError> {
        self.pos += 1; // skip opening quote
        let start = self.pos;
        while self.pos < self.chars.len() && self.chars[self.pos] != quote {
            self.pos += 1;
        }
        if self.pos >= self.chars.len() {
            return Err(FilterError::Parse {
                pos: start,
                msg: "unterminated string".to_string(),
            });
        }
        let s: String = self.chars[start..self.pos].iter().collect();
        self.pos += 1; // skip closing quote
        Ok(Token::Str(s))
    }

    fn read_number(&mut self) -> Result<Token, FilterError> {
        let start = self.pos;
        if self.chars[self.pos] == '-' {
            self.pos += 1;
        }
        while self.pos < self.chars.len() && (self.chars[self.pos].is_ascii_digit() || self.chars[self.pos] == '.') {
            self.pos += 1;
        }
        let s: String = self.chars[start..self.pos].iter().collect();
        let n = s.parse::<f64>().map_err(|_| FilterError::Parse {
            pos: start,
            msg: format!("invalid number: '{s}'"),
        })?;
        Ok(Token::Num(n))
    }

    #[expect(clippy::unnecessary_wraps)]
    fn read_ident(&mut self) -> Result<Token, FilterError> {
        let start = self.pos;
        while self.pos < self.chars.len()
            && (self.chars[self.pos].is_alphanumeric()
                || self.chars[self.pos] == '_'
                || self.chars[self.pos] == '.')
        {
            self.pos += 1;
        }
        let s: String = self.chars[start..self.pos].iter().collect();
        Ok(Token::Ident(s))
    }
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }

    fn advance(&mut self) -> Token {
        let tok = self.tokens.get(self.pos).cloned().unwrap_or(Token::Eof);
        self.pos += 1;
        tok
    }

    fn parse_expr(&mut self) -> Result<Expr, FilterError> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr, FilterError> {
        let mut left = self.parse_and()?;
        while matches!(self.peek(), Token::Or) {
            self.advance();
            let right = self.parse_and()?;
            left = Expr::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, FilterError> {
        let mut left = self.parse_unary()?;
        while matches!(self.peek(), Token::And) {
            self.advance();
            let right = self.parse_unary()?;
            left = Expr::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, FilterError> {
        if matches!(self.peek(), Token::Not) {
            self.advance();
            let expr = self.parse_primary()?;
            return Ok(Expr::Not(Box::new(expr)));
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Expr, FilterError> {
        if matches!(self.peek(), Token::LParen) {
            self.advance();
            let expr = self.parse_expr()?;
            if !matches!(self.peek(), Token::RParen) {
                return Err(FilterError::Parse {
                    pos: self.pos,
                    msg: "expected ')'".to_string(),
                });
            }
            self.advance();
            return Ok(expr);
        }

        // Must be: field op value
        let field = match self.advance() {
            Token::Ident(s) => s,
            other => {
                return Err(FilterError::Parse {
                    pos: self.pos,
                    msg: format!("expected field name, got {other:?}"),
                });
            }
        };

        let op = match self.advance() {
            Token::Op(op) => op,
            other => {
                return Err(FilterError::Parse {
                    pos: self.pos,
                    msg: format!("expected operator, got {other:?}"),
                });
            }
        };

        let value = match self.advance() {
            Token::Str(s) => Value::Str(s),
            Token::Num(n) => Value::Num(n),
            Token::Ident(s) => {
                // Treat bare identifiers as strings (e.g., `true`, `aac`)
                Value::Str(s)
            }
            other => {
                return Err(FilterError::Parse {
                    pos: self.pos,
                    msg: format!("expected value, got {other:?}"),
                });
            }
        };

        Ok(Expr::Compare { field, op, value })
    }
}

impl Filter {
    /// Parse a filter expression string.
    ///
    /// # Errors
    /// Returns a `FilterError` if the expression has syntax errors.
    pub fn parse(input: &str) -> Result<Self, FilterError> {
        let mut lexer = Lexer::new(input);
        let mut tokens = Vec::new();
        loop {
            let tok = lexer.next_token()?;
            if matches!(tok, Token::Eof) {
                break;
            }
            tokens.push(tok);
        }

        if tokens.is_empty() {
            return Err(FilterError::Parse {
                pos: 0,
                msg: "empty filter expression".to_string(),
            });
        }

        let mut parser = Parser::new(tokens);
        let expr = parser.parse_expr()?;
        Ok(Self { expr })
    }

    /// Evaluate the filter against a `MediaEntry`.
    ///
    /// Serializes the entry to JSON and resolves field paths against it.
    ///
    /// # Errors
    /// Returns a `FilterError` if field resolution or comparison fails.
    pub fn matches(&self, entry: &MediaEntry) -> Result<bool, FilterError> {
        let json = serde_json::to_value(entry).map_err(|e| {
            FilterError::Eval(format!("failed to serialize entry: {e}"))
        })?;
        eval_expr(&self.expr, &json)
    }
}

fn eval_expr(expr: &Expr, json: &serde_json::Value) -> Result<bool, FilterError> {
    match expr {
        Expr::Compare { field, op, value } => {
            let field_val = resolve_field(json, field);
            compare_values(&field_val, *op, value)
        }
        Expr::And(left, right) => {
            Ok(eval_expr(left, json)? && eval_expr(right, json)?)
        }
        Expr::Or(left, right) => {
            Ok(eval_expr(left, json)? || eval_expr(right, json)?)
        }
        Expr::Not(inner) => Ok(!eval_expr(inner, json)?),
    }
}

fn resolve_field(json: &serde_json::Value, path: &str) -> serde_json::Value {
    let mut current = json;
    for part in path.split('.') {
        match current.get(part) {
            Some(v) => current = v,
            None => return serde_json::Value::Null,
        }
    }
    current.clone()
}

fn compare_values(
    field: &serde_json::Value,
    op: CmpOp,
    value: &Value,
) -> Result<bool, FilterError> {
    if field.is_null() {
        return Ok(false);
    }

    match value {
        Value::Num(n) => {
            let field_num = field.as_f64().ok_or_else(|| {
                FilterError::Eval(format!("field value is not numeric: {field}"))
            })?;
            Ok(match op {
                CmpOp::Eq => (field_num - n).abs() < f64::EPSILON,
                CmpOp::Ne => (field_num - n).abs() >= f64::EPSILON,
                CmpOp::Gt => field_num > *n,
                CmpOp::Ge => field_num >= *n,
                CmpOp::Lt => field_num < *n,
                CmpOp::Le => field_num <= *n,
            })
        }
        Value::Str(s) => {
            let field_str = field
                .as_str().map_or_else(|| field.to_string().trim_matches('"').to_string(), String::from);
            Ok(match op {
                CmpOp::Eq => field_str == *s,
                CmpOp::Ne => field_str != *s,
                CmpOp::Gt => field_str.as_str() > s.as_str(),
                CmpOp::Ge => field_str.as_str() >= s.as_str(),
                CmpOp::Lt => (field_str.as_str()) < s.as_str(),
                CmpOp::Le => field_str.as_str() <= s.as_str(),
            })
        }
    }
}
