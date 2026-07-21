//! Single-pass recursive-descent and Pratt parser.

mod context;

pub use context::{Diagnostic, Severity};

use std::{error::Error, fmt};

use crate::{
    Language, ParseOptions, SourceKind,
    lexer::{Lexer, Token, TokenKind},
    operator::{
        AssignmentOperator, UnaryOperator, UpdateOperator, assignment_operator, binary_binding,
        unary_operator, update_operator,
    },
    tape::{FrozenTape, NodeRef, NodeTag, Span, TapeBuilder, TapeError, ValueRef},
};

use self::context::{BindingKind, GrammarContext, LabelKind, ParserContext, ScopeKind};

/// Successful native parse output.
#[derive(Debug)]
pub struct ParseResult {
    pub tape: FrozenTape,
    pub diagnostics: Vec<Diagnostic>,
}

/// An input or wire-format limit prevented parsing.
#[derive(Debug)]
pub enum ParseError {
    SourceTooLarge,
    Tape(TapeError),
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceTooLarge => formatter.write_str("source exceeds the four-GiB wire limit"),
            Self::Tape(error) => error.fmt(formatter),
        }
    }
}

impl Error for ParseError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::SourceTooLarge => None,
            Self::Tape(error) => Some(error),
        }
    }
}

impl From<TapeError> for ParseError {
    fn from(error: TapeError) -> Self {
        Self::Tape(error)
    }
}

/// Parse source directly into JetSyntax's owned postfix tape.
pub fn parse(source: &str, options: ParseOptions) -> Result<ParseResult, ParseError> {
    let source_len = u32::try_from(source.len()).map_err(|_| ParseError::SourceTooLarge)?;
    Parser::new(source, source_len, options).parse_program()
}

#[derive(Clone, Copy)]
struct ParsedNode {
    node: NodeRef,
    span: Span,
}

impl ParsedNode {
    const fn value(self) -> ValueRef {
        self.node.as_value()
    }
}

struct Parser<'s> {
    source: &'s str,
    lexer: Lexer<'s>,
    current: Token,
    tape: TapeBuilder,
    context: ParserContext<'s>,
    options: ParseOptions,
    function_depth: u32,
}

impl<'s> Parser<'s> {
    fn new(source: &'s str, source_len: u32, options: ParseOptions) -> Self {
        let mut lexer = Lexer::new(source);
        let current = lexer.next_token();
        let module = matches!(options.source_kind, SourceKind::Module);
        let ambient = matches!(options.language, Language::TypeScriptDefinition);
        let grammar = GrammarContext::new(module, ambient);
        Self {
            source,
            lexer,
            current,
            tape: TapeBuilder::new(source_len),
            context: ParserContext::new(grammar),
            options,
            function_depth: 0,
        }
    }

    fn parse_program(mut self) -> Result<ParseResult, ParseError> {
        self.context.enter_scope(ScopeKind::Program);
        let mut body = Vec::new();
        while self.current.kind != TokenKind::Eof {
            let before = self.current.start;
            body.push(self.parse_statement()?.value());
            if self.current.kind != TokenKind::Eof && self.current.start == before {
                self.error(
                    self.current_span(),
                    "parser recovery consumed an unexpected token",
                );
                self.bump();
            }
        }
        let _ = self.context.leave_scope();

        for error in self.lexer.errors() {
            self.context
                .error(Span::new(error.start, error.end), error.message);
        }

        let body = self.tape.push_list(&body)?;
        let source_type = self.tape.push_u32(match self.options.source_kind {
            SourceKind::Script => 0,
            SourceKind::Module | SourceKind::Unambiguous => 1,
            SourceKind::CommonJs => 2,
        })?;
        let root = self.tape.push_node(
            NodeTag::PROGRAM,
            Span::new(0, u32::try_from(self.source.len()).unwrap_or(u32::MAX)),
            0,
            &[body, source_type],
        )?;
        let tape = self.tape.finish(root)?;
        Ok(ParseResult {
            tape,
            diagnostics: self.context.take_diagnostics(),
        })
    }

    fn parse_statement(&mut self) -> Result<ParsedNode, ParseError> {
        match self.current.kind {
            TokenKind::Semicolon => self.parse_empty_statement(),
            TokenKind::LeftBrace => self.parse_block_statement(),
            TokenKind::Var | TokenKind::Let | TokenKind::Const => {
                self.parse_variable_declaration(true)
            }
            TokenKind::Function => self.parse_function(true, false),
            TokenKind::Async if self.followed_by_word("function") => {
                self.bump();
                self.parse_function(true, true)
            }
            TokenKind::Return => self.parse_return_statement(),
            TokenKind::Throw => self.parse_throw_statement(),
            TokenKind::If => self.parse_if_statement(),
            TokenKind::While => self.parse_while_statement(),
            TokenKind::Do => self.parse_do_while_statement(),
            TokenKind::For => self.parse_for_statement(),
            TokenKind::Switch => self.parse_switch_statement(),
            TokenKind::Try => self.parse_try_statement(),
            TokenKind::Class => self.parse_class(true),
            TokenKind::Import => self.parse_import_declaration(),
            TokenKind::Export => self.parse_export_declaration(),
            TokenKind::Break => self.parse_jump_statement(false),
            TokenKind::Continue => self.parse_jump_statement(true),
            TokenKind::Debugger => self.parse_debugger_statement(),
            TokenKind::With => self.parse_with_statement(),
            _ => self.parse_expression_or_labeled_statement(),
        }
    }

    fn parse_empty_statement(&mut self) -> Result<ParsedNode, ParseError> {
        let token = self.take();
        self.node(NodeTag::EMPTY_STATEMENT, self.token_span(token), &[])
    }

    fn parse_debugger_statement(&mut self) -> Result<ParsedNode, ParseError> {
        let start = self.take().start;
        let end = self.consume_semicolon();
        self.node(NodeTag::DEBUGGER_STATEMENT, Span::new(start, end), &[])
    }

    fn parse_block_statement(&mut self) -> Result<ParsedNode, ParseError> {
        let start = self.expect(TokenKind::LeftBrace).start;
        self.context.enter_scope(ScopeKind::Block);
        let mut body = Vec::new();
        while !matches!(self.current.kind, TokenKind::RightBrace | TokenKind::Eof) {
            let before = self.current.start;
            body.push(self.parse_statement()?.value());
            if self.current.start == before {
                self.bump();
            }
        }
        let end = self.expect(TokenKind::RightBrace).end;
        let _ = self.context.leave_scope();
        let body = self.tape.push_list(&body)?;
        self.node(NodeTag::BLOCK_STATEMENT, Span::new(start, end), &[body])
    }

    fn parse_variable_declaration(
        &mut self,
        consume_semicolon: bool,
    ) -> Result<ParsedNode, ParseError> {
        let keyword = self.take();
        let (kind, binding_kind) = match keyword.kind {
            TokenKind::Var => (0, BindingKind::Var),
            TokenKind::Let => (1, BindingKind::Lexical),
            TokenKind::Const => (2, BindingKind::Lexical),
            _ => (0, BindingKind::Var),
        };
        let mut declarations = Vec::new();
        let mut end = keyword.end;
        loop {
            let id = self.parse_binding_pattern(binding_kind)?;
            let init = if self.eat(TokenKind::Eq).is_some() {
                self.parse_assignment_expression(true)?.value()
            } else {
                self.tape.push_null()?
            };
            end = self.previous_end(end);
            let declarator = self.node(
                NodeTag::VARIABLE_DECLARATOR,
                Span::new(id.span.start, end),
                &[id.value(), init],
            )?;
            declarations.push(declarator.value());
            if self.eat(TokenKind::Comma).is_none() {
                break;
            }
        }
        if consume_semicolon {
            end = self.consume_semicolon();
        }
        let declarations = self.tape.push_list(&declarations)?;
        let kind = self.tape.push_u32(kind)?;
        self.node(
            NodeTag::VARIABLE_DECLARATION,
            Span::new(keyword.start, end),
            &[declarations, kind],
        )
    }

    fn parse_function(
        &mut self,
        declaration: bool,
        asynchronous: bool,
    ) -> Result<ParsedNode, ParseError> {
        let start = self.expect(TokenKind::Function).start;
        let generator = self.eat(TokenKind::Star).is_some();
        let id = if self.is_identifier_name(self.current.kind) {
            self.parse_binding_identifier(BindingKind::Function)?
                .value()
        } else {
            if declaration {
                self.error(self.current_span(), "function declaration requires a name");
            }
            self.tape.push_null()?
        };
        self.expect(TokenKind::LeftParen);
        let params = self.parse_parameter_list()?;
        self.expect(TokenKind::RightParen);
        self.function_depth = self.function_depth.saturating_add(1);
        self.context.enter_scope(ScopeKind::Function);
        let previous_grammar = self.context.grammar();
        self.context.set_grammar(
            previous_grammar
                .with_function(true)
                .with_generator(generator)
                .with_async_function(asynchronous)
                .with_allow_yield(generator)
                .with_allow_await(asynchronous),
        );
        let body = self.parse_block_statement()?;
        self.context.set_grammar(previous_grammar);
        let _ = self.context.leave_scope();
        self.function_depth = self.function_depth.saturating_sub(1);
        let generator = self.tape.push_bool(generator)?;
        let asynchronous = self.tape.push_bool(asynchronous)?;
        self.node(
            if declaration {
                NodeTag::FUNCTION_DECLARATION
            } else {
                NodeTag::FUNCTION_EXPRESSION
            },
            Span::new(start, body.span.end),
            &[id, params, body.value(), generator, asynchronous],
        )
    }

