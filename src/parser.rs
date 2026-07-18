use crate::ast::Expr;
use crate::error::{LispError, ParseError, Span};
use crate::lexer::{Token, tokenize};

/// Parses tokens into generic S-expressions. Recognizes only parentheses,
/// integers, and symbols -- `fn` and `let` are given meaning later, by the
/// evaluator.
struct Parser {
    tokens: Vec<crate::lexer::SpannedToken>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&crate::lexer::SpannedToken> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<crate::lexer::SpannedToken> {
        let tok = self.tokens.get(self.pos).cloned();
        if tok.is_some() {
            self.pos += 1;
        }
        tok
    }

    fn parse_expr(&mut self) -> Result<Expr, LispError> {
        let spanned = self.next().ok_or(ParseError::UnexpectedEof)?;
        match spanned.token {
            Token::Int(n) => Ok(Expr::int(n)),
            Token::Bool(b) => Ok(Expr::bool(b)),
            Token::String(s) => Ok(Expr::string(s)),
            Token::Symbol(s) => Ok(Expr::symbol(s)),
            Token::LParen => self.parse_list(),
            Token::RParen => Err(LispError::Parse(ParseError::UnmatchedCloseParen {
                span: spanned.span,
            })),
        }
    }

    fn parse_list(&mut self) -> Result<Expr, LispError> {
        let mut items = Vec::new();
        loop {
            match self.peek() {
                None => {
                    return Err(LispError::Parse(ParseError::UnmatchedOpenParen {
                        span: Span::new(0, 0),
                    }));
                }
                Some(spanned) if spanned.token == Token::RParen => {
                    self.next();
                    return Ok(Expr::list(items));
                }
                _ => items.push(self.parse_expr()?),
            }
        }
    }
}

/// Parses exactly one top-level expression. Errors if the input is empty
/// or contains more than one expression.
pub fn parse_one(source: &str) -> Result<Expr, LispError> {
    let tokens = tokenize(source)?;
    if tokens.is_empty() {
        return Err(LispError::Parse(ParseError::EmptyInput));
    }
    let mut parser = Parser { tokens, pos: 0 };
    let expr = parser.parse_expr()?;
    if let Some(extra) = parser.peek() {
        return Err(LispError::Parse(ParseError::ExtraTokensAfterExpression {
            span: extra.span,
        }));
    }
    Ok(expr)
}

/// Parses every top-level expression in the source, in order.
pub fn parse_program(source: &str) -> Result<Vec<Expr>, LispError> {
    let tokens = tokenize(source)?;
    let mut parser = Parser { tokens, pos: 0 };
    let mut exprs = Vec::new();
    while parser.peek().is_some() {
        exprs.push(parser.parse_expr()?);
    }
    Ok(exprs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_list() {
        let expr = parse_one("(+ 1 (* 2 3))").expect("should parse");
        assert_eq!(
            expr,
            Expr::list(vec![
                Expr::symbol("+"),
                Expr::int(1),
                Expr::list(vec![Expr::symbol("*"), Expr::int(2), Expr::int(3),]),
            ])
        );
    }

    #[test]
    fn parses_empty_list() {
        let expr = parse_one("()").expect("should parse");
        assert_eq!(expr, Expr::list(vec![]));
    }

    #[test]
    fn unmatched_open_paren_is_error() {
        let result = parse_one("(+ 1 2");
        assert!(matches!(
            result,
            Err(LispError::Parse(ParseError::UnmatchedOpenParen { .. }))
        ));
    }

    #[test]
    fn unmatched_close_paren_is_error() {
        let result = parse_one("(+ 1 2))");
        assert!(matches!(
            result,
            Err(LispError::Parse(ParseError::UnmatchedCloseParen { .. }))
                | Err(LispError::Parse(
                    ParseError::ExtraTokensAfterExpression { .. }
                ))
        ));
    }

    #[test]
    fn empty_input_is_error() {
        let result = parse_one("");
        assert!(matches!(
            result,
            Err(LispError::Parse(ParseError::EmptyInput))
        ));
    }

    #[test]
    fn extra_top_level_expression_is_error() {
        let result = parse_one("(+ 1 2) (+ 3 4)");
        assert!(matches!(
            result,
            Err(LispError::Parse(
                ParseError::ExtraTokensAfterExpression { .. }
            ))
        ));
    }

    #[test]
    fn parse_program_returns_multiple_expressions() {
        let exprs = parse_program("(+ 1 2)\n(* 3 4)").expect("should parse");
        assert_eq!(exprs.len(), 2);
    }
}
