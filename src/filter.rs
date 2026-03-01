/// Filter expression parser and evaluator.
///
/// Supports a minimal expression language for filtering media entries:
///   `media.duration_ms > 60000`
///   `media.video.width >= 1920`
///   `media.audio.codec.name == "aac"`
///   `media.kind == "av" && media.duration_ms > 300000`
///
/// Common fields have shorthand aliases (e.g. `duration_ms` → `media.duration_ms`).
///
/// Operators: `== != > >= < <=` `&& || !` `()`
/// Field paths are dot-separated and resolved via typed access on `MediaEntry`.
use crate::types::MediaEntry;
use std::borrow::Cow;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum FilterError {
    #[error("parse error at token {pos}: {msg}")]
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
        let mut has_dot = false;
        while self.pos < self.chars.len() {
            let ch = self.chars[self.pos];
            if ch.is_ascii_digit() {
                self.pos += 1;
            } else if ch == '.' && !has_dot {
                has_dot = true;
                self.pos += 1;
            } else {
                break;
            }
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
            let expr = self.parse_unary()?;
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

        if parser.pos < parser.tokens.len() {
            return Err(FilterError::Parse {
                pos: parser.pos,
                msg: format!("unexpected trailing token: {:?}", parser.tokens[parser.pos]),
            });
        }

        Ok(Self { expr })
    }

    /// Evaluate the filter against a `MediaEntry`.
    ///
    /// Uses typed field access — no JSON serialization.
    ///
    /// # Errors
    /// Returns a `FilterError` if field resolution or comparison fails.
    pub fn matches(&self, entry: &MediaEntry) -> Result<bool, FilterError> {
        eval_expr(&self.expr, entry)
    }
}

/// Lightweight value type for typed field resolution (avoids `serde_json::Value`).
enum FieldValue<'a> {
    Num(f64),
    Str(Cow<'a, str>),
    Null,
}

fn eval_expr(expr: &Expr, entry: &MediaEntry) -> Result<bool, FilterError> {
    match expr {
        Expr::Compare { field, op, value } => {
            let field_val = resolve_field_typed(entry, field);
            compare_values(&field_val, *op, value)
        }
        Expr::And(left, right) => Ok(eval_expr(left, entry)? && eval_expr(right, entry)?),
        Expr::Or(left, right) => Ok(eval_expr(left, entry)? || eval_expr(right, entry)?),
        Expr::Not(inner) => Ok(!eval_expr(inner, entry)?),
    }
}