    fn parse_parameter_list(&mut self) -> Result<ValueRef, ParseError> {
        let mut params = Vec::new();
        while !matches!(self.current.kind, TokenKind::RightParen | TokenKind::Eof) {
            let parameter = if self.eat(TokenKind::Ellipsis).is_some() {
                let argument = self.parse_binding_pattern(BindingKind::Parameter)?;
                self.node(NodeTag::REST_ELEMENT, argument.span, &[argument.value()])?
            } else {
                self.parse_binding_pattern(BindingKind::Parameter)?
            };
            params.push(parameter.value());
            if self.eat(TokenKind::Comma).is_none() {
                break;
            }
        }
        Ok(self.tape.push_list(&params)?)
    }

    fn parse_class(&mut self, declaration: bool) -> Result<ParsedNode, ParseError> {
        let start = self.expect(TokenKind::Class).start;
        let id = if self.is_identifier_name(self.current.kind) {
            self.parse_binding_identifier(BindingKind::Lexical)?.value()
        } else {
            if declaration {
                self.error(self.current_span(), "class declaration requires a name");
            }
            self.tape.push_null()?
        };
        let super_class = if self.eat(TokenKind::Extends).is_some() {
            self.parse_assignment_expression(true)?.value()
        } else {
            self.tape.push_null()?
        };
        let body_start = self.expect(TokenKind::LeftBrace).start;
        self.context.enter_scope(ScopeKind::Class);
        let previous_grammar = self.context.grammar();
        self.context
            .set_grammar(previous_grammar.with_class(true).with_strict(true));
        let mut elements = Vec::new();
        while !matches!(self.current.kind, TokenKind::RightBrace | TokenKind::Eof) {
            if self.eat(TokenKind::Semicolon).is_some() {
                continue;
            }
            elements.push(self.parse_class_element()?.value());
        }
        let end = self.expect(TokenKind::RightBrace).end;
        self.context.set_grammar(previous_grammar);
        let _ = self.context.leave_scope();
        let elements = self.tape.push_list(&elements)?;
        let body = self.node(NodeTag::CLASS_BODY, Span::new(body_start, end), &[elements])?;
        self.node(
            if declaration {
                NodeTag::CLASS_DECLARATION
            } else {
                NodeTag::CLASS_EXPRESSION
            },
            Span::new(start, end),
            &[id, super_class, body.value()],
        )
    }

    fn parse_class_element(&mut self) -> Result<ParsedNode, ParseError> {
        let start = self.current.start;
        let is_static = if self.current.kind == TokenKind::Static {
            self.bump();
            true
        } else {
            false
        };
        if is_static && self.current.kind == TokenKind::LeftBrace {
            let block = self.parse_block_statement()?;
            return self.node(
                NodeTag::STATIC_BLOCK,
                Span::new(start, block.span.end),
                &[block.value()],
            );
        }
        let computed = self.eat(TokenKind::LeftBracket).is_some();
        let key = if self.current.kind == TokenKind::PrivateIdentifier {
            let token = self.take();
            let name_span = Span::new(token.start.saturating_add(1), token.end);
            let name_text = self
                .source
                .get(name_span.start as usize..name_span.end as usize)
                .unwrap_or_default();
            let _ = self.context.declare_private(name_text, name_span);
            let name = self.tape.push_source_slice(name_span)?;
            self.node(NodeTag::PRIVATE_IDENTIFIER, self.token_span(token), &[name])?
        } else if matches!(self.current.kind, TokenKind::String | TokenKind::Number) {
            self.parse_literal()?
        } else {
            self.parse_identifier()?
        };
        if computed {
            self.expect(TokenKind::RightBracket);
        }
        if self.current.kind == TokenKind::LeftParen {
            self.bump();
            let params = self.parse_parameter_list()?;
            self.expect(TokenKind::RightParen);
            self.function_depth = self.function_depth.saturating_add(1);
            let body = self.parse_block_statement()?;
            self.function_depth = self.function_depth.saturating_sub(1);
            let id = self.tape.push_null()?;
            let generator = self.tape.push_bool(false)?;
            let asynchronous = self.tape.push_bool(false)?;
            let function = self.node(
                NodeTag::FUNCTION_EXPRESSION,
                Span::new(key.span.start, body.span.end),
                &[id, params, body.value(), generator, asynchronous],
            )?;
            let kind = self.tape.push_u32(0)?;
            let computed = self.tape.push_bool(computed)?;
            let is_static = self.tape.push_bool(is_static)?;
            return self.node(
                NodeTag::METHOD_DEFINITION,
                Span::new(start, body.span.end),
                &[key.value(), function.value(), kind, computed, is_static],
            );
        }

        let type_annotation =
            if self.options.language.is_typescript() && self.eat(TokenKind::Colon).is_some() {
                self.parse_type_annotation()?.value()
            } else {
                self.tape.push_null()?
            };
        let value = if self.eat(TokenKind::Eq).is_some() {
            self.parse_assignment_expression(true)?.value()
        } else {
            self.tape.push_null()?
        };
        let end = self.consume_semicolon();
        let computed = self.tape.push_bool(computed)?;
        let is_static = self.tape.push_bool(is_static)?;
        self.node(
            NodeTag::PROPERTY_DEFINITION,
            Span::new(start, end),
            &[key.value(), value, computed, is_static, type_annotation],
        )
    }

    fn parse_import_declaration(&mut self) -> Result<ParsedNode, ParseError> {
        let start = self.expect(TokenKind::Import).start;
        if self.eat(TokenKind::LeftParen).is_some() {
            let source = self.parse_assignment_expression(true)?;
            let options = if self.eat(TokenKind::Comma).is_some() {
                self.parse_assignment_expression(true)?.value()
            } else {
                self.tape.push_null()?
            };
            let end = self.expect(TokenKind::RightParen).end;
            let expression = self.node(
                NodeTag::IMPORT_EXPRESSION,
                Span::new(start, end),
                &[source.value(), options],
            )?;
            let end = self.consume_semicolon();
            return self.node(
                NodeTag::EXPRESSION_STATEMENT,
                Span::new(start, end),
                &[expression.value()],
            );
        }

        let mut specifiers = Vec::new();
        let source = if self.current.kind == TokenKind::String {
            self.parse_literal()?
        } else {
            if self.is_identifier_name(self.current.kind) {
                let local = self.parse_binding_identifier(BindingKind::Import)?;
                specifiers.push(
                    self.node(
                        NodeTag::IMPORT_DEFAULT_SPECIFIER,
                        local.span,
                        &[local.value()],
                    )?
                    .value(),
                );
                let _ = self.eat(TokenKind::Comma);
            }
            if self.eat(TokenKind::Star).is_some() {
                self.expect(TokenKind::As);
                let local = self.parse_binding_identifier(BindingKind::Import)?;
                specifiers.push(
                    self.node(
                        NodeTag::IMPORT_NAMESPACE_SPECIFIER,
                        local.span,
                        &[local.value()],
                    )?
                    .value(),
                );
            } else if self.eat(TokenKind::LeftBrace).is_some() {
                while !matches!(self.current.kind, TokenKind::RightBrace | TokenKind::Eof) {
                    let imported = self.parse_identifier()?;
                    let local = if self.eat(TokenKind::As).is_some() {
                        self.parse_binding_identifier(BindingKind::Import)?
                    } else {
                        self.identifier_from_span(imported.span)?
                    };
                    let import_kind = self.tape.push_u32(0)?;
                    specifiers.push(
                        self.node(
                            NodeTag::IMPORT_SPECIFIER,
                            Span::new(imported.span.start, local.span.end),
                            &[imported.value(), local.value(), import_kind],
                        )?
                        .value(),
                    );
                    if self.eat(TokenKind::Comma).is_none() {
                        break;
                    }
                }
                self.expect(TokenKind::RightBrace);
            }
            self.expect(TokenKind::From);
            self.parse_literal()?
        };
        let end = self.consume_semicolon();
        let specifiers = self.tape.push_list(&specifiers)?;
        let attributes = self.tape.push_list(&[])?;
        let import_kind = self.tape.push_u32(0)?;
        self.node(
            NodeTag::IMPORT_DECLARATION,
            Span::new(start, end),
            &[specifiers, source.value(), attributes, import_kind],
        )
    }

