//! Parser
//!
//! This module contains functions than can be used for writing plugins
//! but should be ignored for simple usage.

use std::collections::HashSet;
use std::slice::Iter;

use liquid_interpreter::Expression;
use liquid_interpreter::Renderable;
use liquid_interpreter::Text;
use liquid_interpreter::Variable;
use liquid_interpreter::{FilterCall, FilterChain};

use super::error::{Error, Result};
use super::Element;
use super::LiquidOptions;
use super::ParseBlock;
use super::ParseTag;
use super::Token;

/// Parses the provided elements into a number of Renderable items
/// This is the internal version of parse that accepts Elements tokenized
/// by `lexer::tokenize` and does not register built-in blocks. The main use
/// for this function is for writing custom blocks.
///
/// For parsing from a String you should refer to `liquid::parse`.
pub fn parse(elements: &[Element], options: &LiquidOptions) -> Result<Vec<Box<Renderable>>> {
    let mut ret = vec![];
    let mut iter = elements.iter();
    let mut token = iter.next();
    while token.is_some() {
        let render = match *token.unwrap() {
            Element::Expression(ref tokens, _) => parse_expression(tokens, options)?,
            Element::Tag(ref tokens, _) => parse_tag(&mut iter, tokens, options)?,
            Element::Raw(ref x) => Box::new(Text::new(x.as_str())),
        };
        ret.push(render);
        token = iter.next();
    }
    Ok(ret)
}

const NOTHING: Option<&str> = None;

pub fn unexpected_token_error<S: ToString>(expected: &str, actual: Option<S>) -> Error {
    let actual = actual.map(|x| x.to_string());
    unexpected_token_error_string(expected, actual)
}

pub fn unexpected_token_error_string(expected: &str, actual: Option<String>) -> Error {
    let actual = actual.unwrap_or_else(|| "nothing".to_owned());
    Error::with_msg(format!("Expected {}, found `{}`", expected, actual))
}

// creates an expression, which wraps everything that gets rendered
fn parse_expression(tokens: &[Token], options: &LiquidOptions) -> Result<Box<Renderable>> {
    match tokens.get(0) {
        Some(&Token::Identifier(_))
            if tokens.len() > 1 && (tokens[1] == Token::Dot || tokens[1] == Token::OpenSquare) =>
        {
            let mut result = tokens[0]
                .to_arg()?
                .into_variable()
                .expect("identifiers must be variables");
            let indexes = parse_indexes(&tokens[1..])?;
            result.extend(indexes);
            Ok(Box::new(result))
        }
        Some(&Token::Identifier(ref x)) if options.tags.contains_key(x.as_str()) => {
            options.tags[x.as_str()].parse(x, &tokens[1..], options)
        }
        None => Err(unexpected_token_error("expression", NOTHING)),
        _ => {
            let output = parse_output(tokens)?;
            Ok(Box::new(output))
        }
    }
}

pub fn parse_indexes(mut tokens: &[Token]) -> Result<Vec<Expression>> {
    let mut indexes: Vec<Expression> = Vec::new();

    let mut rest = 0;
    while tokens.len() > rest {
        tokens = &tokens[rest..];
        rest = match tokens[0] {
            Token::Dot if tokens.len() > 1 => {
                match tokens[1] {
                    Token::Identifier(ref x) => {
                        indexes.push(Expression::with_literal(x.to_owned()))
                    }
                    _ => {
                        return Err(unexpected_token_error("identifier", Some(&tokens[0])));
                    }
                };
                2
            }
            Token::OpenSquare if tokens.len() > 2 => {
                let index = match tokens[1] {
                    Token::StringLiteral(ref x) => Expression::with_literal(x.to_owned()),
                    Token::IntegerLiteral(ref x) => Expression::with_literal(*x),
                    Token::Identifier(ref x) => {
                        Expression::Variable(Variable::with_literal(x.to_owned()))
                    }
                    _ => {
                        return Err(unexpected_token_error(
                            "string | whole number | identifier",
                            Some(&tokens[0]),
                        ));
                    }
                };
                indexes.push(index);

                if tokens[2] != Token::CloseSquare {
                    return Err(unexpected_token_error("`]`", Some(&tokens[0])));
                }
                3
            }
            _ => return Ok(indexes),
        };
    }

    Ok(indexes)
}