/// Resolve a dot-separated field path to a typed value via direct struct access.
///
/// Covers all `MediaEntry` fields that filters can reference.
#[expect(clippy::too_many_lines, clippy::cast_precision_loss)]
fn resolve_field_typed<'a>(entry: &'a MediaEntry, path: &str) -> FieldValue<'a> {
    match path {
        // Top-level fields
        "path" => FieldValue::Str(Cow::Borrowed(entry.path.to_str().unwrap_or(""))),
        "file_name" => FieldValue::Str(Cow::Borrowed(&entry.file_name)),
        "extension" => FieldValue::Str(Cow::Borrowed(&entry.extension)),

        // fs.*
        "fs.size_bytes" => FieldValue::Num(entry.fs.size_bytes as f64),
        "fs.modified_at" => match entry.fs.modified_at {
            Some(dt) => FieldValue::Str(Cow::Owned(dt.to_rfc3339())),
            None => FieldValue::Null,
        },
        "fs.created_at" => match entry.fs.created_at {
            Some(dt) => FieldValue::Str(Cow::Owned(dt.to_rfc3339())),
            None => FieldValue::Null,
        },

        // media.*
        "media.kind" => FieldValue::Str(Cow::Owned(entry.media.kind.to_string())),
        "media.duration_ms" => match entry.media.duration_ms {
            Some(d) => FieldValue::Num(d as f64),
            None => FieldValue::Null,
        },
        "media.overall_bitrate_bps" => match entry.media.overall_bitrate_bps {
            Some(b) => FieldValue::Num(b as f64),
            None => FieldValue::Null,
        },

        // media.container.*
        "media.container.format_name" => {
            FieldValue::Str(Cow::Borrowed(&entry.media.container.format_name))
        }
        "media.container.format_primary" => {
            FieldValue::Str(Cow::Borrowed(&entry.media.container.format_primary))
        }

        // media.video.*
        "media.video.width" => match entry.media.video {
            Some(ref v) => FieldValue::Num(f64::from(v.width)),
            None => FieldValue::Null,
        },
        "media.video.height" => match entry.media.video {
            Some(ref v) => FieldValue::Num(f64::from(v.height)),
            None => FieldValue::Null,
        },
        "media.video.bitrate_bps" => match entry.media.video {
            Some(ref v) => match v.bitrate_bps {
                Some(b) => FieldValue::Num(b as f64),
                None => FieldValue::Null,
            },
            None => FieldValue::Null,
        },
        "media.video.pixel_format" => match entry.media.video {
            Some(ref v) => match v.pixel_format {
                Some(ref pf) => FieldValue::Str(Cow::Borrowed(pf)),
                None => FieldValue::Null,
            },
            None => FieldValue::Null,
        },
        "media.video.codec.name" => match entry.media.video {
            Some(ref v) => FieldValue::Str(Cow::Borrowed(&v.codec.name)),
            None => FieldValue::Null,
        },
        "media.video.codec.profile" => match entry.media.video {
            Some(ref v) => match v.codec.profile {
                Some(ref p) => FieldValue::Str(Cow::Borrowed(p)),
                None => FieldValue::Null,
            },
            None => FieldValue::Null,
        },
        "media.video.codec.level" => match entry.media.video {
            Some(ref v) => match v.codec.level {
                Some(ref l) => FieldValue::Str(Cow::Borrowed(l)),
                None => FieldValue::Null,
            },
            None => FieldValue::Null,
        },
        "media.video.fps.num" => match entry.media.video {
            Some(ref v) => match v.fps {
                Some(fps) => FieldValue::Num(f64::from(fps.num)),
                None => FieldValue::Null,
            },
            None => FieldValue::Null,
        },
        "media.video.fps.den" => match entry.media.video {
            Some(ref v) => match v.fps {
                Some(fps) => FieldValue::Num(f64::from(fps.den)),
                None => FieldValue::Null,
            },
            None => FieldValue::Null,
        },

        // media.audio.*
        "media.audio.channels" => match entry.media.audio {
            Some(ref a) => FieldValue::Num(f64::from(a.channels)),
            None => FieldValue::Null,
        },
        "media.audio.channel_layout" => match entry.media.audio {
            Some(ref a) => match a.channel_layout {
                Some(ref cl) => FieldValue::Str(Cow::Borrowed(cl)),
                None => FieldValue::Null,
            },
            None => FieldValue::Null,
        },
        "media.audio.sample_rate_hz" => match entry.media.audio {
            Some(ref a) => match a.sample_rate_hz {
                Some(sr) => FieldValue::Num(f64::from(sr)),
                None => FieldValue::Null,
            },
            None => FieldValue::Null,
        },
        "media.audio.bitrate_bps" => match entry.media.audio {
            Some(ref a) => match a.bitrate_bps {
                Some(b) => FieldValue::Num(b as f64),
                None => FieldValue::Null,
            },
            None => FieldValue::Null,
        },
        "media.audio.codec.name" => match entry.media.audio {
            Some(ref a) => FieldValue::Str(Cow::Borrowed(&a.codec.name)),
            None => FieldValue::Null,
        },
        "media.audio.codec.profile" => match entry.media.audio {
            Some(ref a) => match a.codec.profile {
                Some(ref p) => FieldValue::Str(Cow::Borrowed(p)),
                None => FieldValue::Null,
            },
            None => FieldValue::Null,
        },

        // media.tags.*
        "media.tags.title" => match entry.media.tags.title {
            Some(ref t) => FieldValue::Str(Cow::Borrowed(t)),
            None => FieldValue::Null,
        },
        "media.tags.artist" => match entry.media.tags.artist {
            Some(ref a) => FieldValue::Str(Cow::Borrowed(a)),
            None => FieldValue::Null,
        },
        "media.tags.album" => match entry.media.tags.album {
            Some(ref a) => FieldValue::Str(Cow::Borrowed(a)),
            None => FieldValue::Null,
        },
        "media.tags.date" => match entry.media.tags.date {
            Some(ref d) => FieldValue::Str(Cow::Borrowed(d)),
            None => FieldValue::Null,
        },
        "media.tags.genre" => match entry.media.tags.genre {
            Some(ref g) => FieldValue::Str(Cow::Borrowed(g)),
            None => FieldValue::Null,
        },

        // probe.*
        "probe.backend" => FieldValue::Str(Cow::Borrowed(&entry.probe.backend)),
        "probe.took_ms" => FieldValue::Num(entry.probe.took_ms as f64),

        // Convenience aliases (top-level shortcuts for common fields)
        "duration_ms" => resolve_field_typed(entry, "media.duration_ms"),
        "size_bytes" => resolve_field_typed(entry, "fs.size_bytes"),
        "kind" => resolve_field_typed(entry, "media.kind"),
        "width" => resolve_field_typed(entry, "media.video.width"),
        "height" => resolve_field_typed(entry, "media.video.height"),
        "bitrate_bps" | "bitrate" => resolve_field_typed(entry, "media.overall_bitrate_bps"),

        // Unknown field
        _ => {
            tracing::debug!(field = path, "unknown filter field path, treating as null");
            FieldValue::Null
        }
    }
}