    fn parse_export_declaration(&mut self) -> Result<ParsedNode, ParseError> {
        let start = self.expect(TokenKind::Export).start;
        if self.eat(TokenKind::Default).is_some() {
            let (declaration, needs_semicolon) = match self.current.kind {
                TokenKind::Function => (self.parse_function(true, false)?, false),
                TokenKind::Class => (self.parse_class(true)?, false),
                _ => (self.parse_assignment_expression(true)?, true),
            };
            let _ = self.context.declare_export("default", declaration.span);
            let end = if needs_semicolon {
                self.consume_semicolon()
            } else {
                declaration.span.end
            };
            return self.node(
                NodeTag::EXPORT_DEFAULT_DECLARATION,
                Span::new(start, end),
                &[declaration.value()],
            );
        }
        if self.eat(TokenKind::Star).is_some() {
            let exported = if self.eat(TokenKind::As).is_some() {
                self.parse_identifier()?.value()
            } else {
                self.tape.push_null()?
            };
            self.expect(TokenKind::From);
            let source = self.parse_literal()?;
            let end = self.consume_semicolon();
            let attributes = self.tape.push_list(&[])?;
            let export_kind = self.tape.push_u32(0)?;
            return self.node(
                NodeTag::EXPORT_ALL_DECLARATION,
                Span::new(start, end),
                &[source.value(), exported, attributes, export_kind],
            );
        }

        if self.eat(TokenKind::LeftBrace).is_some() {
            let mut specifiers = Vec::new();
            while !matches!(self.current.kind, TokenKind::RightBrace | TokenKind::Eof) {
                let local = self.parse_identifier()?;
                let exported = if self.eat(TokenKind::As).is_some() {
                    self.parse_identifier()?
                } else {
                    self.identifier_from_span(local.span)?
                };
                if let Some(name) = self
                    .source
                    .get(exported.span.start as usize..exported.span.end as usize)
                {
                    let _ = self.context.declare_export(name, exported.span);
                }
                specifiers.push(
                    self.node(
                        NodeTag::EXPORT_SPECIFIER,
                        Span::new(local.span.start, exported.span.end),
                        &[local.value(), exported.value()],
                    )?
                    .value(),
                );
                if self.eat(TokenKind::Comma).is_none() {
                    break;
                }
            }
            self.expect(TokenKind::RightBrace);
            let source = if self.eat(TokenKind::From).is_some() {
                self.parse_literal()?.value()
            } else {
                self.tape.push_null()?
            };
            let end = self.consume_semicolon();
            let declaration = self.tape.push_null()?;
            let specifiers = self.tape.push_list(&specifiers)?;
            let attributes = self.tape.push_list(&[])?;
            let export_kind = self.tape.push_u32(0)?;
            return self.node(
                NodeTag::EXPORT_NAMED_DECLARATION,
                Span::new(start, end),
                &[declaration, specifiers, source, attributes, export_kind],
            );
        }

        let declaration = match self.current.kind {
            TokenKind::Var | TokenKind::Let | TokenKind::Const => {
                self.parse_variable_declaration(true)?
            }
            TokenKind::Function => self.parse_function(true, false)?,
            TokenKind::Class => self.parse_class(true)?,
            _ => {
                self.error(self.current_span(), "expected an export declaration");
                self.parse_statement()?
            }
        };
        let specifiers = self.tape.push_list(&[])?;
        let source = self.tape.push_null()?;
        let attributes = self.tape.push_list(&[])?;
        let export_kind = self.tape.push_u32(0)?;
        self.node(
            NodeTag::EXPORT_NAMED_DECLARATION,
            Span::new(start, declaration.span.end),
            &[
                declaration.value(),
                specifiers,
                source,
                attributes,
                export_kind,
            ],
        )
    }

    fn parse_return_statement(&mut self) -> Result<ParsedNode, ParseError> {
        let keyword = self.take();
        if self.function_depth == 0 && !self.options.allow_return_outside_function {
            self.error(
                self.token_span(keyword),
                "return is only valid inside a function",
            );
        }
        let argument = if self.current.flags.line_break_before()
            || matches!(
                self.current.kind,
                TokenKind::Semicolon | TokenKind::RightBrace | TokenKind::Eof
            ) {
            self.tape.push_null()?
        } else {
            self.parse_expression(true)?.value()
        };
        let end = self.consume_semicolon();
        self.node(
            NodeTag::RETURN_STATEMENT,
            Span::new(keyword.start, end),
            &[argument],
        )
    }

    fn parse_throw_statement(&mut self) -> Result<ParsedNode, ParseError> {
        let keyword = self.take();
        let argument = if self.current.flags.line_break_before() {
            self.error(self.current_span(), "line break is not allowed after throw");
            self.invalid_expression()?
        } else {
            self.parse_expression(true)?
        };
        let end = self.consume_semicolon();
        self.node(
            NodeTag::THROW_STATEMENT,
            Span::new(keyword.start, end),
            &[argument.value()],
        )
    }

    fn parse_if_statement(&mut self) -> Result<ParsedNode, ParseError> {
        let start = self.take().start;
        self.expect(TokenKind::LeftParen);
        let test = self.parse_expression(true)?;
        self.expect(TokenKind::RightParen);
        let consequent = self.parse_statement()?;
        let alternate = if self.eat(TokenKind::Else).is_some() {
            self.parse_statement()?.value()
        } else {
            self.tape.push_null()?
        };
        let end = self.previous_end(consequent.span.end);
        self.node(
            NodeTag::IF_STATEMENT,
            Span::new(start, end),
            &[test.value(), consequent.value(), alternate],
        )
    }

    fn parse_while_statement(&mut self) -> Result<ParsedNode, ParseError> {
        let start = self.take().start;
        self.expect(TokenKind::LeftParen);
        let test = self.parse_expression(true)?;
        self.expect(TokenKind::RightParen);
        self.context
            .push_label(None, LabelKind::Loop, Span::new(start, start));
        let body = self.parse_statement()?;
        let _ = self.context.pop_label();
        self.node(
            NodeTag::WHILE_STATEMENT,
            Span::new(start, body.span.end),
            &[test.value(), body.value()],
        )
    }

    fn parse_do_while_statement(&mut self) -> Result<ParsedNode, ParseError> {
        let start = self.take().start;
        self.context
            .push_label(None, LabelKind::Loop, Span::new(start, start));
        let body = self.parse_statement()?;
        let _ = self.context.pop_label();
        self.expect(TokenKind::While);
        self.expect(TokenKind::LeftParen);
        let test = self.parse_expression(true)?;
        self.expect(TokenKind::RightParen);
        let end = self.consume_semicolon();
        self.node(
            NodeTag::DO_WHILE_STATEMENT,
            Span::new(start, end),
            &[body.value(), test.value()],
        )
    }

    fn parse_for_statement(&mut self) -> Result<ParsedNode, ParseError> {
        let start = self.take().start;
        let asynchronous = self.eat(TokenKind::Await).is_some();
        self.expect(TokenKind::LeftParen);
        let init = if matches!(
            self.current.kind,
            TokenKind::Var | TokenKind::Let | TokenKind::Const
        ) {
            self.parse_variable_declaration(false)?.value()
        } else if self.current.kind == TokenKind::Semicolon {
            self.tape.push_null()?
        } else {
            self.parse_expression(false)?.value()
        };

        if matches!(self.current.kind, TokenKind::In | TokenKind::Of) {
            let operator = self.take();
            let right = self.parse_expression(true)?;
            self.expect(TokenKind::RightParen);
            self.context
                .push_label(None, LabelKind::Loop, Span::new(start, start));
            let body = self.parse_statement()?;
            let _ = self.context.pop_label();
            let asynchronous = self.tape.push_bool(asynchronous)?;
            return self.node(
                if operator.kind == TokenKind::In {
                    NodeTag::FOR_IN_STATEMENT
                } else {
                    NodeTag::FOR_OF_STATEMENT
                },
                Span::new(start, body.span.end),
                &[init, right.value(), body.value(), asynchronous],
            );
        }

        self.expect(TokenKind::Semicolon);
        let test = if self.current.kind == TokenKind::Semicolon {
            self.tape.push_null()?
        } else {
            self.parse_expression(true)?.value()
        };
        self.expect(TokenKind::Semicolon);
        let update = if self.current.kind == TokenKind::RightParen {
            self.tape.push_null()?
        } else {
            self.parse_expression(true)?.value()
        };
        self.expect(TokenKind::RightParen);
        self.context
            .push_label(None, LabelKind::Loop, Span::new(start, start));
        let body = self.parse_statement()?;
        let _ = self.context.pop_label();
        self.node(
            NodeTag::FOR_STATEMENT,
            Span::new(start, body.span.end),
            &[init, test, update, body.value()],
        )
    }