/// Creates an FilterChain, a wrapper around values, variables and filters
/// used internally, from a list of Tokens. This is mostly useful
/// for correctly parsing complex expressions with filters.
pub fn parse_output(tokens: &[Token]) -> Result<FilterChain> {
    let first_pipe = tokens
        .iter()
        .enumerate()
        .filter_map(|(i, t)| if *t == Token::Pipe { Some(i) } else { None })
        .next()
        .unwrap_or_else(|| tokens.len());

    let mut entry = tokens[0].to_arg()?;
    if let Expression::Variable(ref mut entry) = &mut entry {
        let indexes = parse_indexes(&tokens[1..first_pipe])?;
        entry.extend(indexes);
    }
    let tokens = &tokens[first_pipe..];

    let mut filters = vec![];
    let mut iter = tokens.iter().peekable();

    while iter.peek() != None {
        expect(&mut iter, &Token::Pipe)?;

        let name = match iter.next() {
            Some(&Token::Identifier(ref name)) => name,
            x => {
                return Err(unexpected_token_error("identifier", x));
            }
        };
        let mut args = vec![];

        match iter.peek() {
            Some(&&Token::Pipe) | None => {
                filters.push(FilterCall::new(name, args));
                continue;
            }
            _ => (),
        }

        expect(&mut iter, &Token::Colon)?;

        // loops through the argument list after the filter name
        while iter.peek() != None && iter.peek().unwrap() != &&Token::Pipe {
            args.push(iter.next().unwrap().to_arg()?);

            // ensure that the next token is either a Comma or a Pipe
            match iter.peek() {
                Some(&&Token::Comma) => {
                    let _ = iter.next().unwrap();
                    continue;
                }
                Some(&&Token::Pipe) | None => break,
                _ => {
                    return Err(unexpected_token_error(
                        "`,` | `|`",
                        Some(iter.next().unwrap()),
                    ));
                }
            }
        }

        filters.push(FilterCall::new(name, args));
    }

    Ok(FilterChain::new(entry, filters))
}

// a tag can be either a single-element tag or a block, which can contain other
// elements and is delimited by a closing tag named {{end +
// the_name_of_the_tag}}. Tags do not get rendered, but blocks may contain
// renderable expressions
fn parse_tag(
    iter: &mut Iter<Element>,
    tokens: &[Token],
    options: &LiquidOptions,
) -> Result<Box<Renderable>> {
    let tag = &tokens[0];
    match *tag {
        // is a tag
        Token::Identifier(ref x) if options.tags.contains_key(x.as_str()) => {
            options.tags[x.as_str()].parse(x, &tokens[1..], options)
        }

        // is a block
        Token::Identifier(ref x) if options.blocks.contains_key(x.as_str()) => {
            // Collect all the inner elements of this block until we find a
            // matching "end<blockname>" tag. Note that there may be nested blocks
            // of the same type (and hence have the same closing delimiter) *inside*
            // the body of the block, which would premauturely stop the element
            // collection early if we did a nesting-unaware search for the
            // closing tag.
            //
            // The whole nesting count machinery below is to ensure we only stop
            // collecting elements when we have an un-nested closing tag.

            let end_tag = Token::Identifier(format!("end{}", x));
            let mut children = vec![];
            let mut nesting_depth = 0;
            for t in iter {
                if let Element::Tag(ref tokens, _) = *t {
                    match tokens[0] {
                        ref n if n == tag => {
                            nesting_depth += 1;
                        }
                        ref n if n == &end_tag && nesting_depth > 0 => {
                            nesting_depth -= 1;
                        }
                        ref n if n == &end_tag && nesting_depth == 0 => break,
                        _ => {}
                    }
                };
                children.push(t.clone())
            }
            options.blocks[x.as_str()].parse(x, &tokens[1..], &children, options)
        }

        ref x => Err(Error::with_msg("Tag is not supported").context("tag", format!("{}", x))),
    }
}

