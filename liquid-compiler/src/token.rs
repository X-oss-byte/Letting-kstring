use std::fmt;

use liquid_interpreter::Expression;
use liquid_interpreter::Variable;
use liquid_value::Scalar;
use liquid_value::Value;

use super::error::Result;
use super::parser::unexpected_token_error;

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum ComparisonOperator {
    Equals,
    NotEquals,
    LessThan,
    GreaterThan,
    LessThanEquals,
    GreaterThanEquals,
    Contains,
}

impl fmt::Display for ComparisonOperator {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let out = match *self {
            ComparisonOperator::Equals => "==",
            ComparisonOperator::NotEquals => "!=",
            ComparisonOperator::LessThanEquals => "<=",
            ComparisonOperator::GreaterThanEquals => ">=",
            ComparisonOperator::LessThan => "<",
            ComparisonOperator::GreaterThan => ">",
            ComparisonOperator::Contains => "contains",
        };
        write!(f, "{}", out)
    }
}
#[derive(Clone, Debug, PartialEq)]
pub enum Token {
    Pipe,
    Dot,
    Colon,
    Comma,
    OpenSquare,
    CloseSquare,
    OpenRound,
    CloseRound,
    Question,
    Dash,
    Assignment,
    Identifier(String),
    StringLiteral(String),
    IntegerLiteral(i32),
    FloatLiteral(f64),
    BooleanLiteral(bool),
    DotDot,
    Comparison(ComparisonOperator),
    And,
    Or,
}

impl Token {
    /// Parses a token that can possibly represent a Value
    /// to said Value. Returns an Err if the token can not
    /// be interpreted as a Value.
    pub fn to_value(&self) -> Result<Value> {
        match self {
            &Token::StringLiteral(ref x) => Ok(Value::scalar(x.to_owned())),
            &Token::IntegerLiteral(x) => Ok(Value::scalar(x)),
            &Token::FloatLiteral(x) => Ok(Value::scalar(x)),
            &Token::BooleanLiteral(x) => Ok(Value::scalar(x)),
            x => Err(unexpected_token_error("string | number | boolean", Some(x))),
        }
    }

    /// Translates a Token to a Value, looking it up in the context if
    /// necessary
    pub fn to_arg(&self) -> Result<Expression> {
        match *self {
            Token::IntegerLiteral(f) => Ok(Expression::with_literal(f)),
            Token::FloatLiteral(f) => Ok(Expression::with_literal(f)),
            Token::StringLiteral(ref s) => Ok(Expression::with_literal(s.to_owned())),
            Token::BooleanLiteral(b) => Ok(Expression::with_literal(b)),
            Token::Identifier(ref id) => {
                let mut path = id.split('.').map(|s| Scalar::new(s.to_owned()));
                let mut var = Variable::with_literal(
                    path.next().expect("there should always be at least one"),
                );
                var.extend(path);
                Ok(Expression::Variable(var))
            }
            ref x => Err(unexpected_token_error(
                "string | number | boolean | identifier",
                Some(x),
            )),
        }
    }
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Token::Pipe => write!(f, "|"),
            Token::Dot => write!(f, "."),
            Token::Colon => write!(f, ":"),
            Token::Comma => write!(f, ","),
            Token::OpenSquare => write!(f, "["),
            Token::CloseSquare => write!(f, "]"),
            Token::OpenRound => write!(f, "("),
            Token::CloseRound => write!(f, ")"),
            Token::Question => write!(f, "?"),
            Token::Dash => write!(f, "-"),
            Token::DotDot => write!(f, ".."),
            Token::Assignment => write!(f, "="),
            Token::And => write!(f, "and"),
            Token::Or => write!(f, "or"),

            Token::Comparison(ref x) => write!(f, "{}", x),
            Token::Identifier(ref x) | Token::StringLiteral(ref x) => write!(f, "{}", x),
            Token::IntegerLiteral(ref x) => write!(f, "{}", x),
            Token::FloatLiteral(ref x) => write!(f, "{}", x),
            Token::BooleanLiteral(ref x) => write!(f, "{}", x),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use liquid_interpreter::Context;

    #[test]
    fn evaluate_handles_string_literals() {
        let ctx = Context::new();
        let t = Token::StringLiteral("hello".to_owned());
        assert_eq!(
            *t.to_arg().unwrap().evaluate(&ctx).unwrap(),
            Value::scalar("hello")
        );
    }

    #[test]
    fn evaluate_handles_number_literals() {
        let ctx = Context::new();
        assert_eq!(
            *Token::FloatLiteral(42f64)
                .to_arg()
                .unwrap()
                .evaluate(&ctx)
                .unwrap(),
            Value::scalar(42f64)
        );

        let ctx = Context::new();
        assert_eq!(
            *Token::IntegerLiteral(42i32)
                .to_arg()
                .unwrap()
                .evaluate(&ctx)
                .unwrap(),
            Value::scalar(42i32)
        );
    }

    #[test]
    fn evaluate_handles_boolean_literals() {
        let ctx = Context::new();
        assert_eq!(
            *Token::BooleanLiteral(true)
                .to_arg()
                .unwrap()
                .evaluate(&ctx)
                .unwrap(),
            Value::scalar(true)
        );

        assert_eq!(
            *Token::BooleanLiteral(false)
                .to_arg()
                .unwrap()
                .evaluate(&ctx)
                .unwrap(),
            Value::scalar(false)
        );
    }

    #[test]
    fn evaluate_handles_identifiers() {
        let mut ctx = Context::new();
        ctx.stack_mut().set_global("var0", Value::scalar(42f64));
        assert_eq!(
            *Token::Identifier("var0".to_owned())
                .to_arg()
                .unwrap()
                .evaluate(&ctx)
                .unwrap(),
            Value::scalar(42f64)
        );
        assert!(
            Token::Identifier("nope".to_owned())
                .to_arg()
                .unwrap()
                .evaluate(&ctx)
                .is_err()
        );
    }

    #[test]
    fn evaluate_returns_none_on_invalid_token() {
        assert!(Token::DotDot.to_arg().is_err());
    }
}