    fn parse_switch_statement(&mut self) -> Result<ParsedNode, ParseError> {
        let start = self.take().start;
        self.expect(TokenKind::LeftParen);
        let discriminant = self.parse_expression(true)?;
        self.expect(TokenKind::RightParen);
        self.expect(TokenKind::LeftBrace);
        self.context
            .push_label(None, LabelKind::Switch, Span::new(start, start));
        let mut cases = Vec::new();
        while !matches!(self.current.kind, TokenKind::RightBrace | TokenKind::Eof) {
            let case_start = self.current.start;
            let test = if self.eat(TokenKind::Case).is_some() {
                self.parse_expression(true)?.value()
            } else if self.eat(TokenKind::Default).is_some() {
                self.tape.push_null()?
            } else {
                self.error(self.current_span(), "expected `case` or `default`");
                self.bump();
                continue;
            };
            self.expect(TokenKind::Colon);
            let mut consequent = Vec::new();
            while !matches!(
                self.current.kind,
                TokenKind::Case | TokenKind::Default | TokenKind::RightBrace | TokenKind::Eof
            ) {
                consequent.push(self.parse_statement()?.value());
            }
            let end = self.previous_end(case_start);
            let consequent = self.tape.push_list(&consequent)?;
            cases.push(
                self.node(
                    NodeTag::SWITCH_CASE,
                    Span::new(case_start, end),
                    &[test, consequent],
                )?
                .value(),
            );
        }
        let end = self.expect(TokenKind::RightBrace).end;
        let _ = self.context.pop_label();
        let cases = self.tape.push_list(&cases)?;
        self.node(
            NodeTag::SWITCH_STATEMENT,
            Span::new(start, end),
            &[discriminant.value(), cases],
        )
    }

    fn parse_try_statement(&mut self) -> Result<ParsedNode, ParseError> {
        let start = self.take().start;
        let block = self.parse_block_statement()?;
        let (handler, has_handler) = if self.eat(TokenKind::Catch).is_some() {
            let catch_start = self.previous_end(block.span.end);
            let parameter = if self.eat(TokenKind::LeftParen).is_some() {
                let parameter = self.parse_binding_identifier(BindingKind::Lexical)?;
                self.expect(TokenKind::RightParen);
                parameter.value()
            } else {
                self.tape.push_null()?
            };
            self.context.enter_scope(ScopeKind::Catch);
            let body = self.parse_block_statement()?;
            let _ = self.context.leave_scope();
            (
                self.node(
                    NodeTag::CATCH_CLAUSE,
                    Span::new(catch_start, body.span.end),
                    &[parameter, body.value()],
                )?
                .value(),
                true,
            )
        } else {
            (self.tape.push_null()?, false)
        };
        let (finalizer, has_finalizer) = if self.eat(TokenKind::Finally).is_some() {
            (self.parse_block_statement()?.value(), true)
        } else {
            (self.tape.push_null()?, false)
        };
        if !has_handler && !has_finalizer {
            self.error(
                Span::new(start, block.span.end),
                "try requires catch or finally",
            );
        }
        let end = self.previous_end(block.span.end);
        self.node(
            NodeTag::TRY_STATEMENT,
            Span::new(start, end),
            &[block.value(), handler, finalizer],
        )
    }

    fn parse_jump_statement(&mut self, is_continue: bool) -> Result<ParsedNode, ParseError> {
        let keyword = self.take();
        let label_name = if !self.current.flags.line_break_before()
            && self.is_identifier_name(self.current.kind)
        {
            self.source
                .get(self.current.start as usize..self.current.end as usize)
        } else {
            None
        };
        let resolved = if is_continue {
            self.context.resolve_continue(label_name)
        } else {
            self.context.resolve_break(label_name)
        };
        if !resolved {
            self.error(
                self.token_span(keyword),
                if is_continue {
                    "continue does not target an enclosing loop"
                } else {
                    "break does not target an enclosing loop, switch, or label"
                },
            );
        }
        let label = if label_name.is_some() {
            self.parse_identifier()?.value()
        } else {
            self.tape.push_null()?
        };
        let end = self.consume_semicolon();
        self.node(
            if is_continue {
                NodeTag::CONTINUE_STATEMENT
            } else {
                NodeTag::BREAK_STATEMENT
            },
            Span::new(keyword.start, end),
            &[label],
        )
    }

    fn parse_with_statement(&mut self) -> Result<ParsedNode, ParseError> {
        let start = self.take().start;
        if self.context.grammar().strict() {
            self.error(
                Span::new(start, start + 4),
                "with is forbidden in strict mode",
            );
        }
        self.expect(TokenKind::LeftParen);
        let object = self.parse_expression(true)?;
        self.expect(TokenKind::RightParen);
        let body = self.parse_statement()?;
        self.node(
            NodeTag::WITH_STATEMENT,
            Span::new(start, body.span.end),
            &[object.value(), body.value()],
        )
    }

    fn parse_expression_or_labeled_statement(&mut self) -> Result<ParsedNode, ParseError> {
        let expression = self.parse_expression(true)?;
        if self.eat(TokenKind::Colon).is_some() {
            let name = self
                .source
                .get(expression.span.start as usize..expression.span.end as usize);
            let kind = if matches!(
                self.current.kind,
                TokenKind::While | TokenKind::Do | TokenKind::For
            ) {
                LabelKind::Loop
            } else {
                LabelKind::Statement
            };
            self.context.push_label(name, kind, expression.span);
            let body = self.parse_statement()?;
            let _ = self.context.pop_label();
            return self.node(
                NodeTag::LABELED_STATEMENT,
                Span::new(expression.span.start, body.span.end),
                &[expression.value(), body.value()],
            );
        }
        let end = self.consume_semicolon();
        self.node(
            NodeTag::EXPRESSION_STATEMENT,
            Span::new(expression.span.start, end),
            &[expression.value()],
        )
    }

    fn parse_expression(&mut self, allow_in: bool) -> Result<ParsedNode, ParseError> {
        let first = self.parse_assignment_expression(allow_in)?;
        if self.eat(TokenKind::Comma).is_none() {
            return Ok(first);
        }
        let mut expressions = vec![first.value()];
        let end = loop {
            let expression = self.parse_assignment_expression(allow_in)?;
            let end = expression.span.end;
            expressions.push(expression.value());
            if self.eat(TokenKind::Comma).is_none() {
                break end;
            }
        };
        let expressions = self.tape.push_list(&expressions)?;
        self.node(
            NodeTag::SEQUENCE_EXPRESSION,
            Span::new(first.span.start, end),
            &[expressions],
        )
    }

    fn parse_assignment_expression(&mut self, allow_in: bool) -> Result<ParsedNode, ParseError> {
        if self.current.kind == TokenKind::Async && self.looks_like_async_arrow() {
            let start = self.take().start;
            let mut parameters = Vec::new();
            if self.eat(TokenKind::LeftParen).is_some() {
                while !matches!(self.current.kind, TokenKind::RightParen | TokenKind::Eof) {
                    parameters.push(
                        self.parse_binding_identifier(BindingKind::Parameter)?
                            .value(),
                    );
                    if self.eat(TokenKind::Comma).is_none() {
                        break;
                    }
                }
                self.expect(TokenKind::RightParen);
            } else {
                parameters.push(
                    self.parse_binding_identifier(BindingKind::Parameter)?
                        .value(),
                );
            }
            self.expect(TokenKind::Arrow);
            let expression_body = self.current.kind != TokenKind::LeftBrace;
            self.function_depth = self.function_depth.saturating_add(1);
            let body = if expression_body {
                self.parse_assignment_expression(allow_in)?
            } else {
                self.parse_block_statement()?
            };
            self.function_depth = self.function_depth.saturating_sub(1);
            let parameters = self.tape.push_list(&parameters)?;
            let asynchronous = self.tape.push_bool(true)?;
            let expression = self.tape.push_bool(expression_body)?;
            return self.node(
                NodeTag::ARROW_FUNCTION_EXPRESSION,
                Span::new(start, body.span.end),
                &[parameters, body.value(), asynchronous, expression],
            );
        }
        let left = self.parse_conditional_expression(allow_in)?;
        if self.eat(TokenKind::Arrow).is_some() {
            let params = self.tape.push_list(&[left.value()])?;
            let expression_body = self.current.kind != TokenKind::LeftBrace;
            let body = if !expression_body {
                self.function_depth = self.function_depth.saturating_add(1);
                let body = self.parse_block_statement()?;
                self.function_depth = self.function_depth.saturating_sub(1);
                body
            } else {
                self.parse_assignment_expression(allow_in)?
            };
            let asynchronous = self.tape.push_bool(false)?;
            let expression = self.tape.push_bool(expression_body)?;
            return self.node(
                NodeTag::ARROW_FUNCTION_EXPRESSION,
                Span::new(left.span.start, body.span.end),
                &[params, body.value(), asynchronous, expression],
            );
        }
        let Some(operator) = assignment_operator(self.current.kind) else {
            return Ok(left);
        };
        self.bump();
        let mut assignment_left = left;
        if operator == AssignmentOperator::Assign {
            let source = self
                .source
                .get(left.span.start as usize..left.span.end as usize)
                .unwrap_or_default();
            if source.contains('[') {
                assignment_left = self.node(
                    NodeTag::ARRAY_PATTERN,
                    left.span,
                    &[assignment_left.value()],
                )?;
            }
            if source.contains('{') {
                assignment_left = self.node(
                    NodeTag::OBJECT_PATTERN,
                    left.span,
                    &[assignment_left.value()],
                )?;
            }
        }
        let right = self.parse_assignment_expression(allow_in)?;
        let operator = self.tape.push_u32(operator as u32)?;
        self.node(
            NodeTag::ASSIGNMENT_EXPRESSION,
            Span::new(left.span.start, right.span.end),
            &[operator, assignment_left.value(), right.value()],
        )
    }