// All numeric fields originate from integers (u64/u32/i64) and filter literals
// are parsed from integer tokens — direct f64 equality is exact for ≤ 2^53.
#[expect(clippy::float_cmp)]
fn compare_values(field: &FieldValue<'_>, op: CmpOp, value: &Value) -> Result<bool, FilterError> {
    if matches!(field, FieldValue::Null) {
        return Ok(false);
    }

    match (field, value) {
        (FieldValue::Num(field_num), Value::Num(n)) => Ok(match op {
            CmpOp::Eq => *field_num == *n,
            CmpOp::Ne => *field_num != *n,
            CmpOp::Gt => *field_num > *n,
            CmpOp::Ge => *field_num >= *n,
            CmpOp::Lt => *field_num < *n,
            CmpOp::Le => *field_num <= *n,
        }),
        (FieldValue::Str(field_str), Value::Str(s)) => Ok(match op {
            CmpOp::Eq => field_str.as_ref() == s,
            CmpOp::Ne => field_str.as_ref() != s,
            CmpOp::Gt => field_str.as_ref() > s.as_str(),
            CmpOp::Ge => field_str.as_ref() >= s.as_str(),
            CmpOp::Lt => field_str.as_ref() < s.as_str(),
            CmpOp::Le => field_str.as_ref() <= s.as_str(),
        }),
        (FieldValue::Num(n), Value::Str(s)) => {
            // Try numeric comparison first so `width > "9"` compares 1920 > 9
            // instead of lexicographic "1920" < "9".
            if let Ok(s_num) = s.parse::<f64>() {
                Ok(match op {
                    CmpOp::Eq => *n == s_num,
                    CmpOp::Ne => *n != s_num,
                    CmpOp::Gt => *n > s_num,
                    CmpOp::Ge => *n >= s_num,
                    CmpOp::Lt => *n < s_num,
                    CmpOp::Le => *n <= s_num,
                })
            } else {
                let field_str = n.to_string();
                Ok(match op {
                    CmpOp::Eq => field_str == *s,
                    CmpOp::Ne => field_str != *s,
                    CmpOp::Gt => field_str.as_str() > s.as_str(),
                    CmpOp::Ge => field_str.as_str() >= s.as_str(),
                    CmpOp::Lt => field_str.as_str() < s.as_str(),
                    CmpOp::Le => field_str.as_str() <= s.as_str(),
                })
            }
        }
        (FieldValue::Str(field_str), Value::Num(n)) => {
            if let Ok(field_num) = field_str.parse::<f64>() {
                Ok(match op {
                    CmpOp::Eq => field_num == *n,
                    CmpOp::Ne => field_num != *n,
                    CmpOp::Gt => field_num > *n,
                    CmpOp::Ge => field_num >= *n,
                    CmpOp::Lt => field_num < *n,
                    CmpOp::Le => field_num <= *n,
                })
            } else {
                Err(FilterError::Eval(format!(
                    "cannot compare string '{field_str}' with number",
                )))
            }
        }
        (FieldValue::Null, _) => Ok(false),
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::types::{
        AudioInfo, CodecInfo, ContainerInfo, Fps, FsInfo, MediaEntry, MediaInfo, MediaKind,
        MediaTags, ProbeInfo, VideoInfo,
    };
    use std::borrow::Cow;
    use std::path::PathBuf;

    fn make_entry() -> MediaEntry {
        MediaEntry {
            path: PathBuf::from("/test/video.mp4"),
            file_name: "video.mp4".to_string(),
            extension: "mp4".to_string(),
            fs: FsInfo {
                size_bytes: 1_000_000,
                modified_at: None,
                created_at: None,
            },
            media: MediaInfo {
                kind: MediaKind::Av,
                container: ContainerInfo {
                    format_name: "mov,mp4,m4a,3gp,3g2,mj2".to_string(),
                    format_primary: "mov".to_string(),
                },
                duration_ms: Some(120_000),
                overall_bitrate_bps: Some(5_000_000),
                video: Some(VideoInfo {
                    width: 1920,
                    height: 1080,
                    fps: Some(Fps { num: 24, den: 1 }),
                    bitrate_bps: Some(4_500_000),
                    codec: CodecInfo {
                        name: "h264".to_string(),
                        profile: Some("High".to_string()),
                        level: Some("41".to_string()),
                    },
                    pixel_format: Some("yuv420p".to_string()),
                }),
                audio: Some(AudioInfo {
                    channels: 2,
                    channel_layout: Some("stereo".to_string()),
                    sample_rate_hz: Some(48000),
                    bitrate_bps: Some(128_000),
                    codec: CodecInfo {
                        name: "aac".to_string(),
                        profile: Some("LC".to_string()),
                        level: None,
                    },
                }),
                streams: vec![],
                tags: MediaTags::default(),
            },
            probe: ProbeInfo {
                backend: Cow::Borrowed("ffprobe"),
                took_ms: 50,
                error: None,
            },
        }
    }

    // --- Lexer tests ---

    #[test]
    fn lex_simple_comparison() {
        let mut lexer = Lexer::new("field == 42");
        assert!(matches!(lexer.next_token().unwrap(), Token::Ident(s) if s == "field"));
        assert!(matches!(lexer.next_token().unwrap(), Token::Op(CmpOp::Eq)));
        assert!(
            matches!(lexer.next_token().unwrap(), Token::Num(n) if (n - 42.0).abs() < f64::EPSILON)
        );
        assert!(matches!(lexer.next_token().unwrap(), Token::Eof));
    }

    #[test]
    fn lex_string_double_quotes() {
        let mut lexer = Lexer::new("\"hello\"");
        assert!(matches!(lexer.next_token().unwrap(), Token::Str(s) if s == "hello"));
    }

    #[test]
    fn lex_string_single_quotes() {
        let mut lexer = Lexer::new("'world'");
        assert!(matches!(lexer.next_token().unwrap(), Token::Str(s) if s == "world"));
    }

    #[test]
    fn lex_all_comparison_operators() {
        let mut lexer = Lexer::new("== != > >= < <=");
        assert!(matches!(lexer.next_token().unwrap(), Token::Op(CmpOp::Eq)));
        assert!(matches!(lexer.next_token().unwrap(), Token::Op(CmpOp::Ne)));
        assert!(matches!(lexer.next_token().unwrap(), Token::Op(CmpOp::Gt)));
        assert!(matches!(lexer.next_token().unwrap(), Token::Op(CmpOp::Ge)));
        assert!(matches!(lexer.next_token().unwrap(), Token::Op(CmpOp::Lt)));
        assert!(matches!(lexer.next_token().unwrap(), Token::Op(CmpOp::Le)));
        assert!(matches!(lexer.next_token().unwrap(), Token::Eof));
    }

    #[test]
    fn lex_logical_operators() {
        let mut lexer = Lexer::new("&& || !");
        assert!(matches!(lexer.next_token().unwrap(), Token::And));
        assert!(matches!(lexer.next_token().unwrap(), Token::Or));
        assert!(matches!(lexer.next_token().unwrap(), Token::Not));
    }

    #[test]
    fn lex_parens() {
        let mut lexer = Lexer::new("()");
        assert!(matches!(lexer.next_token().unwrap(), Token::LParen));
        assert!(matches!(lexer.next_token().unwrap(), Token::RParen));
    }

    #[test]
    fn lex_dotted_field_path() {
        let mut lexer = Lexer::new("media.video.width");
        assert!(matches!(lexer.next_token().unwrap(), Token::Ident(s) if s == "media.video.width"));
    }

    #[test]
    fn lex_negative_number() {
        let mut lexer = Lexer::new("-42");
        assert!(
            matches!(lexer.next_token().unwrap(), Token::Num(n) if (n - (-42.0)).abs() < f64::EPSILON)
        );
    }

    #[test]
    fn lex_decimal_number() {
        let mut lexer = Lexer::new("2.75");
        assert!(matches!(lexer.next_token().unwrap(), Token::Num(n) if (n - 2.75).abs() < 0.001));
    }

    #[test]
    fn lex_unterminated_string_error() {
        let mut lexer = Lexer::new("\"hello");
        assert!(lexer.next_token().is_err());
    }

    #[test]
    fn lex_unexpected_char_error() {
        let mut lexer = Lexer::new("@");
        assert!(lexer.next_token().is_err());
    }

    #[test]
    fn lex_whitespace_skipped() {
        let mut lexer = Lexer::new("   42   ");
        assert!(
            matches!(lexer.next_token().unwrap(), Token::Num(n) if (n - 42.0).abs() < f64::EPSILON)
        );
        assert!(matches!(lexer.next_token().unwrap(), Token::Eof));
    }

    // --- Parser / Filter::parse tests ---

    #[test]
    fn parse_empty_expression() {
        assert!(Filter::parse("").is_err());
    }

    #[test]
    fn parse_whitespace_only() {
        assert!(Filter::parse("   ").is_err());
    }

    #[test]
    fn parse_simple_num_comparison() {
        assert!(Filter::parse("duration_ms > 60000").is_ok());
    }

    #[test]
    fn parse_simple_str_comparison() {
        assert!(Filter::parse("media.audio.codec.name == \"aac\"").is_ok());
    }

    #[test]
    fn parse_and_expression() {
        assert!(Filter::parse("duration_ms > 60000 && fs.size_bytes < 5000000").is_ok());
    }

    #[test]
    fn parse_or_expression() {
        assert!(Filter::parse("extension == \"mp4\" || extension == \"mkv\"").is_ok());
    }

    #[test]
    fn parse_not_expression() {
        assert!(Filter::parse("!extension == \"avi\"").is_ok());
    }

    #[test]
    fn parse_nested_parens() {
        assert!(
            Filter::parse("(duration_ms > 60000 || fs.size_bytes > 1000) && extension == \"mp4\"")
                .is_ok()
        );
    }

    #[test]
    fn parse_deeply_nested() {
        assert!(Filter::parse("((a > 1))").is_ok());
    }

    #[test]
    fn parse_missing_rparen_error() {
        assert!(Filter::parse("(duration_ms > 60000").is_err());
    }

    #[test]
    fn parse_missing_operator_error() {
        assert!(Filter::parse("field 42").is_err());
    }

    #[test]
    fn parse_missing_value_error() {
        assert!(Filter::parse("field ==").is_err());
    }

    // --- Evaluation tests ---

    #[test]
    fn eval_numeric_gt_true() {
        let entry = make_entry();
        let f = Filter::parse("media.duration_ms > 60000").unwrap();
        assert!(f.matches(&entry).unwrap());
    }

    #[test]
    fn eval_numeric_gt_false() {
        let entry = make_entry();
        let f = Filter::parse("media.duration_ms > 200000").unwrap();
        assert!(!f.matches(&entry).unwrap());
    }

    #[test]
    fn eval_numeric_eq() {
        let entry = make_entry();
        let f = Filter::parse("media.duration_ms == 120000").unwrap();
        assert!(f.matches(&entry).unwrap());
    }

    #[test]
    fn eval_numeric_ne() {
        let entry = make_entry();
        let f = Filter::parse("media.duration_ms != 60000").unwrap();
        assert!(f.matches(&entry).unwrap());
    }

    #[test]
    fn eval_numeric_ge() {
        let entry = make_entry();
        let f = Filter::parse("media.duration_ms >= 120000").unwrap();
        assert!(f.matches(&entry).unwrap());
    }

    #[test]
    fn eval_numeric_lt() {
        let entry = make_entry();
        let f = Filter::parse("media.duration_ms < 200000").unwrap();
        assert!(f.matches(&entry).unwrap());
    }

    #[test]
    fn eval_numeric_le() {
        let entry = make_entry();
        let f = Filter::parse("media.duration_ms <= 120000").unwrap();
        assert!(f.matches(&entry).unwrap());
    }

    #[test]
    fn eval_string_eq() {
        let entry = make_entry();
        let f = Filter::parse("extension == \"mp4\"").unwrap();
        assert!(f.matches(&entry).unwrap());
    }

    #[test]
    fn eval_string_ne() {
        let entry = make_entry();
        let f = Filter::parse("extension != \"avi\"").unwrap();
        assert!(f.matches(&entry).unwrap());
    }

    #[test]
    fn eval_nested_field_path() {
        let entry = make_entry();
        let f = Filter::parse("media.video.width >= 1920").unwrap();
        assert!(f.matches(&entry).unwrap());
    }

    #[test]
    fn eval_deep_nested_codec() {
        let entry = make_entry();
        let f = Filter::parse("media.video.codec.name == \"h264\"").unwrap();
        assert!(f.matches(&entry).unwrap());
    }

    #[test]
    fn eval_and_both_true() {
        let entry = make_entry();
        let f = Filter::parse("media.duration_ms > 60000 && extension == \"mp4\"").unwrap();
        assert!(f.matches(&entry).unwrap());
    }

    #[test]
    fn eval_and_one_false() {
        let entry = make_entry();
        let f = Filter::parse("media.duration_ms > 60000 && extension == \"avi\"").unwrap();
        assert!(!f.matches(&entry).unwrap());
    }

    #[test]
    fn eval_or_one_true() {
        let entry = make_entry();
        let f = Filter::parse("extension == \"avi\" || extension == \"mp4\"").unwrap();
        assert!(f.matches(&entry).unwrap());
    }

    #[test]
    fn eval_or_both_false() {
        let entry = make_entry();
        let f = Filter::parse("extension == \"avi\" || extension == \"wmv\"").unwrap();
        assert!(!f.matches(&entry).unwrap());
    }

    #[test]
    fn eval_not_negates() {
        let entry = make_entry();
        let f = Filter::parse("!extension == \"avi\"").unwrap();
        assert!(f.matches(&entry).unwrap());
    }

    #[test]
    fn eval_not_true_becomes_false() {
        let entry = make_entry();
        let f = Filter::parse("!extension == \"mp4\"").unwrap();
        assert!(!f.matches(&entry).unwrap());
    }

    #[test]
    fn eval_missing_field_returns_false() {
        let entry = make_entry();
        let f = Filter::parse("nonexistent.field == \"x\"").unwrap();
        assert!(!f.matches(&entry).unwrap());
    }

    #[test]
    fn eval_complex_expression() {
        let entry = make_entry();
        let f = Filter::parse(
            "(media.video.width >= 1920 || media.video.height >= 1080) && media.duration_ms > 60000"
        ).unwrap();
        assert!(f.matches(&entry).unwrap());
    }

    #[test]
    fn eval_bare_ident_as_string() {
        let entry = make_entry();
        // Bare identifier "av" should be treated as string
        let f = Filter::parse("media.kind == av").unwrap();
        assert!(f.matches(&entry).unwrap());
    }

    // --- Trailing token tests ---

    #[test]
    fn parse_trailing_garbage_rejected() {
        let result = Filter::parse("duration_ms > 60000 garbage");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("trailing"),
            "error should mention trailing token: {err}"
        );
    }

    #[test]
    fn parse_trailing_number_rejected() {
        let result = Filter::parse("extension == \"mp4\" 42");
        assert!(result.is_err());
    }

    #[test]
    fn parse_trailing_after_parens_rejected() {
        let result = Filter::parse("(duration_ms > 60000) extra");
        assert!(result.is_err());
    }

    #[test]
    fn parse_valid_no_trailing_accepted() {
        assert!(Filter::parse("duration_ms > 60000").is_ok());
        assert!(Filter::parse("duration_ms > 60000 && extension == \"mp4\"").is_ok());
    }

    // --- Shorthand alias tests ---

    #[test]
    fn eval_shorthand_duration_ms() {
        let entry = make_entry();
        let f = Filter::parse("duration_ms > 60000").unwrap();
        assert!(f.matches(&entry).unwrap());
    }

    #[test]
    fn eval_shorthand_width() {
        let entry = make_entry();
        let f = Filter::parse("width == 1920").unwrap();
        assert!(f.matches(&entry).unwrap());
    }

    #[test]
    fn eval_shorthand_kind() {
        let entry = make_entry();
        let f = Filter::parse("kind == av").unwrap();
        assert!(f.matches(&entry).unwrap());
    }

    #[test]
    fn eval_shorthand_size_bytes() {
        let entry = make_entry();
        let f = Filter::parse("size_bytes == 1000000").unwrap();
        assert!(f.matches(&entry).unwrap());
    }

    // --- Double negation test ---

    #[test]
    fn parse_double_negation() {
        assert!(Filter::parse("!!extension == \"mp4\"").is_ok());
    }

    #[test]
    fn eval_double_negation_identity() {
        let entry = make_entry();
        let f = Filter::parse("!!extension == \"mp4\"").unwrap();
        // !!true == true
        assert!(f.matches(&entry).unwrap());
    }

    // --- Multi-dot number rejection test ---

    #[test]
    fn lex_multi_dot_number_rejected() {
        // 1.2.3 should not parse as a single number token.
        // The lexer reads "1.2" then stops at the second dot,
        // which becomes part of a subsequent ident token.
        let mut lexer = Lexer::new("1.2.3");
        let first = lexer.next_token().unwrap();
        assert!(
            matches!(first, Token::Num(n) if (n - 1.2).abs() < 0.001),
            "first token should be 1.2, got {first:?}"
        );
        // The ".3" becomes an ident starting with '.' — lexer rejects it
        // because '.' is not alphabetic/underscore.
        assert!(lexer.next_token().is_err());
    }

    // --- Large integer equality test ---

    #[test]
    fn eval_large_integer_equality() {
        let mut entry = make_entry();
        // 2^53 = 9007199254740992 — exact in f64
        entry.fs.size_bytes = 9_007_199_254_740_992;
        let f = Filter::parse("fs.size_bytes == 9007199254740992").unwrap();
        assert!(f.matches(&entry).unwrap());
    }

    // --- Numeric field vs string literal ordering test ---

    #[test]
    fn eval_numeric_field_gt_string_numeric() {
        let entry = make_entry();
        // width=1920, should be numerically > 9 (not lexicographic)
        let f = Filter::parse("media.video.width > \"9\"").unwrap();
        assert!(f.matches(&entry).unwrap());
    }

    // --- NOT with compound expressions ---

    #[test]
    fn eval_not_compound_expression() {
        let entry = make_entry();
        // Inner: 1920 > 1920 is false, so AND is false, NOT false = true
        let f = Filter::parse("!(width > 1920 && extension == \"mkv\")").unwrap();
        assert!(f.matches(&entry).unwrap());
    }

    #[test]
    fn eval_not_compound_when_inner_true() {
        let mut entry = make_entry();
        entry.media.video.as_mut().unwrap().width = 3840;
        entry.extension = "mkv".to_string();
        // Inner: 3840 > 1920 is true AND ext=="mkv" is true, NOT true = false
        let f = Filter::parse("!(width > 1920 && extension == \"mkv\")").unwrap();
        assert!(!f.matches(&entry).unwrap());
    }

    // --- Additional shorthand alias tests ---

    #[test]
    fn eval_shorthand_height() {
        let entry = make_entry();
        let f = Filter::parse("height == 1080").unwrap();
        assert!(f.matches(&entry).unwrap());
    }

    #[test]
    fn eval_shorthand_bitrate_alias() {
        let entry = make_entry();
        let f = Filter::parse("bitrate == 5000000").unwrap();
        assert!(f.matches(&entry).unwrap());
    }
}