/// Confirm that the next token in a token stream is what you want it
/// to be. The token iterator is moved to the next token in the stream.
pub fn expect<'a, T>(tokens: &mut T, expected: &Token) -> Result<&'a Token>
where
    T: Iterator<Item = &'a Token>,
{
    match tokens.next() {
        Some(x) if x == expected => Ok(x),
        x => Err(unexpected_token_error(&format!("`{}`", expected), x)),
    }
}

/// Extracts a token from the token stream that can be used to express a
/// value. For our purposes, this is either a string literal, number literal
/// or an identifier that might refer to a variable.
pub fn consume_value_token(tokens: &mut Iter<Token>) -> Result<Token> {
    match tokens.next() {
        Some(t) => value_token(t.clone()),
        None => Err(unexpected_token_error(
            "string | number | boolean | identifier",
            NOTHING,
        )),
    }
}

/// Recognises a value token, returning an error if a non-value token
/// is presented.
pub fn value_token(t: Token) -> Result<Token> {
    match t {
        v @ Token::StringLiteral(_)
        | v @ Token::IntegerLiteral(_)
        | v @ Token::FloatLiteral(_)
        | v @ Token::BooleanLiteral(_)
        | v @ Token::Identifier(_) => Ok(v),
        x => Err(unexpected_token_error(
            "string | number | boolean | identifier",
            Some(&x),
        )),
    }
}

/// Describes the optional trailing part of a block split.
pub struct BlockSplit<'a> {
    pub delimiter: String,
    pub args: &'a [Token],
    pub trailing: &'a [Element],
}

/// A sub-block aware splitter that will only split the token stream
/// when it finds a delimter at the top level of the token stream,
/// ignoring any it finds in nested blocks.
///
/// Returns a slice contaiing all elements before the delimiter, and
/// an optional `BlockSplit` struct describing the delimiter and
/// trailing elements.
pub fn split_block<'a>(
    tokens: &'a [Element],
    delimiters: &[&str],
    options: &LiquidOptions,
) -> (&'a [Element], Option<BlockSplit<'a>>) {
    // construct a fast-lookup cache of the delimiters, as we're going to be
    // consulting the delimiter list a *lot*.
    let delims: HashSet<&str> = delimiters.iter().cloned().collect();
    let mut stack: Vec<String> = Vec::new();

    for (i, t) in tokens.iter().enumerate() {
        if let Element::Tag(ref args, _) = *t {
            match args[0] {
                Token::Identifier(ref name) if options.blocks.contains_key(name.as_str()) => {
                    stack.push("end".to_owned() + name);
                }

                Token::Identifier(ref name) if Some(name) == stack.last() => {
                    stack.pop();
                }

                Token::Identifier(ref name)
                    if stack.is_empty() && delims.contains(name.as_str()) =>
                {
                    let leading = &tokens[0..i];
                    let split = BlockSplit {
                        delimiter: name.clone(),
                        args,
                        trailing: &tokens[i..],
                    };
                    return (leading, Some(split));
                }
                _ => {}
            }
        }
    }

    (&tokens[..], None)
}

#[cfg(test)]
mod test_parse_expression {
    use super::super::lexer::granularize;
    use super::*;

    use liquid_interpreter::Context;
    use liquid_value::Array;
    use liquid_value::Object;
    use liquid_value::Value;

    fn null_options() -> LiquidOptions {
        LiquidOptions::default()
    }

    #[test]
    fn string() {
        let tokens = granularize("\"hey\"").unwrap();
        let result = parse_expression(&tokens, &null_options()).unwrap();
        let mut context = Context::new();
        let result = result.render(&mut context).unwrap();
        assert_eq!("hey", result);
    }

    #[test]
    fn object_dot_access() {
        let tokens = granularize("post.number").unwrap();
        let mut context = Context::new();
        let mut post = Object::new();
        post.insert("number".into(), Value::scalar(42i32));
        context.stack_mut().set_global("post", Value::Object(post));

        let result = parse_expression(&tokens, &null_options()).unwrap();
        let result = result.render(&mut context).unwrap();
        assert_eq!("42", result);
    }