    fn parse_conditional_expression(&mut self, allow_in: bool) -> Result<ParsedNode, ParseError> {
        let test = self.parse_binary_expression(0, allow_in)?;
        if self.eat(TokenKind::Question).is_none() {
            return Ok(test);
        }
        let consequent = self.parse_assignment_expression(true)?;
        self.expect(TokenKind::Colon);
        let alternate = self.parse_assignment_expression(allow_in)?;
        self.node(
            NodeTag::CONDITIONAL_EXPRESSION,
            Span::new(test.span.start, alternate.span.end),
            &[test.value(), consequent.value(), alternate.value()],
        )
    }

    fn parse_binary_expression(
        &mut self,
        minimum: u8,
        allow_in: bool,
    ) -> Result<ParsedNode, ParseError> {
        let mut left = self.parse_unary_expression()?;
        loop {
            let Some(binding) = binary_binding(self.current.kind, allow_in) else {
                break;
            };
            if binding.left < minimum {
                break;
            }
            self.bump();
            let right = self.parse_binary_expression(binding.right, allow_in)?;
            let operator = self.tape.push_u32(binding.operator as u32)?;
            left = self.node(
                if binding.logical {
                    NodeTag::LOGICAL_EXPRESSION
                } else {
                    NodeTag::BINARY_EXPRESSION
                },
                Span::new(left.span.start, right.span.end),
                &[operator, left.value(), right.value()],
            )?;
        }
        Ok(left)
    }

    fn parse_unary_expression(&mut self) -> Result<ParsedNode, ParseError> {
        if let Some(operator) = unary_operator(self.current.kind) {
            let start = self.take().start;
            let argument = self.parse_unary_expression()?;
            if operator == UnaryOperator::Delete && self.context.grammar().strict() {
                let text = self
                    .source
                    .get(argument.span.start as usize..argument.span.end as usize)
                    .unwrap_or_default();
                if text.chars().next().is_some_and(|character| {
                    character == '_' || character == '$' || character.is_alphabetic()
                }) {
                    self.error(
                        argument.span,
                        "deleting an unqualified identifier is forbidden in strict mode",
                    );
                }
            }
            let operator = self.tape.push_u32(operator as u32)?;
            let prefix = self.tape.push_bool(true)?;
            return self.node(
                NodeTag::UNARY_EXPRESSION,
                Span::new(start, argument.span.end),
                &[operator, prefix, argument.value()],
            );
        }
        if let Some(operator) = update_operator(self.current.kind) {
            let start = self.take().start;
            let argument = self.parse_unary_expression()?;
            let operator = self.tape.push_u32(operator as u32)?;
            let prefix = self.tape.push_bool(true)?;
            return self.node(
                NodeTag::UPDATE_EXPRESSION,
                Span::new(start, argument.span.end),
                &[operator, prefix, argument.value()],
            );
        }
        if self.current.kind == TokenKind::Await {
            let start = self.take().start;
            let argument = self.parse_unary_expression()?;
            return self.node(
                NodeTag::AWAIT_EXPRESSION,
                Span::new(start, argument.span.end),
                &[argument.value()],
            );
        }
        if self.current.kind == TokenKind::Yield {
            let start = self.take().start;
            let delegate = self.eat(TokenKind::Star).is_some();
            let argument = if self.current.flags.line_break_before()
                || matches!(
                    self.current.kind,
                    TokenKind::Semicolon | TokenKind::RightBrace | TokenKind::Eof
                ) {
                self.tape.push_null()?
            } else {
                self.parse_assignment_expression(true)?.value()
            };
            let delegate = self.tape.push_bool(delegate)?;
            return self.node(
                NodeTag::YIELD_EXPRESSION,
                Span::new(start, self.previous_end(start)),
                &[argument, delegate],
            );
        }
        self.parse_postfix_expression()
    }

    fn parse_postfix_expression(&mut self) -> Result<ParsedNode, ParseError> {
        let mut expression = self.parse_primary_expression()?;
        let mut is_chain = false;
        loop {
            match self.current.kind {
                TokenKind::Dot => {
                    self.bump();
                    let property = self.parse_identifier()?;
                    let computed = self.tape.push_bool(false)?;
                    let optional = self.tape.push_bool(false)?;
                    expression = self.node(
                        NodeTag::MEMBER_EXPRESSION,
                        Span::new(expression.span.start, property.span.end),
                        &[expression.value(), property.value(), computed, optional],
                    )?;
                }
                TokenKind::QuestionDot => {
                    is_chain = true;
                    self.bump();
                    if self.eat(TokenKind::LeftBracket).is_some() {
                        let property = self.parse_expression(true)?;
                        let end = self.expect(TokenKind::RightBracket).end;
                        let computed = self.tape.push_bool(true)?;
                        let optional = self.tape.push_bool(true)?;
                        expression = self.node(
                            NodeTag::MEMBER_EXPRESSION,
                            Span::new(expression.span.start, end),
                            &[expression.value(), property.value(), computed, optional],
                        )?;
                    } else if self.eat(TokenKind::LeftParen).is_some() {
                        let arguments = self.parse_argument_list()?;
                        let end = self.expect(TokenKind::RightParen).end;
                        let optional = self.tape.push_bool(true)?;
                        expression = self.node(
                            NodeTag::CALL_EXPRESSION,
                            Span::new(expression.span.start, end),
                            &[expression.value(), arguments, optional],
                        )?;
                    } else {
                        let property = self.parse_identifier()?;
                        let computed = self.tape.push_bool(false)?;
                        let optional = self.tape.push_bool(true)?;
                        expression = self.node(
                            NodeTag::MEMBER_EXPRESSION,
                            Span::new(expression.span.start, property.span.end),
                            &[expression.value(), property.value(), computed, optional],
                        )?;
                    }
                }
                TokenKind::LeftBracket => {
                    self.bump();
                    let property = self.parse_expression(true)?;
                    let end = self.expect(TokenKind::RightBracket).end;
                    let computed = self.tape.push_bool(true)?;
                    let optional = self.tape.push_bool(false)?;
                    expression = self.node(
                        NodeTag::MEMBER_EXPRESSION,
                        Span::new(expression.span.start, end),
                        &[expression.value(), property.value(), computed, optional],
                    )?;
                }
                TokenKind::LeftParen => {
                    self.bump();
                    let arguments = self.parse_argument_list()?;
                    let end = self.expect(TokenKind::RightParen).end;
                    let optional = self.tape.push_bool(false)?;
                    expression = self.node(
                        NodeTag::CALL_EXPRESSION,
                        Span::new(expression.span.start, end),
                        &[expression.value(), arguments, optional],
                    )?;
                }
                TokenKind::NoSubstitutionTemplate | TokenKind::TemplateHead => {
                    let quasi = self.parse_template_literal()?;
                    expression = self.node(
                        NodeTag::TAGGED_TEMPLATE_EXPRESSION,
                        Span::new(expression.span.start, quasi.span.end),
                        &[expression.value(), quasi.value()],
                    )?;
                }
                TokenKind::PlusPlus | TokenKind::MinusMinus
                    if !self.current.flags.line_break_before() =>
                {
                    let token = self.take();
                    let operator = update_operator(token.kind).unwrap_or(UpdateOperator::Increment);
                    let operator = self.tape.push_u32(operator as u32)?;
                    let prefix = self.tape.push_bool(false)?;
                    expression = self.node(
                        NodeTag::UPDATE_EXPRESSION,
                        Span::new(expression.span.start, token.end),
                        &[operator, prefix, expression.value()],
                    )?;
                }
                _ => break,
            }
        }
        if is_chain {
            expression = self.node(
                NodeTag::CHAIN_EXPRESSION,
                expression.span,
                &[expression.value()],
            )?;
        }
        Ok(expression)
    }

    fn parse_primary_expression(&mut self) -> Result<ParsedNode, ParseError> {
        match self.current.kind {
            kind if self.is_identifier_name(kind) => self.parse_identifier(),
            TokenKind::Number
            | TokenKind::BigInt
            | TokenKind::String
            | TokenKind::True
            | TokenKind::False
            | TokenKind::Null => self.parse_literal(),
            TokenKind::NoSubstitutionTemplate | TokenKind::TemplateHead => {
                self.parse_template_literal()
            }
            TokenKind::Slash | TokenKind::SlashEq => self.parse_regexp_literal(),
            TokenKind::This => {
                let token = self.take();
                self.node(NodeTag::THIS_EXPRESSION, self.token_span(token), &[])
            }
            TokenKind::Super => {
                let token = self.take();
                self.node(NodeTag::SUPER, self.token_span(token), &[])
            }
            TokenKind::Function => self.parse_function(false, false),
            TokenKind::Class => self.parse_class(false),
            TokenKind::LeftParen => self.parse_parenthesized_expression(),
            TokenKind::LeftBracket => self.parse_array_expression(),
            TokenKind::LeftBrace => self.parse_object_expression(),
            TokenKind::New => self.parse_new_expression(),
            TokenKind::Import => self.parse_import_expression_or_meta(),
            TokenKind::Lt if self.options.language.is_jsx() => self.parse_jsx_element(false),
            _ => self.invalid_expression(),
        }
    }

