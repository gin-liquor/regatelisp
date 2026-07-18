use crate::error::{LexError, Span};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    LParen,
    RParen,
    Int(i64),
    Bool(bool),
    String(String),
    Symbol(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpannedToken {
    pub token: Token,
    pub span: Span,
}

fn is_delimiter(c: char) -> bool {
    c.is_whitespace() || c == '(' || c == ')' || c == ';' || c == '"'
}

/// Splits source text into tokens, tracking byte spans for error reporting.
pub fn tokenize(source: &str) -> Result<Vec<SpannedToken>, LexError> {
    let chars: Vec<(usize, char)> = source.char_indices().collect();
    let len = source.len();
    let mut tokens = Vec::new();
    let mut i = 0usize;

    while i < chars.len() {
        let (pos, c) = chars[i];

        if c.is_whitespace() {
            i += 1;
            continue;
        }

        if c == ';' {
            while i < chars.len() && chars[i].1 != '\n' {
                i += 1;
            }
            continue;
        }

        if c == '(' {
            tokens.push(SpannedToken {
                token: Token::LParen,
                span: Span::new(pos, pos + 1),
            });
            i += 1;
            continue;
        }

        if c == ')' {
            tokens.push(SpannedToken {
                token: Token::RParen,
                span: Span::new(pos, pos + 1),
            });
            i += 1;
            continue;
        }

        if c == '"' {
            let (text, next_i, end_pos) = lex_string(source, &chars, i)?;
            tokens.push(SpannedToken {
                token: Token::String(text),
                span: Span::new(pos, end_pos),
            });
            i = next_i;
            continue;
        }

        // General atom: runs until a delimiter.
        let start = pos;
        let mut end = len;
        let mut j = i;
        while j < chars.len() {
            let (p, ch) = chars[j];
            if is_delimiter(ch) {
                end = p;
                break;
            }
            j += 1;
        }
        if j == chars.len() {
            end = len;
        }
        let text = &source[start..end];
        tokens.push(SpannedToken {
            token: classify_atom(text, Span::new(start, end))?,
            span: Span::new(start, end),
        });
        i = j;
    }

    Ok(tokens)
}

/// Lexes a string literal starting at `chars[start]` (the opening `"`).
/// Returns the decoded text, the index of the first char after the closing
/// quote, and the byte offset of that position.
fn lex_string(
    source: &str,
    chars: &[(usize, char)],
    start: usize,
) -> Result<(String, usize, usize), LexError> {
    let open_pos = chars[start].0;
    let mut decoded = String::new();
    let mut i = start + 1;

    while i < chars.len() {
        let (pos, c) = chars[i];
        match c {
            '"' => {
                let end_pos = pos + 1;
                return Ok((decoded, i + 1, end_pos));
            }
            '\\' => {
                let Some(&(esc_pos, esc_ch)) = chars.get(i + 1) else {
                    return Err(LexError::UnterminatedString {
                        span: Span::new(open_pos, source.len()),
                    });
                };
                let decoded_ch = match esc_ch {
                    '\\' => '\\',
                    '"' => '"',
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    '0' => '\0',
                    other => {
                        return Err(LexError::InvalidEscape {
                            ch: other,
                            span: Span::new(esc_pos - 1, esc_pos + other.len_utf8()),
                        });
                    }
                };
                decoded.push(decoded_ch);
                i += 2;
            }
            _ => {
                decoded.push(c);
                i += 1;
            }
        }
    }

    Err(LexError::UnterminatedString {
        span: Span::new(open_pos, source.len()),
    })
}

fn classify_atom(text: &str, span: Span) -> Result<Token, LexError> {
    if text == "true" {
        return Ok(Token::Bool(true));
    }
    if text == "false" {
        return Ok(Token::Bool(false));
    }
    if let Some(value) = parse_int_literal(text, span)? {
        return Ok(Token::Int(value));
    }
    Ok(Token::Symbol(text.to_string()))
}

/// Recognizes decimal, binary, octal, and hexadecimal integer literals.
/// Returns `Ok(None)` for text that is not an integer literal at all (so it
/// can fall back to being a plain symbol, e.g. `-` or `make-adder`).
/// Returns `Err` for text that looks like a malformed numeric literal (a
/// digit-prefixed radix marker with no valid digits, invalid digits for the
/// radix, misplaced `_`, or an out-of-range value) so such input is never
/// silently accepted as a symbol.
fn parse_int_literal(text: &str, span: Span) -> Result<Option<i64>, LexError> {
    let (negative, unsigned_text) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };

    if unsigned_text.is_empty() {
        // Bare "-" is a symbol.
        return Ok(None);
    }

    let (radix, digits) = if let Some(rest) = unsigned_text.strip_prefix("0b") {
        (2u32, rest)
    } else if let Some(rest) = unsigned_text.strip_prefix("0o") {
        (8u32, rest)
    } else if let Some(rest) = unsigned_text.strip_prefix("0x") {
        (16u32, rest)
    } else if unsigned_text
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_digit())
    {
        (10u32, unsigned_text)
    } else {
        // Doesn't start with a digit or a recognized radix prefix: a symbol.
        return Ok(None);
    };

    let magnitude = parse_digits_with_separators(digits, radix, text, span)?;

    let value = if negative {
        // Read the magnitude as an unsigned value first so `i64::MIN`
        // (whose magnitude, 2^63, does not fit in a positive i64) can be
        // negated correctly.
        if magnitude > (i64::MAX as u64) + 1 {
            return Err(LexError::IntegerOutOfRange {
                text: text.to_string(),
                span,
            });
        }
        if magnitude == (i64::MAX as u64) + 1 {
            i64::MIN
        } else {
            -(magnitude as i64)
        }
    } else {
        if magnitude > i64::MAX as u64 {
            return Err(LexError::IntegerOutOfRange {
                text: text.to_string(),
                span,
            });
        }
        magnitude as i64
    };

    Ok(Some(value))
}

