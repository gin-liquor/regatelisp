//! Parsing and rendering of `print` format strings, using a Rust-compatible
//! subset of `{}` placeholder syntax. Parsing (`parse_format_string`) is
//! kept separate from rendering (`render`) so each can be tested in
//! isolation.

use crate::error::FormatError;
use crate::value::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alignment {
    Left,
    Right,
    Center,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatType {
    Display,
    Binary,
    Octal,
    LowerHex,
    UpperHex,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatSpec {
    pub fill: char,
    pub alignment: Option<Alignment>,
    pub show_sign: bool,
    pub alternate: bool,
    pub zero_pad: bool,
    pub width: Option<usize>,
    pub format_type: FormatType,
}

impl Default for FormatSpec {
    fn default() -> Self {
        FormatSpec {
            fill: ' ',
            alignment: None,
            show_sign: false,
            alternate: false,
            zero_pad: false,
            width: None,
            format_type: FormatType::Display,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgumentIndex {
    Implicit,
    Explicit(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatPart {
    Literal(String),
    Placeholder {
        argument: ArgumentIndex,
        spec: FormatSpec,
    },
}

/// Parses a format string into literal and placeholder parts. Does not
/// resolve argument indices or check argument counts -- that happens in
/// `render`, once the actual argument values are known.
pub fn parse_format_string(source: &str) -> Result<Vec<FormatPart>, FormatError> {
    let chars: Vec<char> = source.chars().collect();
    let mut parts = Vec::new();
    let mut literal = String::new();
    let mut i = 0usize;

    while i < chars.len() {
        let c = chars[i];
        match c {
            '{' if chars.get(i + 1) == Some(&'{') => {
                literal.push('{');
                i += 2;
            }
            '}' if chars.get(i + 1) == Some(&'}') => {
                literal.push('}');
                i += 2;
            }
            '{' => {
                if !literal.is_empty() {
                    parts.push(FormatPart::Literal(std::mem::take(&mut literal)));
                }
                let (placeholder, next_i) = parse_placeholder(&chars, i + 1)?;
                parts.push(placeholder);
                i = next_i;
            }
            '}' => return Err(FormatError::UnmatchedCloseBrace),
            _ => {
                literal.push(c);
                i += 1;
            }
        }
    }

    if !literal.is_empty() {
        parts.push(FormatPart::Literal(literal));
    }

    Ok(parts)
}

/// Parses one `{...}` placeholder body starting right after the opening
/// `{`. Returns the parsed part and the index right after the closing `}`.
fn parse_placeholder(chars: &[char], start: usize) -> Result<(FormatPart, usize), FormatError> {
    let close = chars[start..]
        .iter()
        .position(|&c| c == '}')
        .map(|p| start + p)
        .ok_or(FormatError::UnmatchedOpenBrace)?;

    let body: String = chars[start..close].iter().collect();
    let (index_part, spec_part) = match body.split_once(':') {
        Some((idx, spec)) => (idx, Some(spec)),
        None => (body.as_str(), None),
    };

    let argument = if index_part.is_empty() {
        ArgumentIndex::Implicit
    } else {
        let n: usize = index_part
            .parse()
            .map_err(|_| FormatError::UnmatchedOpenBrace)?;
        ArgumentIndex::Explicit(n)
    };

    let spec = parse_format_spec(spec_part.unwrap_or(""))?;

    Ok((FormatPart::Placeholder { argument, spec }, close + 1))
}

/// Parses a format spec body (the part after `:`), following the grammar
/// `[[fill]align][sign][#][0][width][type]`.
fn parse_format_spec(spec: &str) -> Result<FormatSpec, FormatError> {
    let chars: Vec<char> = spec.chars().collect();
    let mut i = 0usize;
    let mut result = FormatSpec::default();

    // [[fill]align]
    if chars.len() >= 2 && is_alignment_char(chars[1]) {
        result.fill = chars[0];
        result.alignment = Some(alignment_from_char(chars[1]));
        i += 2;
    } else if !chars.is_empty() && is_alignment_char(chars[0]) {
        result.alignment = Some(alignment_from_char(chars[0]));
        i += 1;
    }

    // [sign]
    if chars.get(i) == Some(&'+') {
        result.show_sign = true;
        i += 1;
    }

    // [#]
    if chars.get(i) == Some(&'#') {
        result.alternate = true;
        i += 1;
    }

    // [0]
    if chars.get(i) == Some(&'0') {
        result.zero_pad = true;
        i += 1;
    }

    // [width]
    let width_start = i;
    while chars.get(i).is_some_and(|c| c.is_ascii_digit()) {
        i += 1;
    }
    if i > width_start {
        let width_str: String = chars[width_start..i].iter().collect();
        result.width = width_str.parse().ok();
    }

    // [type]
    if i < chars.len() {
        let type_char = chars[i];
        i += 1;
        result.format_type = match type_char {
            'b' => FormatType::Binary,
            'o' => FormatType::Octal,
            'x' => FormatType::LowerHex,
            'X' => FormatType::UpperHex,
            other => return Err(FormatError::UnsupportedFormatType { type_char: other }),
        };
    }

    if i != chars.len() {
        // Leftover characters mean the spec doesn't match the supported
        // grammar (e.g. `?` for Debug, precision `.2`, `$` width/precision).
        let type_char = chars[i];
        return Err(FormatError::UnsupportedFormatType { type_char });
    }

    Ok(result)
}

fn is_alignment_char(c: char) -> bool {
    matches!(c, '<' | '>' | '^')
}

fn alignment_from_char(c: char) -> Alignment {
    match c {
        '<' => Alignment::Left,
        '>' => Alignment::Right,
        '^' => Alignment::Center,
        _ => unreachable!("caller checks is_alignment_char first"),
    }
}

/// Renders parsed format parts against the given argument values, following
/// Rust's implicit/explicit argument-index rules, and checks that every
/// argument was used and every placeholder had an argument available.
pub fn render(parts: &[FormatPart], args: &[Value]) -> Result<String, FormatError> {
    let mut output = String::new();
    let mut implicit_cursor = 0usize;
    let mut used = vec![false; args.len()];

    for part in parts {
        match part {
            FormatPart::Literal(text) => output.push_str(text),
            FormatPart::Placeholder { argument, spec } => {
                let index = match argument {
                    ArgumentIndex::Implicit => {
                        let idx = implicit_cursor;
                        implicit_cursor += 1;
                        idx
                    }
                    ArgumentIndex::Explicit(idx) => *idx,
                };
                let value = args
                    .get(index)
                    .ok_or(FormatError::MissingArgument { index })?;
                if index < used.len() {
                    used[index] = true;
                }
                render_value(value, spec, &mut output)?;
            }
        }
    }

    if let Some(index) = used.iter().position(|&u| !u) {
        return Err(FormatError::UnusedArgument { index });
    }

    Ok(output)
}

fn render_value(value: &Value, spec: &FormatSpec, output: &mut String) -> Result<(), FormatError> {
    match value {
        Value::Int(n) => render_int(*n, spec, output),
        Value::Bool(b) => render_plain(if *b { "true" } else { "false" }, spec, output),
        Value::String(s) => render_string(s, spec, output),
        Value::Unit => render_plain("()", spec, output),
        Value::Datum(datum) => render_plain(&datum.to_string(), spec, output),
        Value::Builtin(_) | Value::Closure(_) => Err(FormatError::TypeMismatch {
            format_type: format_type_label(spec.format_type),
            value_type: value.type_name(),
        }),
    }
}

fn format_type_label(t: FormatType) -> &'static str {
    match t {
        FormatType::Display => "{}",
        FormatType::Binary => "b",
        FormatType::Octal => "o",
        FormatType::LowerHex => "x",
        FormatType::UpperHex => "X",
    }
}

fn render_int(value: i64, spec: &FormatSpec, output: &mut String) -> Result<(), FormatError> {
    let (sign, digits, prefix) = match spec.format_type {
        FormatType::Display => {
            let sign = if value < 0 {
                "-"
            } else if spec.show_sign {
                "+"
            } else {
                ""
            };
            let digits = value.unsigned_abs().to_string();
            (sign, digits, "")
        }
        FormatType::Binary | FormatType::Octal | FormatType::LowerHex | FormatType::UpperHex => {
            // Rust formats signed integers in non-decimal bases using their
            // two's-complement bit pattern, so there is never a `-` sign;
            // `+` still applies to the (always non-negative) bit pattern.
            let bits = value as u64;
            let (digits, prefix) = match spec.format_type {
                FormatType::Binary => (format!("{bits:b}"), "0b"),
                FormatType::Octal => (format!("{bits:o}"), "0o"),
                FormatType::LowerHex => (format!("{bits:x}"), "0x"),
                FormatType::UpperHex => (format!("{bits:X}"), "0x"),
                FormatType::Display => unreachable!(),
            };
            let sign = if spec.show_sign { "+" } else { "" };
            (sign, digits, prefix)
        }
    };

    let prefix = if spec.alternate { prefix } else { "" };
    let body_len = sign.len() + prefix.len() + digits.len();

    match spec.width {
        Some(width) if width > body_len && spec.zero_pad && spec.alignment.is_none() => {
            // Zero-padding without an explicit alignment inserts zeros
            // right after the sign/prefix, matching Rust's numeric
            // zero-fill behavior (e.g. `{:+06}` -> `+00010`).
            let pad = width - body_len;
            output.push_str(sign);
            output.push_str(prefix);
            for _ in 0..pad {
                output.push('0');
            }
            output.push_str(&digits);
        }
        Some(width) if width > body_len => {
            let pad = width - body_len;
            let body = format!("{sign}{prefix}{digits}");
            push_aligned(output, &body, pad, spec, Alignment::Right);
        }
        _ => {
            output.push_str(sign);
            output.push_str(prefix);
            output.push_str(&digits);
        }
    }

    Ok(())
}

fn render_string(s: &str, spec: &FormatSpec, output: &mut String) -> Result<(), FormatError> {
    if spec.format_type != FormatType::Display {
        return Err(FormatError::TypeMismatch {
            format_type: format_type_label(spec.format_type),
            value_type: "string",
        });
    }
    if spec.show_sign || spec.alternate || spec.zero_pad {
        return Err(FormatError::TypeMismatch {
            format_type: format_type_label(spec.format_type),
            value_type: "string",
        });
    }
    render_plain(s, spec, output)
}

/// Renders text with only width/alignment/fill applied (used for strings
/// and `Unit`, where sign/`#`/zero-padding/radix are not meaningful).
fn render_plain(text: &str, spec: &FormatSpec, output: &mut String) -> Result<(), FormatError> {
    let len = text.chars().count();
    match spec.width {
        Some(width) if width > len => {
            let pad = width - len;
            push_aligned(output, text, pad, spec, Alignment::Left);
        }
        _ => output.push_str(text),
    }
    Ok(())
}

/// Writes `body` into `output`, distributing `pad` fill characters
/// according to `spec.alignment` (falling back to `default_alignment` when
/// unspecified). For center alignment with an odd remainder, the extra fill
/// character goes on the right, matching Rust's behavior.
fn push_aligned(
    output: &mut String,
    body: &str,
    pad: usize,
    spec: &FormatSpec,
    default_alignment: Alignment,
) {
    let alignment = spec.alignment.unwrap_or(default_alignment);
    match alignment {
        Alignment::Left => {
            output.push_str(body);
            for _ in 0..pad {
                output.push(spec.fill);
            }
        }
        Alignment::Right => {
            for _ in 0..pad {
                output.push(spec.fill);
            }
            output.push_str(body);
        }
        Alignment::Center => {
            let left = pad / 2;
            let right = pad - left;
            for _ in 0..left {
                output.push(spec.fill);
            }
            output.push_str(body);
            for _ in 0..right {
                output.push(spec.fill);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    fn ints(values: &[i64]) -> Vec<Value> {
        values.iter().map(|&n| Value::Int(n)).collect()
    }

    fn render_str(format: &str, args: &[Value]) -> String {
        let parts = parse_format_string(format).expect("format should parse");
        render(&parts, args).expect("render should succeed")
    }

    #[test]
    fn plain_placeholders_use_implicit_order() {
        assert_eq!(render_str("{} {}", &ints(&[10, 20])), "10 20");
    }

    #[test]
    fn explicit_argument_indices() {
        assert_eq!(render_str("{1} {0} {1}", &ints(&[10, 20])), "20 10 20");
    }

    #[test]
    fn mixed_explicit_then_implicit_does_not_advance_implicit_cursor() {
        assert_eq!(render_str("{1} {}", &ints(&[10, 20])), "20 10");
    }

    #[test]
    fn escaped_braces() {
        assert_eq!(render_str("{{value={}}}", &ints(&[10])), "{value=10}");
    }

    #[test]
    fn radix_formatting() {
        let args = ints(&[255]);
        assert_eq!(render_str("{:b}", &args), "11111111");
        assert_eq!(render_str("{:o}", &args), "377");
        assert_eq!(render_str("{:x}", &args), "ff");
        assert_eq!(render_str("{:X}", &args), "FF");
    }

    #[test]
    fn alternate_prefix() {
        let args = ints(&[255]);
        assert_eq!(render_str("{:#b}", &args), "0b11111111");
        assert_eq!(render_str("{:#o}", &args), "0o377");
        assert_eq!(render_str("{:#x}", &args), "0xff");
        assert_eq!(render_str("{:#X}", &args), "0xFF");
    }

    #[test]
    fn zero_padding() {
        assert_eq!(render_str("{:08x}", &ints(&[255])), "000000ff");
    }

    #[test]
    fn zero_padding_with_prefix() {
        assert_eq!(render_str("{:#010x}", &ints(&[255])), "0x000000ff");
    }

    #[test]
    fn alignment_variants() {
        let args = ints(&[123]);
        assert_eq!(render_str("|{:<8}|", &args), "|123     |");
        assert_eq!(render_str("|{:>8}|", &args), "|     123|");
        assert_eq!(render_str("|{:^8}|", &args), "|  123   |");
    }

    #[test]
    fn fill_character() {
        assert_eq!(render_str("|{:*>8}|", &ints(&[123])), "|*****123|");
        assert_eq!(render_str("|{:-^9}|", &ints(&[123])), "|---123---|");
    }

    #[test]
    fn sign() {
        assert_eq!(render_str("{:+} {:+}", &ints(&[10, -10])), "+10 -10");
    }

    #[test]
    fn sign_with_zero_padding() {
        assert_eq!(render_str("{:+06}", &ints(&[10])), "+00010");
    }

    #[test]
    fn string_alignment() {
        let args = vec![
            Value::String(Rc::new("abc".to_string())),
            Value::String(Rc::new("abc".to_string())),
        ];
        assert_eq!(render_str("|{:<8}|{:>8}|", &args), "|abc     |     abc|");
    }

    #[test]
    fn negative_hex_uses_twos_complement() {
        assert_eq!(render_str("{:x}", &ints(&[-1])), "ffffffffffffffff");
        assert_eq!(render_str("{:X}", &ints(&[-1])), "FFFFFFFFFFFFFFFF");
        assert_eq!(render_str("{:#x}", &ints(&[-1])), "0xffffffffffffffff");
    }

    #[test]
    fn unmatched_open_brace_is_error() {
        let result = parse_format_string("{");
        assert!(matches!(result, Err(FormatError::UnmatchedOpenBrace)));
    }

    #[test]
    fn unmatched_close_brace_is_error() {
        let result = parse_format_string("}");
        assert!(matches!(result, Err(FormatError::UnmatchedCloseBrace)));
    }

    #[test]
    fn unsupported_format_type_is_error() {
        let result = parse_format_string("{:?}");
        assert!(matches!(
            result,
            Err(FormatError::UnsupportedFormatType { .. })
        ));
    }

    #[test]
    fn string_with_hex_format_is_type_mismatch() {
        let parts = parse_format_string("{:x}").unwrap();
        let args = vec![Value::String(Rc::new("hello".to_string()))];
        let result = render(&parts, &args);
        assert!(matches!(result, Err(FormatError::TypeMismatch { .. })));
    }

    #[test]
    fn missing_argument_is_error() {
        let parts = parse_format_string("{} {}").unwrap();
        let result = render(&parts, &ints(&[10]));
        assert!(matches!(result, Err(FormatError::MissingArgument { .. })));
    }

    #[test]
    fn unused_argument_is_error() {
        let parts = parse_format_string("{}").unwrap();
        let result = render(&parts, &ints(&[10, 20]));
        assert!(matches!(result, Err(FormatError::UnusedArgument { .. })));
    }

    #[test]
    fn explicit_reuse_counts_as_used() {
        assert_eq!(render_str("{0} {0}", &ints(&[10])), "10 10");
    }

    #[test]
    fn out_of_range_argument_index_is_error() {
        let parts = parse_format_string("{1}").unwrap();
        let result = render(&parts, &ints(&[10]));
        assert!(matches!(result, Err(FormatError::MissingArgument { .. })));
    }
}