    fn parse_jsx_element(&mut self, nested: bool) -> Result<ParsedNode, ParseError> {
        let start = self.expect(TokenKind::Lt).start;
        let opening_name = self.parse_jsx_name()?;
        let mut attributes = Vec::new();
        while !matches!(
            self.current.kind,
            TokenKind::Gt | TokenKind::Slash | TokenKind::Eof
        ) {
            if self.eat(TokenKind::LeftBrace).is_some() {
                self.expect(TokenKind::Ellipsis);
                let argument = self.parse_assignment_expression(true)?;
                let end = self.expect(TokenKind::RightBrace).end;
                attributes.push(
                    self.node(
                        NodeTag::JSX_SPREAD_ATTRIBUTE,
                        Span::new(argument.span.start, end),
                        &[argument.value()],
                    )?
                    .value(),
                );
                continue;
            }
            let name = self.parse_jsx_name()?;
            let value = if self.eat(TokenKind::Eq).is_some() {
                if self.current.kind == TokenKind::LeftBrace {
                    let expression_start = self.take().start;
                    let expression = self.parse_expression(true)?;
                    let end = self.expect(TokenKind::RightBrace).end;
                    self.node(
                        NodeTag::JSX_EXPRESSION_CONTAINER,
                        Span::new(expression_start, end),
                        &[expression.value()],
                    )?
                    .value()
                } else {
                    self.parse_literal()?.value()
                }
            } else {
                self.tape.push_null()?
            };
            attributes.push(
                self.node(
                    NodeTag::JSX_ATTRIBUTE,
                    Span::new(name.span.start, self.previous_end(name.span.end)),
                    &[name.value(), value],
                )?
                .value(),
            );
        }

        let self_closing = self.eat(TokenKind::Slash).is_some();
        let greater = self.current;
        if greater.kind != TokenKind::Gt {
            self.error(
                self.current_span(),
                "expected `>` after JSX opening element",
            );
        }
        let attributes = self.tape.push_list(&attributes)?;
        let self_closing_value = self.tape.push_bool(self_closing)?;
        let opening = self.node(
            NodeTag::JSX_OPENING_ELEMENT,
            Span::new(start, greater.end),
            &[opening_name.value(), attributes, self_closing_value],
        )?;
        self.advance_after_jsx_greater(!self_closing || nested);

        if self_closing {
            let closing = self.tape.push_null()?;
            let children = self.tape.push_list(&[])?;
            return self.node(
                NodeTag::JSX_ELEMENT,
                Span::new(start, greater.end),
                &[opening.value(), closing, children],
            );
        }

        let mut children = Vec::new();
        let mut closing = None;
        loop {
            match self.current.kind {
                TokenKind::JsxText => {
                    let text = self.current;
                    if text.start != text.end {
                        let value = self.tape.push_source_slice(self.token_span(text))?;
                        children.push(
                            self.node(NodeTag::JSX_TEXT, self.token_span(text), &[value])?
                                .value(),
                        );
                    }
                    self.current = self.lexer.next_token();
                }
                TokenKind::LeftBrace => {
                    let expression_start = self.take().start;
                    if self.current.kind == TokenKind::RightBrace {
                        let empty = self.node(
                            NodeTag::JSX_EMPTY_EXPRESSION,
                            Span::new(expression_start + 1, self.current.start),
                            &[],
                        )?;
                        let end = self.current.end;
                        self.advance_after_jsx_brace();
                        children.push(
                            self.node(
                                NodeTag::JSX_EXPRESSION_CONTAINER,
                                Span::new(expression_start, end),
                                &[empty.value()],
                            )?
                            .value(),
                        );
                    } else {
                        let expression = self.parse_expression(true)?;
                        let end = self.current.end;
                        self.advance_after_jsx_brace();
                        children.push(
                            self.node(
                                NodeTag::JSX_EXPRESSION_CONTAINER,
                                Span::new(expression_start, end),
                                &[expression.value()],
                            )?
                            .value(),
                        );
                    }
                }
                TokenKind::Lt => {
                    let next = self
                        .source
                        .as_bytes()
                        .get(self.current.end as usize)
                        .copied();
                    if next == Some(b'/') {
                        let closing_start = self.take().start;
                        self.expect(TokenKind::Slash);
                        let name = self.parse_jsx_name()?;
                        let greater = self.current;
                        self.advance_after_jsx_greater(nested);
                        closing = Some(self.node(
                            NodeTag::JSX_CLOSING_ELEMENT,
                            Span::new(closing_start, greater.end),
                            &[name.value()],
                        )?);
                        break;
                    }
                    children.push(self.parse_jsx_element(true)?.value());
                }
                TokenKind::Eof => {
                    self.error(
                        Span::new(start, self.current.end),
                        "unterminated JSX element",
                    );
                    break;
                }
                _ => {
                    self.error(self.current_span(), "unexpected token in JSX children");
                    self.bump();
                }
            }
        }
        let closing_value = if let Some(closing) = closing {
            closing.value()
        } else {
            self.tape.push_null()?
        };
        let end = closing.map_or(greater.end, |node| node.span.end);
        let children = self.tape.push_list(&children)?;
        self.node(
            NodeTag::JSX_ELEMENT,
            Span::new(start, end),
            &[opening.value(), closing_value, children],
        )
    }

    fn parse_jsx_name(&mut self) -> Result<ParsedNode, ParseError> {
        let token = self.take();
        if !self.is_identifier_name(token.kind) {
            self.error(self.token_span(token), "expected a JSX name");
        }
        let name = self.tape.push_source_slice(self.token_span(token))?;
        self.node(NodeTag::JSX_IDENTIFIER, self.token_span(token), &[name])
    }

    fn advance_after_jsx_greater(&mut self, jsx_child_mode: bool) {
        if self.current.kind == TokenKind::Gt {
            if jsx_child_mode {
                self.current = self.lexer.next_jsx_text();
            } else {
                self.current = self.lexer.next_token();
            }
        }
    }

    fn advance_after_jsx_brace(&mut self) {
        if self.current.kind == TokenKind::RightBrace {
            self.current = self.lexer.next_jsx_text();
        } else {
            self.error(self.current_span(), "expected `}` in JSX expression");
        }
    }

    fn parse_parenthesized_expression(&mut self) -> Result<ParsedNode, ParseError> {
        let start = self.take().start;
        let expression = self.parse_expression(true)?;
        let end = self.expect(TokenKind::RightParen).end;
        if self.options.preserve_parentheses {
            self.node(
                NodeTag::PARENTHESIZED_EXPRESSION,
                Span::new(start, end),
                &[expression.value()],
            )
        } else {
            Ok(expression)
        }
    }

    fn parse_array_expression(&mut self) -> Result<ParsedNode, ParseError> {
        let start = self.take().start;
        let mut elements = Vec::new();
        while !matches!(self.current.kind, TokenKind::RightBracket | TokenKind::Eof) {
            if self.eat(TokenKind::Comma).is_some() {
                elements.push(self.tape.push_null()?);
                continue;
            }
            let element = if self.eat(TokenKind::Ellipsis).is_some() {
                let argument = self.parse_assignment_expression(true)?;
                self.node(NodeTag::SPREAD_ELEMENT, argument.span, &[argument.value()])?
            } else {
                self.parse_assignment_expression(true)?
            };
            elements.push(element.value());
            if self.eat(TokenKind::Comma).is_none() {
                break;
            }
        }
        let end = self.expect(TokenKind::RightBracket).end;
        let elements = self.tape.push_list(&elements)?;
        self.node(
            NodeTag::ARRAY_EXPRESSION,
            Span::new(start, end),
            &[elements],
        )
    }