/// Parses the digit run after any radix prefix, allowing `_` only between
/// two valid digits. Returns the magnitude as `u64`.
fn parse_digits_with_separators(
    digits: &str,
    radix: u32,
    full_text: &str,
    span: Span,
) -> Result<u64, LexError> {
    let malformed = || LexError::MalformedRadixLiteral {
        text: full_text.to_string(),
        span,
    };

    let bytes: Vec<char> = digits.chars().collect();
    if bytes.is_empty() {
        return Err(malformed());
    }
    if bytes.first() == Some(&'_') || bytes.last() == Some(&'_') {
        return Err(malformed());
    }

    let mut magnitude: u64 = 0;
    let mut prev_was_underscore = false;
    let mut saw_digit = false;

    for &ch in &bytes {
        if ch == '_' {
            if prev_was_underscore {
                return Err(malformed());
            }
            prev_was_underscore = true;
            continue;
        }
        let Some(digit) = ch.to_digit(radix) else {
            return Err(malformed());
        };
        magnitude = magnitude
            .checked_mul(radix as u64)
            .and_then(|m| m.checked_add(digit as u64))
            .ok_or_else(|| LexError::IntegerOutOfRange {
                text: full_text.to_string(),
                span,
            })?;
        prev_was_underscore = false;
        saw_digit = true;
    }

    if !saw_digit {
        return Err(malformed());
    }

    Ok(magnitude)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token_kinds(source: &str) -> Vec<Token> {
        tokenize(source)
            .expect("tokenize should succeed")
            .into_iter()
            .map(|t| t.token)
            .collect()
    }

    #[test]
    fn tokenizes_plus_with_negative_int() {
        let tokens = token_kinds("(+ 12 -3)");
        assert_eq!(
            tokens,
            vec![
                Token::LParen,
                Token::Symbol("+".to_string()),
                Token::Int(12),
                Token::Int(-3),
                Token::RParen,
            ]
        );
    }

    #[test]
    fn standalone_minus_is_symbol() {
        let tokens = token_kinds("(- 12 3)");
        assert_eq!(
            tokens,
            vec![
                Token::LParen,
                Token::Symbol("-".to_string()),
                Token::Int(12),
                Token::Int(3),
                Token::RParen,
            ]
        );
    }

    #[test]
    fn skips_comments_and_whitespace() {
        let tokens = token_kinds("  (+ 1 2) ; comment\n; full line comment\n(* 3 4)");
        assert_eq!(
            tokens,
            vec![
                Token::LParen,
                Token::Symbol("+".to_string()),
                Token::Int(1),
                Token::Int(2),
                Token::RParen,
                Token::LParen,
                Token::Symbol("*".to_string()),
                Token::Int(3),
                Token::Int(4),
                Token::RParen,
            ]
        );
    }

    #[test]
    fn out_of_range_integer_is_lex_error() {
        let result = tokenize("99999999999999999999");
        assert!(matches!(result, Err(LexError::IntegerOutOfRange { .. })));
    }

    #[test]
    fn symbols_allow_hyphenated_names() {
        let tokens = token_kinds("make-adder");
        assert_eq!(tokens, vec![Token::Symbol("make-adder".to_string())]);
    }

    #[test]
    fn binary_octal_hex_literals() {
        assert_eq!(token_kinds("0b0001"), vec![Token::Int(1)]);
        assert_eq!(token_kinds("0o17"), vec![Token::Int(15)]);
        assert_eq!(token_kinds("0x01"), vec![Token::Int(1)]);
        assert_eq!(token_kinds("0xFF"), vec![Token::Int(255)]);
        assert_eq!(token_kinds("0xff"), vec![Token::Int(255)]);
    }

    #[test]
    fn negative_radix_literals() {
        assert_eq!(token_kinds("-0b1000"), vec![Token::Int(-8)]);
        assert_eq!(token_kinds("-0o10"), vec![Token::Int(-8)]);
        assert_eq!(token_kinds("-0x80"), vec![Token::Int(-128)]);
    }

    #[test]
    fn underscore_separators() {
        assert_eq!(token_kinds("0b1111_0000"), vec![Token::Int(240)]);
        assert_eq!(token_kinds("0xDEAD_BEEF"), vec![Token::Int(3735928559)]);
        assert_eq!(token_kinds("1_000_000"), vec![Token::Int(1_000_000)]);
    }

    #[test]
    fn i64_boundary_values() {
        assert_eq!(
            token_kinds("0x7fff_ffff_ffff_ffff"),
            vec![Token::Int(i64::MAX)]
        );
        assert_eq!(
            token_kinds("-0x8000_0000_0000_0000"),
            vec![Token::Int(i64::MIN)]
        );
        assert_eq!(
            token_kinds("9223372036854775807"),
            vec![Token::Int(i64::MAX)]
        );
        assert_eq!(
            token_kinds("-9223372036854775808"),
            vec![Token::Int(i64::MIN)]
        );
    }

    #[test]
    fn malformed_radix_literals_are_errors() {
        for text in [
            "0b", "0o", "0x", "0b102", "0o89", "0x12g", "0x_ff", "0xff_", "1__000",
        ] {
            let result = tokenize(text);
            assert!(result.is_err(), "expected error for {text}");
        }
    }

    #[test]
    fn uppercase_radix_prefix_is_rejected() {
        // Uppercase prefixes are not valid radix literals; the classifier
        // rejects them as malformed rather than silently treating them as
        // symbols, since they begin with a digit.
        assert!(tokenize("0Xff").is_err());
    }

    #[test]
    fn out_of_range_radix_literals_are_errors() {
        assert!(tokenize("0x8000_0000_0000_0000").is_err());
        assert!(tokenize("-0x8000_0000_0000_0001").is_err());
    }

    #[test]
    fn string_literal_basic() {
        let tokens = token_kinds(r#""hello""#);
        assert_eq!(tokens, vec![Token::String("hello".to_string())]);
    }

    #[test]
    fn string_literal_escapes() {
        let tokens = token_kinds(r#""a\nb""#);
        assert_eq!(tokens, vec![Token::String("a\nb".to_string())]);

        let tokens = token_kinds(r#""\"hello\"""#);
        assert_eq!(tokens, vec![Token::String("\"hello\"".to_string())]);

        let tokens = token_kinds(r#""c:\\temp\\file""#);
        assert_eq!(tokens, vec![Token::String("c:\\temp\\file".to_string())]);
    }

    #[test]
    fn unterminated_string_is_error() {
        let result = tokenize("\"unterminated");
        assert!(matches!(result, Err(LexError::UnterminatedString { .. })));
    }

    #[test]
    fn invalid_escape_is_error() {
        let result = tokenize(r#""\q""#);
        assert!(matches!(result, Err(LexError::InvalidEscape { .. })));
    }
}