    #[test]
    fn object_index_access() {
        let tokens = granularize("post[\"number\"]").unwrap();
        let mut context = Context::new();
        let mut post = Object::new();
        post.insert("number".into(), Value::scalar(42i32));
        context.stack_mut().set_global("post", Value::Object(post));

        let result = parse_expression(&tokens, &null_options()).unwrap();
        let result = result.render(&mut context).unwrap();
        assert_eq!("42", result);
    }

    #[test]
    fn object_variable_access() {
        let tokens = granularize("post[foo]").unwrap();
        let mut context = Context::new();
        let mut post = Object::new();
        post.insert("number".into(), Value::scalar(42i32));
        context.stack_mut().set_global("post", Value::Object(post));
        context
            .stack_mut()
            .set_global("foo", Value::scalar("number"));

        let result = parse_expression(&tokens, &null_options()).unwrap();
        let result = result.render(&mut context).unwrap();
        assert_eq!("42", result);
    }

    #[test]
    fn array_index_access() {
        let tokens = granularize("post[0]").unwrap();
        let mut context = Context::new();
        let mut post = Array::new();
        post.push(Value::scalar(42i32));
        context.stack_mut().set_global("post", Value::Array(post));

        let result = parse_expression(&tokens, &null_options()).unwrap();
        let result = result.render(&mut context).unwrap();
        assert_eq!("42", result);
    }

    #[test]
    fn mixed_access() {
        let tokens = granularize("post.child[0]").unwrap();
        let mut context = Context::new();
        let mut post = Object::new();
        post.insert("child".into(), Value::Array(vec![Value::scalar(42i32)]));
        context.stack_mut().set_global("post", Value::Object(post));

        let result = parse_expression(&tokens, &null_options()).unwrap();
        let result = result.render(&mut context).unwrap();
        assert_eq!("42", result);
    }
}

#[cfg(test)]
mod test_parse_output {
    use super::*;

    use liquid_interpreter::Expression;
    use liquid_value::Value;

    use super::super::lexer::granularize;

    #[test]
    fn parses_filters() {
        let tokens = granularize("abc | def:'1',2,'3' | blabla").unwrap();

        let result = parse_output(&tokens);
        assert_eq!(
            result.unwrap(),
            FilterChain::new(
                Expression::Variable(Variable::with_literal("abc")),
                vec![
                    FilterCall::new(
                        "def",
                        vec![
                            Expression::Literal(Value::scalar("1")),
                            Expression::Literal(Value::scalar(2.0)),
                            Expression::Literal(Value::scalar("3")),
                        ],
                    ),
                    FilterCall::new("blabla", vec![]),
                ]
            )
        );
    }

    #[test]
    fn parses_index() {
        let tokens = granularize("abc[0] | def:'1',2,'3' | blabla").unwrap();

        let result = parse_output(&tokens);
        assert_eq!(
            result.unwrap(),
            FilterChain::new(
                Expression::Variable(Variable::with_literal("abc").push_literal(0)),
                vec![
                    FilterCall::new(
                        "def",
                        vec![
                            Expression::Literal(Value::scalar("1")),
                            Expression::Literal(Value::scalar(2.0)),
                            Expression::Literal(Value::scalar("3")),
                        ],
                    ),
                    FilterCall::new("blabla", vec![]),
                ]
            )
        );
    }

    #[test]
    fn requires_filter_names() {
        let tokens = granularize("abc | '1','2','3' | blabla").unwrap();

        let result = parse_output(&tokens);
        assert_eq!(
            result.unwrap_err().to_string(),
            "liquid: Expected identifier, found `1`\n"
        );
    }

    #[test]
    fn fails_on_missing_pipes() {
        let tokens = granularize("abc | def:'1',2,'3' blabla").unwrap();

        let result = parse_output(&tokens);
        assert_eq!(
            result.unwrap_err().to_string(),
            "liquid: Expected `,` | `|`, found `blabla`\n"
        );
    }