    fn parse_object_expression(&mut self) -> Result<ParsedNode, ParseError> {
        let start = self.take().start;
        let mut properties = Vec::new();
        while !matches!(self.current.kind, TokenKind::RightBrace | TokenKind::Eof) {
            if self.eat(TokenKind::Ellipsis).is_some() {
                let argument = self.parse_assignment_expression(true)?;
                properties.push(
                    self.node(NodeTag::SPREAD_ELEMENT, argument.span, &[argument.value()])?
                        .value(),
                );
            } else {
                let key = if matches!(self.current.kind, TokenKind::String | TokenKind::Number) {
                    self.parse_literal()?
                } else {
                    self.parse_identifier()?
                };
                let (value, shorthand) = if self.eat(TokenKind::Colon).is_some() {
                    (self.parse_assignment_expression(true)?, false)
                } else {
                    (self.identifier_from_span(key.span)?, true)
                };
                let kind = self.tape.push_u32(0)?;
                let method = self.tape.push_bool(false)?;
                let shorthand = self.tape.push_bool(shorthand)?;
                let computed = self.tape.push_bool(false)?;
                properties.push(
                    self.node(
                        NodeTag::PROPERTY,
                        Span::new(key.span.start, value.span.end),
                        &[
                            key.value(),
                            value.value(),
                            kind,
                            method,
                            shorthand,
                            computed,
                        ],
                    )?
                    .value(),
                );
            }
            if self.eat(TokenKind::Comma).is_none() {
                break;
            }
        }
        let end = self.expect(TokenKind::RightBrace).end;
        let properties = self.tape.push_list(&properties)?;
        self.node(
            NodeTag::OBJECT_EXPRESSION,
            Span::new(start, end),
            &[properties],
        )
    }

    fn parse_new_expression(&mut self) -> Result<ParsedNode, ParseError> {
        let start = self.take().start;
        if self.eat(TokenKind::Dot).is_some() {
            let property = self.parse_identifier()?;
            let meta = self.identifier_from_span(Span::new(start, start + 3))?;
            return self.node(
                NodeTag::META_PROPERTY,
                Span::new(start, property.span.end),
                &[meta.value(), property.value()],
            );
        }
        let callee = self.parse_postfix_expression()?;
        let arguments = if self.eat(TokenKind::LeftParen).is_some() {
            let arguments = self.parse_argument_list()?;
            self.expect(TokenKind::RightParen);
            arguments
        } else {
            self.tape.push_list(&[])?
        };
        self.node(
            NodeTag::NEW_EXPRESSION,
            Span::new(start, self.previous_end(callee.span.end)),
            &[callee.value(), arguments],
        )
    }

    fn parse_import_expression_or_meta(&mut self) -> Result<ParsedNode, ParseError> {
        let import = self.take();
        if self.eat(TokenKind::Dot).is_some() {
            let property = self.parse_identifier()?;
            let meta = self.identifier_from_span(self.token_span(import))?;
            return self.node(
                NodeTag::META_PROPERTY,
                Span::new(import.start, property.span.end),
                &[meta.value(), property.value()],
            );
        }
        self.expect(TokenKind::LeftParen);
        let source = self.parse_assignment_expression(true)?;
        let options = if self.eat(TokenKind::Comma).is_some() {
            self.parse_assignment_expression(true)?.value()
        } else {
            self.tape.push_null()?
        };
        let end = self.expect(TokenKind::RightParen).end;
        self.node(
            NodeTag::IMPORT_EXPRESSION,
            Span::new(import.start, end),
            &[source.value(), options],
        )
    }

    fn parse_argument_list(&mut self) -> Result<ValueRef, ParseError> {
        let mut arguments = Vec::new();
        while !matches!(self.current.kind, TokenKind::RightParen | TokenKind::Eof) {
            let argument = if self.eat(TokenKind::Ellipsis).is_some() {
                let argument = self.parse_assignment_expression(true)?;
                self.node(NodeTag::SPREAD_ELEMENT, argument.span, &[argument.value()])?
            } else {
                self.parse_assignment_expression(true)?
            };
            arguments.push(argument.value());
            if self.eat(TokenKind::Comma).is_none() {
                break;
            }
        }
        Ok(self.tape.push_list(&arguments)?)
    }

    fn parse_template_literal(&mut self) -> Result<ParsedNode, ParseError> {
        let first = self.current;
        let mut quasis = Vec::new();
        let mut expressions = Vec::new();
        if first.kind == TokenKind::NoSubstitutionTemplate {
            self.bump();
            let raw = self.tape.push_source_slice(self.token_span(first))?;
            let tail = self.tape.push_bool(true)?;
            quasis.push(
                self.node(
                    NodeTag::TEMPLATE_ELEMENT,
                    self.token_span(first),
                    &[raw, tail],
                )?
                .value(),
            );
            let quasis = self.tape.push_list(&quasis)?;
            let expressions = self.tape.push_list(&expressions)?;
            return self.node(
                NodeTag::TEMPLATE_LITERAL,
                self.token_span(first),
                &[quasis, expressions],
            );
        }

        self.bump();
        let raw = self.tape.push_source_slice(self.token_span(first))?;
        let tail = self.tape.push_bool(false)?;
        quasis.push(
            self.node(
                NodeTag::TEMPLATE_ELEMENT,
                self.token_span(first),
                &[raw, tail],
            )?
            .value(),
        );
        let mut end = first.end;
        loop {
            let expression = self.parse_expression(true)?;
            expressions.push(expression.value());
            let right_brace = self.current;
            if right_brace.kind != TokenKind::RightBrace {
                self.error(self.current_span(), "expected `}` in template substitution");
                break;
            }
            let segment = self.lexer.resume_template(right_brace);
            end = segment.end;
            let is_tail = segment.kind == TokenKind::TemplateTail;
            let raw = self.tape.push_source_slice(self.token_span(segment))?;
            let tail = self.tape.push_bool(is_tail)?;
            quasis.push(
                self.node(
                    NodeTag::TEMPLATE_ELEMENT,
                    self.token_span(segment),
                    &[raw, tail],
                )?
                .value(),
            );
            if is_tail {
                self.current = self.lexer.next_token();
                break;
            }
            self.current = self.lexer.next_token();
        }
        let quasis = self.tape.push_list(&quasis)?;
        let expressions = self.tape.push_list(&expressions)?;
        self.node(
            NodeTag::TEMPLATE_LITERAL,
            Span::new(first.start, end),
            &[quasis, expressions],
        )
    }

    fn parse_literal(&mut self) -> Result<ParsedNode, ParseError> {
        let token = self.take();
        let raw = self.tape.push_source_slice(self.token_span(token))?;
        let kind = self.tape.push_u32(match token.kind {
            TokenKind::Number => 0,
            TokenKind::String => 1,
            TokenKind::True | TokenKind::False => 2,
            TokenKind::Null => 3,
            TokenKind::BigInt => 4,
            TokenKind::NoSubstitutionTemplate => 5,
            _ => 0,
        })?;
        self.node(NodeTag::LITERAL, self.token_span(token), &[raw, kind])
    }

    fn parse_regexp_literal(&mut self) -> Result<ParsedNode, ParseError> {
        let slash = self.current;
        let token = self.lexer.scan_regexp(slash);
        self.current = self.lexer.next_token();
        let raw = self.tape.push_source_slice(self.token_span(token))?;
        let kind = self.tape.push_u32(6)?;
        self.node(NodeTag::LITERAL, self.token_span(token), &[raw, kind])
    }

    fn parse_identifier(&mut self) -> Result<ParsedNode, ParseError> {
        let token = self.take();
        if !self.is_identifier_name(token.kind) {
            self.error(self.token_span(token), "expected an identifier");
        }
        let name = self.tape.push_source_slice(self.token_span(token))?;
        self.node(NodeTag::IDENTIFIER, self.token_span(token), &[name])
    }

    fn identifier_from_span(&mut self, span: Span) -> Result<ParsedNode, ParseError> {
        let name = self.tape.push_source_slice(span)?;
        self.node(NodeTag::IDENTIFIER, span, &[name])
    }

    fn parse_binding_identifier(
        &mut self,
        binding_kind: BindingKind,
    ) -> Result<ParsedNode, ParseError> {
        let token = self.take();
        if !self.is_identifier_name(token.kind) {
            self.error(self.token_span(token), "expected a binding identifier");
        }
        let name_text = self
            .source
            .get(token.start as usize..token.end as usize)
            .unwrap_or_default();
        let _ = self
            .context
            .declare_binding(name_text, binding_kind, self.token_span(token));
        let name = self.tape.push_source_slice(self.token_span(token))?;
        if self.options.language.is_typescript() && self.eat(TokenKind::Colon).is_some() {
            let annotation = self.parse_type_annotation()?;
            let optional = self.tape.push_bool(false)?;
            return self.node(
                NodeTag::IDENTIFIER,
                Span::new(token.start, annotation.span.end),
                &[name, annotation.value(), optional],
            );
        }
        self.node(NodeTag::IDENTIFIER, self.token_span(token), &[name])
    }