    #[test]
    fn fails_on_missing_colons() {
        let tokens = granularize("abc | def '1',2,'3' | blabla").unwrap();

        let result = parse_output(&tokens);
        assert_eq!(
            result.unwrap_err().to_string(),
            "liquid: Expected `:`, found `1`\n"
        );
    }
}

#[cfg(test)]
mod test_expect {
    use super::*;

    #[test]
    fn rejects_unexpected_token() {
        let token_vec = vec![Token::Pipe, Token::Dot, Token::Colon];
        let mut tokens = token_vec.iter();

        assert!(expect(&mut tokens, &Token::Pipe).is_ok());
        assert!(expect(&mut tokens, &Token::Dot).is_ok());
        assert!(expect(&mut tokens, &Token::Comma).is_err());
    }
}

#[cfg(test)]
mod test_split_block {
    use super::*;

    use std::collections::HashMap;
    use std::io::Write;

    use liquid_interpreter;
    use liquid_interpreter::Context;
    use liquid_interpreter::Renderable;

    use super::super::split_block;
    use super::super::tokenize;
    use super::super::BoxedBlockParser;
    use super::super::FnParseBlock;

    #[derive(Debug)]
    struct NullBlock;

    impl Renderable for NullBlock {
        fn render_to(&self, _writer: &mut Write, _context: &mut Context) -> Result<()> {
            Ok(())
        }
    }

    fn null_block(
        _tag_name: &str,
        _arguments: &[Token],
        _tokens: &[Element],
        _options: &LiquidOptions,
    ) -> Result<Box<Renderable>> {
        Ok(Box::new(NullBlock))
    }

    fn options() -> LiquidOptions {
        let mut options = LiquidOptions::default();
        let blocks: [&'static str; 3] = ["comment", "for", "if"];
        let blocks: HashMap<&'static str, BoxedBlockParser> = blocks
            .into_iter()
            .map(|name| (*name, (null_block as FnParseBlock).into()))
            .collect();
        options.blocks = blocks;
        options
    }

    #[test]
    fn parse_empty_expression() {
        let text = "{{}}";

        let tokens = tokenize(&text).unwrap();
        let template = parse(&tokens, &options()).map(liquid_interpreter::Template::new);
        assert!(template.is_err());
    }

    #[test]
    fn handles_nonmatching_stream() {
        // A stream of tokens with lots of `else`s in it, but only one at the
        // top level, which is where it should split.
        let tokens = tokenize(
            "{% comment %}A{%endcomment%} bunch of {{text}} with {{no}} \
             else tag",
        ).unwrap();

        // note that we need an options block that has been initilaised with
        // the supported block list; otherwise the split_tag function won't know
        // which things start a nested block.
        let options = options();
        let (_, trailing) = split_block(&tokens[..], &["else"], &options);
        assert!(trailing.is_none());
    }

    #[test]
    fn honours_nesting() {
        // A stream of tokens with lots of `else`s in it, but only one at the
        // top level, which is where it should split.
        let tokens = tokenize(concat!(
            "{% for x in (1..10) %}",
            "{% if x == 2 %}",
            "{% for y (2..10) %}{{y}}{% else %} zz {% endfor %}",
            "{% else %}",
            "c",
            "{% endif %}",
            "{% else %}",
            "something",
            "{% endfor %}",
            "{% else %}",
            "trailing tags"
        )).unwrap();

        // note that we need an options block that has been initilaised with
        // the supported block list; otherwise the split_tag function won't know
        // which things start a nested block.
        let options = options();
        let (_, trailing) = split_block(&tokens[..], &["else"], &options);
        match trailing {
            Some(split) => {
                assert_eq!(split.delimiter, "else");
                assert_eq!(split.args, &[Token::Identifier("else".to_owned())]);
                assert_eq!(
                    split.trailing,
                    &[
                        Element::Tag(
                            vec![Token::Identifier("else".to_owned())],
                            "{% else %}".to_owned()
                        ),
                        Element::Raw("trailing tags".to_owned())
                    ]
                );
            }
            None => panic!("split failed"),
        }
    }
}