    fn parse_binding_pattern(
        &mut self,
        binding_kind: BindingKind,
    ) -> Result<ParsedNode, ParseError> {
        match self.current.kind {
            TokenKind::LeftBracket => {
                let start = self.take().start;
                let mut elements = Vec::new();
                while !matches!(self.current.kind, TokenKind::RightBracket | TokenKind::Eof) {
                    if self.eat(TokenKind::Comma).is_some() {
                        elements.push(self.tape.push_null()?);
                        continue;
                    }
                    let element = if self.eat(TokenKind::Ellipsis).is_some() {
                        let argument = self.parse_binding_pattern(binding_kind)?;
                        self.node(NodeTag::REST_ELEMENT, argument.span, &[argument.value()])?
                    } else {
                        let left = self.parse_binding_pattern(binding_kind)?;
                        if self.eat(TokenKind::Eq).is_some() {
                            let right = self.parse_assignment_expression(true)?;
                            self.node(
                                NodeTag::ASSIGNMENT_PATTERN,
                                Span::new(left.span.start, right.span.end),
                                &[left.value(), right.value()],
                            )?
                        } else {
                            left
                        }
                    };
                    elements.push(element.value());
                    if self.eat(TokenKind::Comma).is_none() {
                        break;
                    }
                }
                let end = self.expect(TokenKind::RightBracket).end;
                let elements = self.tape.push_list(&elements)?;
                self.node(NodeTag::ARRAY_PATTERN, Span::new(start, end), &[elements])
            }
            TokenKind::LeftBrace => {
                let start = self.take().start;
                let mut properties = Vec::new();
                while !matches!(self.current.kind, TokenKind::RightBrace | TokenKind::Eof) {
                    if self.eat(TokenKind::Ellipsis).is_some() {
                        let argument = self.parse_binding_pattern(binding_kind)?;
                        properties.push(
                            self.node(NodeTag::REST_ELEMENT, argument.span, &[argument.value()])?
                                .value(),
                        );
                    } else {
                        let key = self.parse_identifier()?;
                        let mut value = if self.eat(TokenKind::Colon).is_some() {
                            self.parse_binding_pattern(binding_kind)?
                        } else {
                            self.binding_identifier_from_span(key.span, binding_kind)?
                        };
                        if self.eat(TokenKind::Eq).is_some() {
                            let right = self.parse_assignment_expression(true)?;
                            value = self.node(
                                NodeTag::ASSIGNMENT_PATTERN,
                                Span::new(value.span.start, right.span.end),
                                &[value.value(), right.value()],
                            )?;
                        }
                        let property_kind = self.tape.push_u32(0)?;
                        let method = self.tape.push_bool(false)?;
                        let shorthand = self.tape.push_bool(key.span == value.span)?;
                        let computed = self.tape.push_bool(false)?;
                        properties.push(
                            self.node(
                                NodeTag::PROPERTY,
                                Span::new(key.span.start, value.span.end),
                                &[
                                    key.value(),
                                    value.value(),
                                    property_kind,
                                    method,
                                    shorthand,
                                    computed,
                                ],
                            )?
                            .value(),
                        );
                    }
                    if self.eat(TokenKind::Comma).is_none() {
                        break;
                    }
                }
                let end = self.expect(TokenKind::RightBrace).end;
                let properties = self.tape.push_list(&properties)?;
                self.node(
                    NodeTag::OBJECT_PATTERN,
                    Span::new(start, end),
                    &[properties],
                )
            }
            _ => {
                let mut pattern = self.parse_binding_identifier(binding_kind)?;
                if self.eat(TokenKind::Eq).is_some() {
                    let right = self.parse_assignment_expression(true)?;
                    pattern = self.node(
                        NodeTag::ASSIGNMENT_PATTERN,
                        Span::new(pattern.span.start, right.span.end),
                        &[pattern.value(), right.value()],
                    )?;
                }
                Ok(pattern)
            }
        }
    }

    fn binding_identifier_from_span(
        &mut self,
        span: Span,
        binding_kind: BindingKind,
    ) -> Result<ParsedNode, ParseError> {
        let name_text = self
            .source
            .get(span.start as usize..span.end as usize)
            .unwrap_or_default();
        let _ = self.context.declare_binding(name_text, binding_kind, span);
        self.identifier_from_span(span)
    }

    fn parse_type_annotation(&mut self) -> Result<ParsedNode, ParseError> {
        let start = self.current.start;
        let mut type_name = self.parse_identifier()?;
        while self.eat(TokenKind::Dot).is_some() {
            let right = self.parse_identifier()?;
            type_name = self.node(
                NodeTag::TS_QUALIFIED_NAME,
                Span::new(type_name.span.start, right.span.end),
                &[type_name.value(), right.value()],
            )?;
        }
        let parameters = self.tape.push_null()?;
        let reference = self.node(
            NodeTag::TS_TYPE_REFERENCE,
            Span::new(start, type_name.span.end),
            &[type_name.value(), parameters],
        )?;
        self.node(
            NodeTag::TS_TYPE_ANNOTATION,
            reference.span,
            &[reference.value()],
        )
    }

    fn invalid_expression(&mut self) -> Result<ParsedNode, ParseError> {
        let token = self.take();
        self.error(self.token_span(token), "expected an expression");
        let name = self.tape.push_string("<invalid>")?;
        self.node(NodeTag::IDENTIFIER, self.token_span(token), &[name])
    }

    fn node(
        &mut self,
        tag: NodeTag,
        span: Span,
        fields: &[ValueRef],
    ) -> Result<ParsedNode, ParseError> {
        let node = self.tape.push_node(tag, span, 0, fields)?;
        Ok(ParsedNode { node, span })
    }

    fn consume_semicolon(&mut self) -> u32 {
        if let Some(token) = self.eat(TokenKind::Semicolon) {
            token.end
        } else if self.current.kind == TokenKind::RightBrace
            || self.current.kind == TokenKind::Eof
            || self.current.flags.line_break_before()
        {
            self.current.start
        } else {
            self.error(self.current_span(), "expected a semicolon or line break");
            self.current.start
        }
    }

    fn expect(&mut self, kind: TokenKind) -> Token {
        if self.current.kind == kind {
            return self.take();
        }
        let token = self.current;
        self.error(
            self.current_span(),
            format!("expected {kind:?}, found {:?}", self.current.kind),
        );
        token
    }

    fn eat(&mut self, kind: TokenKind) -> Option<Token> {
        (self.current.kind == kind).then(|| self.take())
    }

    fn take(&mut self) -> Token {
        let token = self.current;
        self.bump();
        token
    }

    fn bump(&mut self) {
        self.current = self.lexer.next_token();
    }

    fn previous_end(&self, fallback: u32) -> u32 {
        self.current.start.max(fallback)
    }

    fn current_span(&self) -> Span {
        self.token_span(self.current)
    }

    const fn token_span(&self, token: Token) -> Span {
        Span::new(token.start, token.end)
    }

    fn error(&mut self, span: Span, message: impl Into<String>) {
        self.context.error(span, message);
    }

    fn followed_by_word(&self, word: &str) -> bool {
        self.source
            .get(self.current.end as usize..)
            .is_some_and(|rest| rest.trim_start().starts_with(word))
    }

    fn looks_like_async_arrow(&self) -> bool {
        self.source
            .get(self.current.end as usize..)
            .and_then(|rest| rest.get(..rest.len().min(256)))
            .is_some_and(|rest| {
                let arrow = rest.find("=>");
                let boundary = rest.find([';', '{', '}']);
                arrow.is_some_and(|arrow| boundary.is_none_or(|boundary| arrow < boundary))
            })
    }

    const fn is_identifier_name(&self, kind: TokenKind) -> bool {
        matches!(
            kind,
            TokenKind::Identifier
                | TokenKind::Async
                | TokenKind::Await
                | TokenKind::Let
                | TokenKind::Static
                | TokenKind::Of
                | TokenKind::Get
                | TokenKind::Set
                | TokenKind::As
                | TokenKind::Satisfies
                | TokenKind::Accessor
                | TokenKind::Using
                | TokenKind::Declare
                | TokenKind::Abstract
                | TokenKind::Interface
                | TokenKind::Type
                | TokenKind::Enum
                | TokenKind::Namespace
                | TokenKind::Module
                | TokenKind::Implements
                | TokenKind::Infer
                | TokenKind::Keyof
                | TokenKind::Readonly
                | TokenKind::Unique
                | TokenKind::Unknown
                | TokenKind::Never
                | TokenKind::Any
                | TokenKind::Boolean
                | TokenKind::NumberKeyword
                | TokenKind::StringKeyword
                | TokenKind::Symbol
                | TokenKind::Object
                | TokenKind::Undefined
                | TokenKind::Is
                | TokenKind::Asserts
                | TokenKind::Public
                | TokenKind::Protected
                | TokenKind::Private
                | TokenKind::Override
                | TokenKind::Out
                | TokenKind::Meta
                | TokenKind::From
                | TokenKind::Require
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tape::TapeValue;

    #[test]
    fn emits_program_as_final_postfix_node() {
        let parsed = parse("const answer = 6 * 7;", ParseOptions::default()).expect("parse");
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        let header = parsed.tape.header();
        let root = parsed.tape.value_at(header.root).expect("root");
        assert!(matches!(
            root,
            TapeValue::Node {
                tag: NodeTag::PROGRAM,
                ..
            }
        ));
    }

    #[test]
    fn reports_recoverable_syntax_errors_with_a_valid_tape() {
        let parsed = parse("const = ;", ParseOptions::default()).expect("recover parse");
        assert!(!parsed.diagnostics.is_empty());
        parsed.tape.validate().expect("valid recovery tape");
    }
}
