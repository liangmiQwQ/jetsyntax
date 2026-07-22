//! Single-pass recursive-descent and Pratt parser.

mod context;

pub use context::{Diagnostic, Severity};

use std::{borrow::Cow, error::Error, fmt, iter::Peekable, str::Chars};

use crate::{
    Language, ParseOptions, SourceKind,
    lexer::{Lexer, Token, TokenFlags, TokenKind},
    operator::{
        AssignmentOperator, UnaryOperator, UpdateOperator, assignment_operator, binary_binding,
        unary_operator, update_operator,
    },
    tape::{FrozenTape, NodeRef, NodeTag, Span, TapeBuilder, TapeError, ValueRef},
};

use self::context::{
    BindingKind, GrammarContext, LabelKind, ParserContext, PrivateAccessorKind, ScopeKind,
};

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

/// Parse source directly into `JetSyntax`'s owned postfix tape.
///
/// # Errors
///
/// Returns [`ParseError::SourceTooLarge`] when the source cannot be represented by the wire
/// format, or [`ParseError::Tape`] when the output tape exceeds its representable limits.
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

#[derive(Clone, Copy, Eq, PartialEq)]
enum AssignmentTargetType {
    RestrictedIdentifier,
    Simple,
    WebCompat,
    OptionalChain,
    Invalid,
}

#[derive(Clone, Copy)]
enum AssignmentTargetPolicy {
    Assignment,
    CompoundAssignment,
    LogicalAssignment,
    Update,
    ForInOf,
}

impl AssignmentTargetPolicy {
    const fn allows_web_compat(self) -> bool {
        !matches!(self, Self::LogicalAssignment)
    }

    const fn allows_optional_chain(self) -> bool {
        matches!(
            self,
            Self::Assignment | Self::CompoundAssignment | Self::LogicalAssignment
        )
    }

    const fn diagnostic(self) -> &'static str {
        match self {
            Self::Assignment => "invalid assignment target",
            Self::CompoundAssignment => "invalid compound assignment target",
            Self::LogicalAssignment => "invalid logical assignment target",
            Self::Update => "invalid update target",
            Self::ForInOf => "invalid for-in/of assignment target",
        }
    }
}

struct ParsedPropertyName {
    key: ParsedNode,
    computed: bool,
    shorthand: bool,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum AccessorKind {
    Get,
    Set,
}

impl AccessorKind {
    const fn method_kind(self) -> u32 {
        match self {
            Self::Get => 1,
            Self::Set => 2,
        }
    }

    const fn private_kind(self) -> PrivateAccessorKind {
        match self {
            Self::Get => PrivateAccessorKind::Get,
            Self::Set => PrivateAccessorKind::Set,
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum MethodBodyPolicy {
    Block,
    TypeScriptSignature,
}

#[derive(Clone, Copy)]
struct FunctionFlags {
    generator: bool,
    asynchronous: bool,
}

#[derive(Clone, Copy)]
enum ImportPhase {
    Source,
    Defer,
}

impl ImportPhase {
    const fn name(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Defer => "defer",
        }
    }

    const fn wire_value(self) -> u32 {
        match self {
            Self::Source => 0,
            Self::Defer => 1,
        }
    }
}

struct ParsedParameterList {
    value: ValueRef,
    count: usize,
    has_rest: bool,
    has_trailing_comma: bool,
    simple: bool,
}

struct ParsedParameters {
    values: Vec<ValueRef>,
    has_rest: bool,
    has_trailing_comma: bool,
    simple: bool,
}

#[derive(Clone, Copy)]
enum AccessibilityModifier {
    Public,
    Protected,
    Private,
}

impl AccessibilityModifier {
    const fn wire_value(self) -> u8 {
        match self {
            Self::Public => 1,
            Self::Protected => 2,
            Self::Private => 3,
        }
    }
}

#[derive(Clone, Copy, Default)]
struct TypeScriptModifiers {
    accessibility: Option<AccessibilityModifier>,
    readonly: bool,
    r#override: bool,
}

impl TypeScriptModifiers {
    const fn any(self) -> bool {
        self.accessibility.is_some() || self.readonly || self.r#override
    }

    const fn wire_flags(self) -> u8 {
        let accessibility = match self.accessibility {
            Some(accessibility) => accessibility.wire_value(),
            None => 0,
        };
        accessibility | ((self.readonly as u8) << 2) | ((self.r#override as u8) << 3)
    }
}

#[derive(Clone, Copy)]
struct TypeScriptClassMemberContext {
    modifiers: TypeScriptModifiers,
    class_has_super: bool,
}

#[derive(Clone, Copy)]
struct AssignmentPatternCandidate {
    node: NodeRef,
    tag: NodeTag,
    group_start: usize,
    error: Option<AssignmentPatternError>,
}

#[derive(Clone, Copy)]
enum AssignmentPatternError {
    Accessor(Span),
    InvalidTarget(Span),
}

#[derive(Clone, Copy)]
struct AssignmentPatternCheckpoint {
    candidate_len: usize,
}

struct Parser<'s> {
    source: &'s str,
    lexer: Lexer<'s>,
    current: Token,
    tape: TapeBuilder,
    context: ParserContext<'s>,
    options: ParseOptions,
    function_depth: u32,
    // Parentheses-transparent semantic tag for immediate grammar checks, without widening ParsedNode.
    last_node_tag: Option<NodeTag>,
    last_assignment_target: AssignmentTargetType,
    assignment_pattern_candidates: Vec<AssignmentPatternCandidate>,
}

impl<'s> Parser<'s> {
    fn new(source: &'s str, source_len: u32, options: ParseOptions) -> Self {
        let mut lexer = Lexer::new(source);
        let current = lexer.next_token();
        let module = matches!(options.source_kind, SourceKind::Module);
        let top_level_await = matches!(
            options.source_kind,
            SourceKind::Module | SourceKind::Unambiguous
        );
        let ambient = matches!(options.language, Language::TypeScriptDefinition);
        let strict =
            module || current.kind == TokenKind::String && has_use_strict_directive(source, 0);
        let typescript_grammar = options.language.is_typescript()
            || options.syntax_extensions.typescript_js_compatibility;
        let grammar = GrammarContext::new(module, ambient, options.semantic_errors)
            .with_strict(strict)
            .with_allow_await(top_level_await || typescript_grammar)
            .with_allow_yield(typescript_grammar);
        Self {
            source,
            lexer,
            current,
            tape: TapeBuilder::new_parser(source_len),
            context: ParserContext::new(grammar),
            options,
            function_depth: 0,
            last_node_tag: None,
            last_assignment_target: AssignmentTargetType::Invalid,
            assignment_pattern_candidates: Vec::new(),
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
        let assignment_patterns = self.assignment_pattern_checkpoint();
        let statement = match self.current.kind {
            TokenKind::Semicolon => self.parse_empty_statement(),
            TokenKind::LeftBrace => self.parse_block_statement(),
            TokenKind::Const
                if self.options.language.is_typescript() && self.followed_by_word("enum") =>
            {
                self.parse_enum_declaration(true)
            }
            TokenKind::Var | TokenKind::Let | TokenKind::Const => {
                self.parse_variable_declaration(true)
            }
            TokenKind::Type if self.options.language.is_typescript() => {
                self.parse_type_alias_declaration()
            }
            TokenKind::Interface if self.options.language.is_typescript() => {
                self.parse_interface_declaration()
            }
            TokenKind::Enum if self.options.language.is_typescript() => {
                self.parse_enum_declaration(false)
            }
            TokenKind::Namespace | TokenKind::Module if self.options.language.is_typescript() => {
                self.parse_module_declaration()
            }
            TokenKind::Function => self.parse_function(true, false),
            TokenKind::Async if self.followed_by_token_without_line_break(TokenKind::Function) => {
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
            TokenKind::Import if self.import_starts_expression() => {
                self.parse_expression_or_labeled_statement()
            }
            TokenKind::Import => self.parse_import_declaration(),
            TokenKind::Export => self.parse_export_declaration(),
            TokenKind::Break => self.parse_jump_statement(false),
            TokenKind::Continue => self.parse_jump_statement(true),
            TokenKind::Debugger => self.parse_debugger_statement(),
            TokenKind::With => self.parse_with_statement(),
            _ => self.parse_expression_or_labeled_statement(),
        };
        self.rollback_assignment_patterns(assignment_patterns);
        statement
    }

    fn parse_empty_statement(&mut self) -> Result<ParsedNode, ParseError> {
        let token = self.take();
        self.node(NodeTag::EMPTY_STATEMENT, Self::token_span(token), &[])
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

    #[allow(clippy::too_many_lines)]
    fn parse_function(
        &mut self,
        declaration: bool,
        asynchronous: bool,
    ) -> Result<ParsedNode, ParseError> {
        let start = if asynchronous {
            let start = self.expect(TokenKind::Async).start;
            self.expect(TokenKind::Function);
            start
        } else {
            self.expect(TokenKind::Function).start
        };
        let generator = self.eat(TokenKind::Star).is_some();
        let id = if Self::is_identifier_name(self.current.kind) {
            Some(if declaration {
                let declaration_binding = self.context.function_declaration_binding_kind();
                self.parse_binding_identifier(declaration_binding)?
            } else {
                self.parse_identifier()?
            })
        } else {
            if declaration {
                self.error(self.current_span(), "function declaration requires a name");
            }
            None
        };
        self.diagnose_function_name(id, declaration, asynchronous, generator);
        let type_parameters =
            if self.options.language.is_typescript() && self.current.kind == TokenKind::Lt {
                Some(self.parse_type_parameters()?)
            } else {
                None
            };
        self.expect(TokenKind::LeftParen);
        let previous_grammar = self.enter_function_context(generator, asynchronous);
        self.context.set_grammar(
            self.context
                .grammar()
                .with_allow_super(false)
                .with_allow_super_call(false)
                .with_parameters(true),
        );
        if !declaration && let Some(id) = id {
            let name = self
                .source
                .get(id.span.start as usize..id.span.end as usize)
                .unwrap_or_default();
            let _ = self
                .context
                .declare_binding(name, BindingKind::Lexical, id.span);
        }
        let params = self.parse_parameter_list()?;
        self.context
            .set_grammar(self.context.grammar().with_parameters(false));
        self.diagnose_rest_parameter_trailing_comma(&params);
        self.expect(TokenKind::RightParen);
        let return_type = self.parse_function_return_type()?;
        let has_use_strict = self.current.kind == TokenKind::LeftBrace
            && has_use_strict_directive(self.source, self.current.end as usize);
        self.diagnose_strict_function_parameters(&params, has_use_strict);
        if has_use_strict {
            self.context
                .set_grammar(self.context.grammar().with_strict(true));
        }
        // Checking for a block first keeps the ordinary function path to one token comparison.
        if self.current.kind != TokenKind::LeftBrace
            && declaration
            && (self.options.language.is_typescript()
                || self.options.syntax_extensions.typescript_js_compatibility)
            && let Some(end) = self.consume_typescript_function_signature_terminator()
        {
            self.leave_function_context(previous_grammar);
            return self.node_typescript_declare_function(
                Span::new(start, end),
                id.map(ParsedNode::value),
                params.value,
                FunctionFlags {
                    generator,
                    asynchronous,
                },
                return_type,
                type_parameters,
            );
        }
        if self.context.grammar().ambient()
            && self.options.semantic_errors
            && self.current.kind == TokenKind::LeftBrace
        {
            self.error(
                self.current_span(),
                "function implementations are not allowed in ambient contexts",
            );
        }
        let body = self.parse_block_statement()?;
        self.leave_function_context(previous_grammar);
        let id = if let Some(id) = id {
            id.value()
        } else {
            self.tape.push_null()?
        };
        let generator = self.tape.push_bool(generator)?;
        let asynchronous = self.tape.push_bool(asynchronous)?;
        let tag = if declaration {
            NodeTag::FUNCTION_DECLARATION
        } else {
            NodeTag::FUNCTION_EXPRESSION
        };
        let span = Span::new(start, body.span.end);
        // Field six remains the return type so existing annotated function tapes keep their shape.
        let mut fields = [
            id,
            params.value,
            body.value(),
            generator,
            asynchronous,
            id,
            id,
        ];
        let field_count = match (return_type, type_parameters) {
            (Some(return_type), Some(type_parameters)) => {
                fields[5] = return_type;
                fields[6] = type_parameters;
                7
            }
            (None, Some(type_parameters)) => {
                fields[5] = self.tape.push_null()?;
                fields[6] = type_parameters;
                7
            }
            (Some(return_type), None) => {
                fields[5] = return_type;
                6
            }
            (None, None) => 5,
        };
        self.node(tag, span, &fields[..field_count])
    }

    #[cold]
    #[inline(never)]
    fn node_typescript_declare_function(
        &mut self,
        span: Span,
        id: Option<ValueRef>,
        params: ValueRef,
        flags: FunctionFlags,
        return_type: Option<ValueRef>,
        type_parameters: Option<ValueRef>,
    ) -> Result<ParsedNode, ParseError> {
        let id = if let Some(id) = id {
            id
        } else {
            self.tape.push_null()?
        };
        let generator = self.tape.push_bool(flags.generator)?;
        let asynchronous = self.tape.push_bool(flags.asynchronous)?;
        let return_type = if let Some(return_type) = return_type {
            return_type
        } else {
            self.tape.push_null()?
        };
        let type_parameters = if let Some(type_parameters) = type_parameters {
            type_parameters
        } else {
            self.tape.push_null()?
        };
        self.node(
            NodeTag::TS_DECLARE_FUNCTION,
            span,
            &[
                id,
                params,
                generator,
                asynchronous,
                return_type,
                type_parameters,
            ],
        )
    }

    #[cold]
    #[inline(never)]
    fn consume_typescript_function_signature_terminator(&mut self) -> Option<u32> {
        if let Some(semicolon) = self.eat(TokenKind::Semicolon) {
            Some(semicolon.end)
        } else if self.current.kind == TokenKind::RightBrace
            || self.current.kind == TokenKind::Eof
            || self.current.flags.line_break_before()
        {
            Some(self.current.start)
        } else {
            None
        }
    }

    fn parse_function_return_type(&mut self) -> Result<Option<ValueRef>, ParseError> {
        if !self.options.language.is_typescript() {
            return Ok(None);
        }
        let Some(colon) = self.eat(TokenKind::Colon) else {
            return Ok(None);
        };
        let annotation = self.parse_type()?;
        Ok(Some(
            self.node(
                NodeTag::TS_TYPE_ANNOTATION,
                Span::new(colon.start, annotation.span.end),
                &[annotation.value()],
            )?
            .value(),
        ))
    }

    fn diagnose_function_name(
        &mut self,
        id: Option<ParsedNode>,
        declaration: bool,
        asynchronous: bool,
        generator: bool,
    ) {
        if !self.reports_ecmascript_early_errors() {
            return;
        }
        let Some(id) = id else {
            return;
        };
        if asynchronous
            && (!declaration || generator)
            && self.static_property_name_matches(id.span, "await")
        {
            self.error(id.span, "async function name cannot be `await`");
        }
        if generator && self.static_property_name_matches(id.span, "yield") {
            self.error(id.span, "generator function name cannot be `yield`");
        }
        if self.context.grammar().strict()
            && (self.static_property_name_matches(id.span, "eval")
                || self.static_property_name_matches(id.span, "arguments"))
        {
            self.error(
                id.span,
                "function name cannot be `eval` or `arguments` in strict mode",
            );
        }
    }

    fn diagnose_rest_parameter_trailing_comma(&mut self, params: &ParsedParameterList) {
        if self.reports_ecmascript_early_errors() && params.has_rest && params.has_trailing_comma {
            self.error(
                self.current_span(),
                "rest parameter cannot have a trailing comma",
            );
        }
    }

    fn diagnose_strict_function_parameters(
        &mut self,
        params: &ParsedParameterList,
        has_use_strict: bool,
    ) {
        if !self.reports_ecmascript_early_errors() {
            return;
        }
        if has_use_strict && !params.simple {
            self.error(
                self.current_span(),
                "a function with non-simple parameters cannot contain a use strict directive",
            );
        }
        if (self.context.grammar().strict() || has_use_strict)
            && let Some(span) = self.context.current_restricted_parameter_binding()
        {
            self.error(span, "eval and arguments cannot be bound in strict mode");
        }
    }

    fn parse_parameter_list(&mut self) -> Result<ParsedParameterList, ParseError> {
        let params = self.parse_parameters()?;
        Ok(ParsedParameterList {
            count: params.values.len(),
            value: self.tape.push_list(&params.values)?,
            has_rest: params.has_rest,
            has_trailing_comma: params.has_trailing_comma,
            simple: params.simple,
        })
    }

    fn parse_parameters(&mut self) -> Result<ParsedParameters, ParseError> {
        let mut values = Vec::new();
        let mut has_rest = false;
        let mut has_trailing_comma = false;
        let mut simple = true;
        while !matches!(self.current.kind, TokenKind::RightParen | TokenKind::Eof) {
            let parameter = if self.eat(TokenKind::Ellipsis).is_some() {
                has_rest = true;
                simple = false;
                let argument = self.parse_binding_pattern(BindingKind::Parameter)?;
                self.parse_binding_rest_element(argument)?
            } else {
                let pattern = self.parse_parameter_binding()?;
                if self.current.kind == TokenKind::Eq {
                    simple = false;
                }
                simple &= self.last_node_tag == Some(NodeTag::IDENTIFIER);
                self.parse_binding_default(pattern)?
            };
            values.push(parameter.value());
            if self.eat(TokenKind::Comma).is_none() {
                break;
            }
            has_trailing_comma =
                matches!(self.current.kind, TokenKind::RightParen | TokenKind::Eof);
            if self.reports_ecmascript_early_errors() && has_rest && !has_trailing_comma {
                self.error(self.current_span(), "rest parameter must be last");
            }
        }
        Ok(ParsedParameters {
            values,
            has_rest,
            has_trailing_comma,
            simple,
        })
    }

    fn parse_parameter_binding(&mut self) -> Result<ParsedNode, ParseError> {
        if Self::is_identifier_name(self.current.kind) {
            self.parse_binding_identifier_with_optional(BindingKind::Parameter)
        } else {
            self.parse_binding_pattern(BindingKind::Parameter)
        }
    }

    fn enter_function_context(&mut self, generator: bool, asynchronous: bool) -> GrammarContext {
        self.function_depth = self.function_depth.saturating_add(1);
        self.context.enter_scope(ScopeKind::Function);
        let previous = self.context.grammar();
        self.context.set_grammar(
            previous
                .with_function(true)
                .with_generator(generator)
                .with_async_function(asynchronous)
                .with_allow_yield(
                    generator
                        || self.options.language.is_typescript()
                        || self.options.syntax_extensions.typescript_js_compatibility,
                )
                .with_allow_await(
                    asynchronous
                        || self.options.language.is_typescript()
                        || self.options.syntax_extensions.typescript_js_compatibility,
                )
                .with_parameters(false),
        );
        previous
    }

    fn leave_function_context(&mut self, previous: GrammarContext) {
        self.context.set_grammar(previous);
        let _ = self.context.leave_scope();
        self.function_depth = self.function_depth.saturating_sub(1);
    }

    fn parse_method_function(
        &mut self,
        start: u32,
        generator: bool,
        asynchronous: bool,
        accessor: Option<AccessorKind>,
        body_policy: MethodBodyPolicy,
    ) -> Result<ParsedNode, ParseError> {
        let signature_start = self.current.start;
        self.expect(TokenKind::LeftParen);
        let previous_grammar = self.enter_function_context(generator, asynchronous);
        self.context.set_grammar(
            self.context
                .grammar()
                .with_accessor(accessor.is_some())
                .with_allow_super(true)
                .with_parameters(true),
        );
        let params = self.parse_parameter_list()?;
        self.context
            .set_grammar(self.context.grammar().with_parameters(false));
        if accessor == Some(AccessorKind::Get) && params.count != 0 {
            self.error(self.current_span(), "getter must not have parameters");
        }
        if accessor == Some(AccessorKind::Set)
            && (params.count != 1 || params.has_rest || params.has_trailing_comma)
        {
            self.error(
                self.current_span(),
                "setter must have exactly one non-rest parameter without a trailing comma",
            );
        }
        self.diagnose_rest_parameter_trailing_comma(&params);
        self.expect(TokenKind::RightParen);
        // Only canonical constructors enable direct super calls, and their return annotations remain unsupported.
        let return_type = if self.context.grammar().allow_super_call() {
            None
        } else {
            self.parse_function_return_type()?
        };
        if return_type.is_some()
            && accessor == Some(AccessorKind::Set)
            && self.options.semantic_errors
        {
            self.error(
                self.current_span(),
                "setter cannot have a return type annotation",
            );
        }
        let has_use_strict = self.current.kind == TokenKind::LeftBrace
            && has_use_strict_directive(self.source, self.current.end as usize);
        if has_use_strict && !params.simple {
            self.error(
                self.current_span(),
                "a function with non-simple parameters cannot contain a use strict directive",
            );
        }
        if (self.context.grammar().strict() || has_use_strict)
            && let Some(span) = self.context.current_restricted_parameter_binding()
        {
            self.error(span, "eval and arguments cannot be bound in strict mode");
        }
        if has_use_strict {
            self.context
                .set_grammar(self.context.grammar().with_strict(true));
        }
        if body_policy == MethodBodyPolicy::TypeScriptSignature
            && accessor.is_none()
            && !generator
            && !asynchronous
            && let Some(semicolon) = self.eat(TokenKind::Semicolon)
        {
            self.leave_function_context(previous_grammar);
            return self.node_typescript_empty_body_function(
                Span::new(signature_start, semicolon.end),
                params.value,
                return_type,
            );
        }
        let body = self.parse_block_statement()?;
        self.leave_function_context(previous_grammar);
        let id = self.tape.push_null()?;
        let generator = self.tape.push_bool(generator)?;
        let asynchronous = self.tape.push_bool(asynchronous)?;
        let span = Span::new(start, body.span.end);
        // The five-field form remains the unannotated hot path shared with JavaScript methods.
        if let Some(return_type) = return_type {
            self.node(
                NodeTag::FUNCTION_EXPRESSION,
                span,
                &[
                    id,
                    params.value,
                    body.value(),
                    generator,
                    asynchronous,
                    return_type,
                ],
            )
        } else {
            self.node(
                NodeTag::FUNCTION_EXPRESSION,
                span,
                &[id, params.value, body.value(), generator, asynchronous],
            )
        }
    }

    #[cold]
    #[inline(never)]
    fn node_typescript_empty_body_function(
        &mut self,
        span: Span,
        params: ValueRef,
        return_type: Option<ValueRef>,
    ) -> Result<ParsedNode, ParseError> {
        let id = self.tape.push_null()?;
        let generator = self.tape.push_bool(false)?;
        let asynchronous = self.tape.push_bool(false)?;
        let return_type = if let Some(return_type) = return_type {
            return_type
        } else {
            self.tape.push_null()?
        };
        self.node(
            NodeTag::TS_EMPTY_BODY_FUNCTION_EXPRESSION,
            span,
            &[id, params, generator, asynchronous, return_type],
        )
    }

    fn parse_method_function_with_super_call(
        &mut self,
        start: u32,
        generator: bool,
        asynchronous: bool,
        accessor: Option<AccessorKind>,
        allow_super_call: bool,
        body_policy: MethodBodyPolicy,
    ) -> Result<ParsedNode, ParseError> {
        let previous = self.context.grammar().allow_super_call();
        self.context.set_grammar(
            self.context
                .grammar()
                .with_allow_super_call(allow_super_call),
        );
        let function =
            self.parse_method_function(start, generator, asynchronous, accessor, body_policy);
        self.context
            .set_grammar(self.context.grammar().with_allow_super_call(previous));
        function
    }

    fn parse_class(&mut self, declaration: bool) -> Result<ParsedNode, ParseError> {
        let start = self.expect(TokenKind::Class).start;
        if self.options.language.is_typescript()
            || self.options.syntax_extensions.typescript_js_compatibility
        {
            return self.parse_typescript_class(start, declaration);
        }
        let id = if Self::is_identifier_name(self.current.kind) {
            self.parse_binding_identifier(BindingKind::Lexical)?.value()
        } else {
            if declaration {
                self.error(self.current_span(), "class declaration requires a name");
            }
            self.tape.push_null()?
        };
        let has_super = self.eat(TokenKind::Extends).is_some();
        let super_class = if has_super {
            self.parse_assignment_expression(true)?.value()
        } else {
            self.tape.push_null()?
        };
        let body_start = self.expect(TokenKind::LeftBrace).start;
        self.context.enter_scope(ScopeKind::Class);
        let previous_grammar = self.context.grammar();
        self.context.set_grammar(
            previous_grammar
                .with_class(true)
                .with_strict(true)
                .with_accessor(false)
                .with_allow_super(true)
                .with_allow_super_call(false),
        );
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

    #[allow(clippy::too_many_lines)]
    fn parse_typescript_class(
        &mut self,
        start: u32,
        declaration: bool,
    ) -> Result<ParsedNode, ParseError> {
        // 1. Distinguish a class named `implements` from an anonymous implementation clause.
        let anonymous_implements_clause = self.current.kind == TokenKind::Implements
            && !self.current.flags.escaped()
            && !self.implements_is_followed_by_class_body();
        let id = if Self::is_identifier_name(self.current.kind) && !anonymous_implements_clause {
            self.parse_binding_identifier(BindingKind::Lexical)?.value()
        } else {
            if declaration {
                self.error(self.current_span(), "class declaration requires a name");
            }
            self.tape.push_null()?
        };
        let type_parameters = if self.current.kind == TokenKind::Lt {
            Some(self.parse_type_parameters()?)
        } else {
            None
        };

        // 2. Recover reordered and repeated clauses while retaining the first base and merging implementation lists.
        let mut super_class = None;
        let mut implementations = Vec::new();
        let mut saw_extends = false;
        let mut saw_implements = false;
        while !self.current.flags.escaped() {
            match self.current.kind {
                TokenKind::Extends => {
                    let keyword = self.take();
                    if self.options.semantic_errors && saw_extends {
                        self.error(Self::token_span(keyword), "extends clause already seen");
                    }
                    if self.options.semantic_errors && saw_implements {
                        self.error(
                            Self::token_span(keyword),
                            "extends clause must precede implements clause",
                        );
                    }
                    saw_extends = true;
                    let empty_clause = match self.current.kind {
                        TokenKind::LeftBrace => self.left_brace_is_followed_by_right_brace(),
                        TokenKind::Eof | TokenKind::Extends => true,
                        TokenKind::Implements => !self.current.flags.escaped(),
                        _ => false,
                    };
                    if empty_clause {
                        if self.options.semantic_errors {
                            self.error(self.current_span(), "extends clause cannot be empty");
                        }
                        continue;
                    }
                    let discarded_base = super_class.is_some().then(|| {
                        (
                            self.tape.checkpoint(),
                            self.assignment_pattern_checkpoint(),
                            self.last_node_tag,
                            self.last_assignment_target,
                        )
                    });
                    let base = self.parse_assignment_expression(true)?;
                    if let Some((
                        tape,
                        assignment_patterns,
                        last_node_tag,
                        last_assignment_target,
                    )) = discarded_base
                    {
                        self.tape.rollback(tape)?;
                        self.rollback_assignment_patterns(assignment_patterns);
                        self.last_node_tag = last_node_tag;
                        self.last_assignment_target = last_assignment_target;
                    } else {
                        super_class = Some(base.value());
                    }
                }
                TokenKind::Implements => {
                    let keyword = self.take();
                    if self.options.semantic_errors && saw_implements {
                        self.error(Self::token_span(keyword), "implements clause already seen");
                    }
                    saw_implements = true;
                    let before = implementations.len();
                    loop {
                        if matches!(
                            self.current.kind,
                            TokenKind::LeftBrace
                                | TokenKind::Eof
                                | TokenKind::Extends
                                | TokenKind::Implements
                        ) && !self.current.flags.escaped()
                        {
                            break;
                        }
                        if let Some(comma) = self.eat(TokenKind::Comma) {
                            self.error(Self::token_span(comma), "expected an implemented type");
                            continue;
                        }
                        implementations.push(
                            self.parse_heritage(
                                NodeTag::TS_CLASS_IMPLEMENTS,
                                self.options.semantic_errors,
                            )?
                            .value(),
                        );
                        if self.eat(TokenKind::Comma).is_none() {
                            break;
                        }
                    }
                    if self.options.semantic_errors && implementations.len() == before {
                        self.error(self.current_span(), "implements list cannot be empty");
                    }
                }
                _ => break,
            }
        }
        let super_class = if let Some(super_class) = super_class {
            super_class
        } else {
            self.tape.push_null()?
        };

        // 3. Parse the body under the same strict class grammar as the JavaScript path.
        let body_start = self.expect(TokenKind::LeftBrace).start;
        self.context.enter_scope(ScopeKind::Class);
        let previous_grammar = self.context.grammar();
        self.context.set_grammar(
            previous_grammar
                .with_class(true)
                .with_strict(true)
                .with_accessor(false)
                .with_allow_super(true)
                .with_allow_super_call(false),
        );
        let mut elements = Vec::new();
        while !matches!(self.current.kind, TokenKind::RightBrace | TokenKind::Eof) {
            if self.eat(TokenKind::Semicolon).is_some() {
                continue;
            }
            elements.push(self.parse_typescript_class_element(saw_extends)?.value());
        }
        let end = self.expect(TokenKind::RightBrace).end;
        self.context.set_grammar(previous_grammar);
        let _ = self.context.leave_scope();
        let elements = self.tape.push_list(&elements)?;
        let body = self.node(NodeTag::CLASS_BODY, Span::new(body_start, end), &[elements])?;
        // 4. Keep nongeneric classes on their existing wire tags.
        if let Some(type_parameters) = type_parameters {
            return self.node_typescript_generic_class(
                declaration,
                Span::new(start, end),
                [id, super_class, body.value()],
                saw_implements.then_some(implementations.as_slice()),
                type_parameters,
            );
        }
        if saw_implements {
            let implementations = self.tape.push_list(&implementations)?;
            self.node(
                if declaration {
                    NodeTag::TS_CLASS_DECLARATION
                } else {
                    NodeTag::TS_CLASS_EXPRESSION
                },
                Span::new(start, end),
                &[id, super_class, body.value(), implementations],
            )
        } else {
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
    }

    #[cold]
    #[inline(never)]
    fn node_typescript_generic_class(
        &mut self,
        declaration: bool,
        span: Span,
        class_fields: [ValueRef; 3],
        implementations: Option<&[ValueRef]>,
        type_parameters: ValueRef,
    ) -> Result<ParsedNode, ParseError> {
        let implementations = if let Some(implementations) = implementations {
            self.tape.push_list(implementations)?
        } else {
            self.tape.push_null()?
        };
        self.node(
            if declaration {
                NodeTag::TS_GENERIC_CLASS_DECLARATION
            } else {
                NodeTag::TS_GENERIC_CLASS_EXPRESSION
            },
            span,
            &[
                class_fields[0],
                class_fields[1],
                class_fields[2],
                implementations,
                type_parameters,
            ],
        )
    }

    fn parse_class_element(&mut self) -> Result<ParsedNode, ParseError> {
        let start = self.current.start;
        let static_token = self.eat(TokenKind::Static);
        if static_token.is_some() && self.current.kind == TokenKind::LeftBrace {
            let block = self.parse_block_statement()?;
            return self.node(
                NodeTag::STATIC_BLOCK,
                Span::new(start, block.span.end),
                &[block.value()],
            );
        }
        let mut leading_key = None;
        let is_static = if let Some(token) = static_token {
            if matches!(
                self.current.kind,
                TokenKind::LeftParen | TokenKind::Eq | TokenKind::Semicolon | TokenKind::RightBrace
            ) {
                leading_key = Some(self.identifier_from_span(Self::token_span(token))?);
                false
            } else {
                true
            }
        } else {
            false
        };
        if leading_key.is_none()
            && let Some(accessor) = self.current_accessor_kind(true)
        {
            return self.parse_class_accessor(start, is_static, accessor);
        }

        let async_token = if leading_key.is_none() && self.current.kind == TokenKind::Async {
            Some(self.take())
        } else {
            None
        };
        let async_modifier = async_token.is_some_and(|token| {
            !token.flags.escaped()
                && !self.current.flags.line_break_before()
                && (self.current.kind == TokenKind::Star
                    || Self::is_property_name_start(self.current.kind, true))
        });
        let generator =
            self.current.kind == TokenKind::Star && async_token.is_none_or(|_| async_modifier);
        let asynchronous = async_modifier;
        if generator {
            self.bump();
        } else if let Some(token) = async_token
            && !asynchronous
        {
            leading_key = Some(self.identifier_from_span(Self::token_span(token))?);
        }

        let property_name = if generator || asynchronous {
            self.parse_property_name(true)?
        } else if let Some(key) = leading_key {
            ParsedPropertyName {
                key,
                computed: false,
                shorthand: true,
            }
        } else {
            self.parse_property_name(true)?
        };
        let key = property_name.key;
        let computed = property_name.computed;

        if generator || asynchronous || self.current.kind == TokenKind::LeftParen {
            return self.parse_class_method_definition(
                start,
                &property_name,
                is_static,
                generator,
                asynchronous,
            );
        }

        let type_annotation = self.tape.push_null()?;
        let value = if self.eat(TokenKind::Eq).is_some() {
            self.parse_assignment_expression(true)?.value()
        } else {
            self.tape.push_null()?
        };
        let end = self.consume_semicolon();
        if property_name.shorthand
            && self.reports_ecmascript_early_errors()
            && self.static_property_name_matches(key.span, "constructor")
        {
            self.error(key.span, "classes cannot have a field named constructor");
        }
        let computed = self.tape.push_bool(computed)?;
        let is_static = self.tape.push_bool(is_static)?;
        self.node(
            NodeTag::PROPERTY_DEFINITION,
            Span::new(start, end),
            &[key.value(), value, computed, is_static, type_annotation],
        )
    }

    #[allow(clippy::too_many_lines)]
    fn parse_typescript_class_element(
        &mut self,
        class_has_super: bool,
    ) -> Result<ParsedNode, ParseError> {
        let start = self.current.start;
        let mut modifiers = TypeScriptModifiers::default();
        let mut last_modifier_rank = None;
        let mut leading_key = None;
        let mut is_static = false;

        // Parse the contextual TypeScript prelude without stealing modifier-shaped member names.
        loop {
            if let Some(accessibility) = self.current_accessibility_modifier()
                && self.typescript_modifier_has_class_member_follower(false)
            {
                let token = self.take();
                self.diagnose_typescript_modifier_order(
                    0,
                    &mut last_modifier_rank,
                    modifiers.accessibility.is_some(),
                    Self::token_span(token),
                );
                modifiers.accessibility.get_or_insert(accessibility);
                continue;
            }

            if !is_static && self.current_typescript_modifier_matches(TokenKind::Static, "static") {
                let has_member_follower = self.typescript_modifier_has_class_member_follower(true);
                let token = self.take();
                if self.current.kind == TokenKind::LeftBrace {
                    if modifiers.any() && self.options.semantic_errors {
                        self.error(
                            Span::new(start, token.end),
                            "static blocks cannot have TypeScript modifiers",
                        );
                    }
                    let block = self.parse_block_statement()?;
                    return self.node(
                        NodeTag::STATIC_BLOCK,
                        Span::new(start, block.span.end),
                        &[block.value()],
                    );
                }
                if !has_member_follower {
                    leading_key = Some(self.identifier_from_span(Self::token_span(token))?);
                    break;
                }
                self.diagnose_typescript_modifier_order(
                    1,
                    &mut last_modifier_rank,
                    false,
                    Self::token_span(token),
                );
                is_static = true;
                continue;
            }

            if self.current_typescript_modifier_matches(TokenKind::Override, "override")
                && self.typescript_modifier_has_class_member_follower(false)
            {
                let token = self.take();
                self.diagnose_typescript_modifier_order(
                    2,
                    &mut last_modifier_rank,
                    modifiers.r#override,
                    Self::token_span(token),
                );
                modifiers.r#override = true;
                continue;
            }

            if self.current_typescript_modifier_matches(TokenKind::Readonly, "readonly")
                && self.typescript_modifier_has_class_member_follower(false)
            {
                let token = self.take();
                self.diagnose_typescript_modifier_order(
                    3,
                    &mut last_modifier_rank,
                    modifiers.readonly,
                    Self::token_span(token),
                );
                modifiers.readonly = true;
                continue;
            }
            break;
        }
        let member_context = TypeScriptClassMemberContext {
            modifiers,
            class_has_super,
        };

        // Resolve contextual accessor and async introducers before parsing the property name.
        if leading_key.is_none()
            && let Some(accessor) = self.current_accessor_kind(true)
        {
            return self.parse_typescript_class_accessor(
                start,
                is_static,
                accessor,
                member_context,
            );
        }

        let async_token = if leading_key.is_none() && self.current.kind == TokenKind::Async {
            Some(self.take())
        } else {
            None
        };
        let async_modifier = async_token.is_some_and(|token| {
            !token.flags.escaped()
                && !self.current.flags.line_break_before()
                && (self.current.kind == TokenKind::Star
                    || Self::is_property_name_start(self.current.kind, true))
        });
        let generator =
            self.current.kind == TokenKind::Star && async_token.is_none_or(|_| async_modifier);
        let asynchronous = async_modifier;
        if generator {
            self.bump();
        } else if let Some(token) = async_token
            && !asynchronous
        {
            leading_key = Some(self.identifier_from_span(Self::token_span(token))?);
        }

        let property_name = if generator || asynchronous {
            self.parse_property_name(true)?
        } else if let Some(key) = leading_key {
            ParsedPropertyName {
                key,
                computed: false,
                shorthand: true,
            }
        } else {
            self.parse_property_name(true)?
        };
        let key = property_name.key;
        let computed = property_name.computed;

        if generator || asynchronous || self.current.kind == TokenKind::LeftParen {
            return self.parse_typescript_class_method_definition(
                start,
                &property_name,
                is_static,
                generator,
                asynchronous,
                member_context,
            );
        }

        // Fields share their legacy five-value payload; modifiers select only the cold outer tag.
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
        if property_name.shorthand && self.static_property_name_matches(key.span, "constructor") {
            self.error(key.span, "classes cannot have a field named constructor");
        }
        self.diagnose_typescript_class_member_modifiers(
            modifiers,
            key.span,
            false,
            false,
            member_context.class_has_super,
        );
        let computed = self.tape.push_bool(computed)?;
        let is_static = self.tape.push_bool(is_static)?;
        if modifiers.any() {
            return self.node_typescript_modified_property_definition(
                Span::new(start, end),
                [key.value(), value, computed, is_static, type_annotation],
                modifiers,
            );
        }
        self.node(
            NodeTag::PROPERTY_DEFINITION,
            Span::new(start, end),
            &[key.value(), value, computed, is_static, type_annotation],
        )
    }

    fn parse_class_method_definition(
        &mut self,
        start: u32,
        property_name: &ParsedPropertyName,
        is_static: bool,
        generator: bool,
        asynchronous: bool,
    ) -> Result<ParsedNode, ParseError> {
        let key = property_name.key;
        let computed = property_name.computed;
        if (generator || asynchronous)
            && !is_static
            && !computed
            && self.static_property_name_matches(key.span, "constructor")
        {
            self.error(key.span, "class constructor cannot be async or a generator");
        }
        if (generator || asynchronous)
            && is_static
            && !computed
            && self.static_property_name_matches(key.span, "prototype")
        {
            self.error(key.span, "static class method cannot be named `prototype`");
        }
        let allow_super_call = !is_static
            && !computed
            && !generator
            && !asynchronous
            && self.static_property_name_matches(key.span, "constructor");
        let function = self.parse_method_function_with_super_call(
            key.span.start,
            generator,
            asynchronous,
            None,
            allow_super_call,
            MethodBodyPolicy::Block,
        )?;
        let kind = self.tape.push_u32(if allow_super_call { 3 } else { 0 })?;
        let computed = self.tape.push_bool(computed)?;
        let is_static = self.tape.push_bool(is_static)?;
        self.node(
            NodeTag::METHOD_DEFINITION,
            Span::new(start, function.span.end),
            &[key.value(), function.value(), kind, computed, is_static],
        )
    }

    fn parse_typescript_class_method_definition(
        &mut self,
        start: u32,
        property_name: &ParsedPropertyName,
        is_static: bool,
        generator: bool,
        asynchronous: bool,
        member_context: TypeScriptClassMemberContext,
    ) -> Result<ParsedNode, ParseError> {
        let modifiers = member_context.modifiers;
        let key = property_name.key;
        let computed = property_name.computed;
        if (generator || asynchronous)
            && !is_static
            && !computed
            && self.static_property_name_matches(key.span, "constructor")
        {
            self.error(key.span, "class constructor cannot be async or a generator");
        }
        if (generator || asynchronous)
            && is_static
            && !computed
            && self.static_property_name_matches(key.span, "prototype")
        {
            self.error(key.span, "static class method cannot be named `prototype`");
        }
        let allow_super_call = !is_static
            && !computed
            && !generator
            && !asynchronous
            && self.static_property_name_matches(key.span, "constructor");
        self.diagnose_typescript_class_member_modifiers(
            modifiers,
            key.span,
            true,
            allow_super_call,
            member_context.class_has_super,
        );
        let function = self.parse_method_function_with_super_call(
            key.span.start,
            generator,
            asynchronous,
            None,
            allow_super_call,
            MethodBodyPolicy::TypeScriptSignature,
        )?;
        let kind = self.tape.push_u32(if allow_super_call { 3 } else { 0 })?;
        let computed = self.tape.push_bool(computed)?;
        let is_static = self.tape.push_bool(is_static)?;
        if modifiers.any() {
            return self.node_typescript_modified_method_definition(
                Span::new(start, function.span.end),
                [key.value(), function.value(), kind, computed, is_static],
                modifiers,
            );
        }
        self.node(
            NodeTag::METHOD_DEFINITION,
            Span::new(start, function.span.end),
            &[key.value(), function.value(), kind, computed, is_static],
        )
    }

    #[cold]
    #[inline(never)]
    fn node_typescript_modified_method_definition(
        &mut self,
        span: Span,
        fields: [ValueRef; 5],
        modifiers: TypeScriptModifiers,
    ) -> Result<ParsedNode, ParseError> {
        let tag = NodeTag::TS_MODIFIED_METHOD_DEFINITION;
        let node = self
            .tape
            .push_node(tag, span, modifiers.wire_flags(), &fields)?;
        self.last_node_tag = Some(tag);
        self.last_assignment_target = AssignmentTargetType::Invalid;
        Ok(ParsedNode { node, span })
    }

    #[cold]
    #[inline(never)]
    fn node_typescript_modified_property_definition(
        &mut self,
        span: Span,
        fields: [ValueRef; 5],
        modifiers: TypeScriptModifiers,
    ) -> Result<ParsedNode, ParseError> {
        let tag = NodeTag::TS_MODIFIED_PROPERTY_DEFINITION;
        let node = self
            .tape
            .push_node(tag, span, modifiers.wire_flags(), &fields)?;
        self.last_node_tag = Some(tag);
        self.last_assignment_target = AssignmentTargetType::Invalid;
        Ok(ParsedNode { node, span })
    }

    fn parse_class_accessor(
        &mut self,
        start: u32,
        is_static: bool,
        accessor: AccessorKind,
    ) -> Result<ParsedNode, ParseError> {
        self.bump();
        let (property_name, private) = if self.current.kind == TokenKind::PrivateIdentifier {
            let (key, name) = self.parse_private_identifier()?;
            let name_span = Span::new(key.span.start.saturating_add(1), key.span.end);
            if name == "constructor" && self.reports_private_early_errors() {
                self.error(key.span, "private name `#constructor` is not allowed");
            }
            let _ = self.context.declare_private_accessor(
                name,
                name_span,
                accessor.private_kind(),
                is_static,
            );
            (
                ParsedPropertyName {
                    key,
                    computed: false,
                    shorthand: false,
                },
                true,
            )
        } else {
            (self.parse_property_name(true)?, false)
        };
        let key = property_name.key;
        let computed = property_name.computed;
        if !private
            && !computed
            && !is_static
            && self.static_property_name_matches(key.span, "constructor")
        {
            self.error(key.span, "class constructor cannot be an accessor");
        }
        if !private
            && !computed
            && is_static
            && self.static_property_name_matches(key.span, "prototype")
        {
            self.error(
                key.span,
                "static class accessor cannot be named `prototype`",
            );
        }
        let function = self.parse_method_function_with_super_call(
            key.span.start,
            false,
            false,
            Some(accessor),
            false,
            MethodBodyPolicy::Block,
        )?;
        let kind = self.tape.push_u32(accessor.method_kind())?;
        let computed = self.tape.push_bool(computed)?;
        let is_static = self.tape.push_bool(is_static)?;
        self.node(
            NodeTag::METHOD_DEFINITION,
            Span::new(start, function.span.end),
            &[key.value(), function.value(), kind, computed, is_static],
        )
    }

    fn parse_typescript_class_accessor(
        &mut self,
        start: u32,
        is_static: bool,
        accessor: AccessorKind,
        member_context: TypeScriptClassMemberContext,
    ) -> Result<ParsedNode, ParseError> {
        let modifiers = member_context.modifiers;
        self.bump();
        let (property_name, private) = if self.current.kind == TokenKind::PrivateIdentifier {
            let (key, name) = self.parse_private_identifier()?;
            let name_span = Span::new(key.span.start.saturating_add(1), key.span.end);
            if name == "constructor" && self.reports_private_early_errors() {
                self.error(key.span, "private name `#constructor` is not allowed");
            }
            let _ = self.context.declare_private_accessor(
                name,
                name_span,
                accessor.private_kind(),
                is_static,
            );
            (
                ParsedPropertyName {
                    key,
                    computed: false,
                    shorthand: false,
                },
                true,
            )
        } else {
            (self.parse_property_name(true)?, false)
        };
        let key = property_name.key;
        let computed = property_name.computed;
        if !private
            && !computed
            && !is_static
            && self.static_property_name_matches(key.span, "constructor")
        {
            self.error(key.span, "class constructor cannot be an accessor");
        }
        if !private
            && !computed
            && is_static
            && self.static_property_name_matches(key.span, "prototype")
        {
            self.error(
                key.span,
                "static class accessor cannot be named `prototype`",
            );
        }
        self.diagnose_typescript_class_member_modifiers(
            modifiers,
            key.span,
            true,
            false,
            member_context.class_has_super,
        );
        let function = self.parse_method_function_with_super_call(
            key.span.start,
            false,
            false,
            Some(accessor),
            false,
            MethodBodyPolicy::Block,
        )?;
        let kind = self.tape.push_u32(accessor.method_kind())?;
        let computed = self.tape.push_bool(computed)?;
        let is_static = self.tape.push_bool(is_static)?;
        if modifiers.any() {
            return self.node_typescript_modified_method_definition(
                Span::new(start, function.span.end),
                [key.value(), function.value(), kind, computed, is_static],
                modifiers,
            );
        }
        self.node(
            NodeTag::METHOD_DEFINITION,
            Span::new(start, function.span.end),
            &[key.value(), function.value(), kind, computed, is_static],
        )
    }

    // Import alternatives share the consumed keyword and declaration-placement diagnostics.
    #[allow(clippy::too_many_lines)]
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
        if self.function_depth > 0 && self.current.kind != TokenKind::Dot {
            self.error(
                Span::new(start, start.saturating_add(6)),
                "import declarations are only allowed at the top level",
            );
        }
        if let Some(type_only) = self.import_equals_type_only(self.current) {
            if self.options.source_kind == SourceKind::Script && self.options.semantic_errors {
                self.error(
                    Span::new(start, start.saturating_add(6)),
                    "import declarations require module source type",
                );
            }
            return self.parse_import_equals_declaration(start, type_only);
        }
        if self.context.in_type_scope() {
            self.error(
                Span::new(start, start.saturating_add(6)),
                "import declarations in a namespace cannot reference a module",
            );
        }

        let mut specifiers = Vec::new();
        if self.current.kind != TokenKind::String {
            if Self::is_identifier_name(self.current.kind) {
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
        }
        let source = self.parse_literal()?;
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

    fn parse_import_equals_declaration(
        &mut self,
        start: u32,
        type_only: bool,
    ) -> Result<ParsedNode, ParseError> {
        if type_only {
            self.expect(TokenKind::Type);
        }
        let id = self.parse_binding_identifier(if type_only {
            BindingKind::Type
        } else {
            BindingKind::ImportEquals
        })?;
        self.expect(TokenKind::Eq);

        let external = if self.current.kind == TokenKind::Require && !self.current.flags.escaped() {
            let mut lookahead = Lexer::new(self.source);
            lookahead.set_position(self.current.end as usize);
            lookahead.next_token().kind == TokenKind::LeftParen
        } else {
            false
        };
        let module_reference = if external {
            let reference = self.parse_external_module_reference()?;
            if self.context.in_type_scope() {
                self.error(
                    reference.span,
                    "import declarations in a namespace cannot reference a module",
                );
            }
            reference
        } else {
            let reference = self.parse_import_equals_entity_name()?;
            if type_only {
                self.error(
                    reference.span,
                    "a type-only import alias must reference an external module",
                );
            }
            reference
        };
        let end = self.consume_semicolon();
        let import_kind = self.tape.push_u32(u32::from(type_only))?;
        self.node(
            NodeTag::TS_IMPORT_EQUALS_DECLARATION,
            Span::new(start, end),
            &[id.value(), module_reference.value(), import_kind],
        )
    }

    fn parse_import_equals_entity_name(&mut self) -> Result<ParsedNode, ParseError> {
        if !Self::is_type_reference_name(self.current.kind) {
            let span = self.current_span();
            self.error(span, "expected an import alias module reference");
            let name = self.tape.push_string("<invalid>")?;
            return self.node(NodeTag::IDENTIFIER, span, &[name]);
        }
        let mut name = self.parse_type_identifier()?;
        while self.eat(TokenKind::Dot).is_some() {
            let right = self.parse_type_identifier()?;
            name = self.node(
                NodeTag::TS_QUALIFIED_NAME,
                Span::new(name.span.start, right.span.end),
                &[name.value(), right.value()],
            )?;
        }
        Ok(name)
    }

    fn parse_external_module_reference(&mut self) -> Result<ParsedNode, ParseError> {
        let start = self.expect(TokenKind::Require).start;
        self.expect(TokenKind::LeftParen);

        let expression = if self.current.kind == TokenKind::String {
            self.parse_literal()?
        } else {
            let span = self.current_span();
            self.error(span, "external module reference requires a string literal");
            if matches!(
                self.current.kind,
                TokenKind::Comma | TokenKind::RightParen | TokenKind::Semicolon | TokenKind::Eof
            ) {
                let name = self.tape.push_string("<invalid>")?;
                self.node(NodeTag::IDENTIFIER, span, &[name])?
            } else {
                self.parse_assignment_expression(true)?
            }
        };

        if self.current.kind == TokenKind::Comma {
            self.error(
                self.current_span(),
                "external module reference accepts exactly one argument",
            );
            // Recovery cannot build discarded AST nodes because every tape record needs one parent.
            let mut depth = 0_u32;
            while self.current.kind != TokenKind::Eof {
                match self.current.kind {
                    TokenKind::LeftParen | TokenKind::LeftBracket | TokenKind::LeftBrace => {
                        depth = depth.saturating_add(1);
                    }
                    TokenKind::RightParen | TokenKind::Semicolon if depth == 0 => break,
                    TokenKind::RightParen | TokenKind::RightBracket | TokenKind::RightBrace => {
                        depth = depth.saturating_sub(1);
                    }
                    _ => {}
                }
                self.bump();
            }
        }
        let end = if let Some(right_paren) = self.eat(TokenKind::RightParen) {
            right_paren.end
        } else {
            self.expect(TokenKind::RightParen);
            self.current.start.max(expression.span.end)
        };
        self.node(
            NodeTag::TS_EXTERNAL_MODULE_REFERENCE,
            Span::new(start, end),
            &[expression.value()],
        )
    }

    // Export alternatives share state and recovery rules, so keeping the grammar branch local is
    // easier to audit than distributing it across helpers.
    #[allow(clippy::too_many_lines)]
    fn parse_export_declaration(&mut self) -> Result<ParsedNode, ParseError> {
        let start = self.expect(TokenKind::Export).start;
        if self.function_depth > 0 {
            self.error(
                Span::new(start, start.saturating_add(6)),
                "export declarations are only allowed at the top level",
            );
        }
        // Babel emits both TypeScript-only forms as standalone statements rather than ES export wrappers.
        if self.options.language.is_typescript() && self.eat(TokenKind::Eq).is_some() {
            let expression = self.parse_assignment_expression(true)?;
            let end = self.consume_semicolon();
            return self.node(
                NodeTag::TS_EXPORT_ASSIGNMENT,
                Span::new(start, end),
                &[expression.value()],
            );
        }
        if self.options.language.is_typescript()
            && self.current.kind == TokenKind::As
            && !self.current.flags.escaped()
        {
            self.bump();
            self.expect(TokenKind::Namespace);
            let id = self.parse_identifier()?;
            let end = self.consume_semicolon();
            return self.node(
                NodeTag::TS_NAMESPACE_EXPORT_DECLARATION,
                Span::new(start, end),
                &[id.value()],
            );
        }
        if self.options.language.is_typescript()
            && self.current.kind == TokenKind::Import
            && self.looks_like_export_import_equals()
        {
            let declaration = self.parse_import_declaration()?;
            let specifiers = self.tape.push_list(&[])?;
            let source = self.tape.push_null()?;
            let attributes = self.tape.push_list(&[])?;
            let export_kind = self.tape.push_u32(0)?;
            return self.node(
                NodeTag::EXPORT_NAMED_DECLARATION,
                Span::new(start, declaration.span.end),
                &[
                    declaration.value(),
                    specifiers,
                    source,
                    attributes,
                    export_kind,
                ],
            );
        }
        if self.eat(TokenKind::Default).is_some() {
            let (declaration, needs_semicolon) = match self.current.kind {
                TokenKind::Async
                    if self.followed_by_token_without_line_break(TokenKind::Function) =>
                {
                    (self.parse_function(true, true)?, false)
                }
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
            TokenKind::Const
                if self.options.language.is_typescript() && self.followed_by_word("enum") =>
            {
                self.parse_enum_declaration(true)?
            }
            TokenKind::Var | TokenKind::Let | TokenKind::Const => {
                self.parse_variable_declaration(true)?
            }
            TokenKind::Type if self.options.language.is_typescript() => {
                self.parse_type_alias_declaration()?
            }
            TokenKind::Interface if self.options.language.is_typescript() => {
                self.parse_interface_declaration()?
            }
            TokenKind::Enum if self.options.language.is_typescript() => {
                self.parse_enum_declaration(false)?
            }
            TokenKind::Namespace | TokenKind::Module if self.options.language.is_typescript() => {
                self.parse_module_declaration()?
            }
            TokenKind::Async if self.followed_by_token_without_line_break(TokenKind::Function) => {
                self.parse_function(true, true)?
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
                Self::token_span(keyword),
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
        let lexical_scope = matches!(self.current.kind, TokenKind::Let | TokenKind::Const);
        if lexical_scope {
            self.context.enter_scope(ScopeKind::Block);
        }
        let statement = self.parse_for_statement_with_head(start, asynchronous);
        if lexical_scope {
            let _ = self.context.leave_scope();
        }
        statement
    }

    fn parse_for_statement_with_head(
        &mut self,
        start: u32,
        asynchronous: bool,
    ) -> Result<ParsedNode, ParseError> {
        let mut expression_init = None;
        let init = if matches!(
            self.current.kind,
            TokenKind::Var | TokenKind::Let | TokenKind::Const
        ) {
            self.parse_variable_declaration(false)?.value()
        } else if self.current.kind == TokenKind::Semicolon {
            self.tape.push_null()?
        } else {
            let expression = self.parse_expression(false)?;
            expression_init = Some(expression);
            expression.value()
        };

        if matches!(self.current.kind, TokenKind::In | TokenKind::Of) {
            if let Some(expression) = expression_init {
                let assignment_target = self.last_assignment_target;
                let pattern = self.retag_assignment_pattern(expression.node)?;
                if !pattern {
                    self.validate_assignment_target(
                        expression.span,
                        assignment_target,
                        AssignmentTargetPolicy::ForInOf,
                    );
                }
            }
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
            self.context.enter_scope(ScopeKind::Catch);
            let parameter = if self.eat(TokenKind::LeftParen).is_some() {
                let parameter = self.parse_binding_pattern(BindingKind::Lexical)?;
                self.expect(TokenKind::RightParen);
                parameter.value()
            } else {
                self.tape.push_null()?
            };
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
            && Self::is_identifier_name(self.current.kind)
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
                Self::token_span(keyword),
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
        let assignment_patterns = self.assignment_pattern_checkpoint();
        if self.current.kind == TokenKind::Async && self.looks_like_async_arrow() {
            let start = self.take().start;
            let arrow = self.parse_async_arrow_function(start, allow_in);
            self.rollback_assignment_patterns(assignment_patterns);
            return arrow;
        }
        // JavaScript cover grammar handles ordinary arrow heads; only a rest prefix needs binding grammar.
        if self.current.kind == TokenKind::LeftParen
            && (self.options.language.is_typescript() && self.looks_like_parenthesized_arrow()
                || !self.options.language.is_typescript()
                    && self.starts_parenthesized_rest_parameter())
        {
            let start = self.take().start;
            let arrow = self.parse_parenthesized_arrow_function(start, allow_in);
            self.rollback_assignment_patterns(assignment_patterns);
            return arrow;
        }
        if self.current.kind == TokenKind::LeftParen && self.looks_like_empty_arrow() {
            let start = self.take().start;
            self.expect(TokenKind::RightParen);
            self.expect(TokenKind::Arrow);
            let previous_grammar = self.enter_function_context(false, false);
            let arrow = self.parse_arrow_function(start, &[], false, allow_in);
            self.leave_function_context(previous_grammar);
            self.rollback_assignment_patterns(assignment_patterns);
            return arrow;
        }
        let left = self.parse_conditional_expression(allow_in)?;
        if self.eat(TokenKind::Arrow).is_some() {
            if self.reports_ecmascript_early_errors() {
                match self.last_assignment_target {
                    AssignmentTargetType::OptionalChain => {
                        self.error(left.span, "optional chains are not valid arrow parameters");
                    }
                    AssignmentTargetType::WebCompat => {
                        self.error(left.span, "call expressions are not valid arrow parameters");
                    }
                    _ => {}
                }
            }
            let previous_grammar = self.enter_function_context(false, false);
            let arrow =
                self.parse_arrow_function(left.span.start, &[left.value()], false, allow_in);
            self.leave_function_context(previous_grammar);
            self.rollback_assignment_patterns(assignment_patterns);
            return arrow;
        }
        // After a bare `yield`, line-leading `/=` starts a regexp statement.
        if self.current.kind == TokenKind::SlashEq
            && self.current.flags.line_break_before()
            && self.last_node_tag == Some(NodeTag::YIELD_EXPRESSION)
        {
            self.retain_root_assignment_pattern(assignment_patterns, left.node);
            return Ok(left);
        }
        let Some(operator) = assignment_operator(self.current.kind) else {
            self.retain_root_assignment_pattern(assignment_patterns, left.node);
            return Ok(left);
        };
        let assignment_target = self.last_assignment_target;
        self.bump();
        let pattern = if operator == AssignmentOperator::Assign {
            self.retag_assignment_pattern(left.node)?
        } else {
            false
        };
        if !pattern {
            let policy = match operator {
                AssignmentOperator::Assign => AssignmentTargetPolicy::Assignment,
                AssignmentOperator::LogicalOr
                | AssignmentOperator::LogicalAnd
                | AssignmentOperator::Nullish => AssignmentTargetPolicy::LogicalAssignment,
                _ => AssignmentTargetPolicy::CompoundAssignment,
            };
            self.validate_assignment_target(left.span, assignment_target, policy);
        }
        let right = self.parse_assignment_expression(allow_in)?;
        let operator = self.tape.push_u32(operator as u32)?;
        let assignment = self.node(
            NodeTag::ASSIGNMENT_EXPRESSION,
            Span::new(left.span.start, right.span.end),
            &[operator, left.value(), right.value()],
        );
        self.rollback_assignment_patterns(assignment_patterns);
        assignment
    }

    fn parse_parenthesized_arrow_function(
        &mut self,
        start: u32,
        allow_in: bool,
    ) -> Result<ParsedNode, ParseError> {
        let outer_grammar = self.context.grammar();
        let inherited_async_parameters =
            outer_grammar.parameters() && outer_grammar.async_function();
        let inherited_generator_parameters =
            outer_grammar.parameters() && outer_grammar.generator();
        let previous_grammar = self.enter_function_context(false, false);
        self.context.set_grammar(
            self.context
                .grammar()
                .with_parameters(true)
                .with_async_function(inherited_async_parameters)
                .with_generator(inherited_generator_parameters),
        );
        let parameters = self.parse_parameters()?;
        self.expect(TokenKind::RightParen);
        self.expect(TokenKind::Arrow);
        self.context.set_grammar(
            self.context
                .grammar()
                .with_parameters(false)
                .with_async_function(false)
                .with_generator(false),
        );
        if self.reports_ecmascript_early_errors()
            && parameters.has_rest
            && parameters.has_trailing_comma
        {
            self.error(
                self.current_span(),
                "rest parameter cannot have a trailing comma",
            );
        }
        if self.reports_ecmascript_early_errors()
            && !parameters.simple
            && self.current.kind == TokenKind::LeftBrace
            && has_use_strict_directive(self.source, self.current.end as usize)
        {
            self.error(
                self.current_span(),
                "an arrow function with non-simple parameters cannot contain a use strict directive",
            );
        }
        let arrow = self.parse_arrow_function(start, &parameters.values, false, allow_in);
        self.leave_function_context(previous_grammar);
        arrow
    }

    fn parse_async_arrow_function(
        &mut self,
        start: u32,
        allow_in: bool,
    ) -> Result<ParsedNode, ParseError> {
        let previous_grammar = self.enter_function_context(false, true);
        self.context
            .set_grammar(self.context.grammar().with_parameters(true));
        let parenthesized = self.eat(TokenKind::LeftParen).is_some();
        let (parameters, simple_parameters, invalid_rest_trailing_comma) = if parenthesized {
            let parameters = self.parse_parameters()?;
            let invalid_rest_trailing_comma = parameters.has_rest && parameters.has_trailing_comma;
            let simple = parameters.simple;
            let values = parameters.values;
            (values, simple, invalid_rest_trailing_comma)
        } else {
            (
                vec![
                    self.parse_binding_identifier(BindingKind::Parameter)?
                        .value(),
                ],
                true,
                false,
            )
        };
        if parenthesized {
            self.expect(TokenKind::RightParen);
        }
        self.expect(TokenKind::Arrow);
        self.context
            .set_grammar(self.context.grammar().with_parameters(false));
        if self.reports_ecmascript_early_errors() && invalid_rest_trailing_comma {
            self.error(
                self.current_span(),
                "rest parameter cannot have a trailing comma",
            );
        }
        if self.reports_ecmascript_early_errors()
            && !simple_parameters
            && self.current.kind == TokenKind::LeftBrace
            && has_use_strict_directive(self.source, self.current.end as usize)
        {
            self.error(
                self.current_span(),
                "an arrow function with non-simple parameters cannot contain a use strict directive",
            );
        }
        let arrow = self.parse_arrow_function(start, &parameters, true, allow_in);
        self.leave_function_context(previous_grammar);
        arrow
    }

    fn parse_arrow_function(
        &mut self,
        start: u32,
        parameters: &[ValueRef],
        asynchronous: bool,
        allow_in: bool,
    ) -> Result<ParsedNode, ParseError> {
        let expression_body = self.current.kind != TokenKind::LeftBrace;
        let body = if expression_body {
            self.parse_assignment_expression(allow_in)?
        } else {
            self.parse_block_statement()?
        };
        let parameters = self.tape.push_list(parameters)?;
        let asynchronous = self.tape.push_bool(asynchronous)?;
        let expression = self.tape.push_bool(expression_body)?;
        self.node(
            NodeTag::ARROW_FUNCTION_EXPRESSION,
            Span::new(start, body.span.end),
            &[parameters, body.value(), asynchronous, expression],
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
        // TypeScript gives `as` and `satisfies` the same binding threshold as relational `in`.
        const TS_ASSERTION_BINDING: u8 = 14;

        let mut left = self.parse_unary_expression()?;
        loop {
            if self.options.language.is_typescript()
                && minimum <= TS_ASSERTION_BINDING
                && !self.current.flags.line_break_before()
                && !self.current.flags.escaped()
                && matches!(self.current.kind, TokenKind::As | TokenKind::Satisfies)
            {
                let assignment_target = self.last_assignment_target;
                let operator = self.take();
                let type_annotation =
                    if operator.kind == TokenKind::As && self.current.kind == TokenKind::Const {
                        self.parse_const_assertion_type()?
                    } else {
                        self.parse_type()?
                    };
                left = self.node(
                    if operator.kind == TokenKind::Satisfies {
                        NodeTag::TS_SATISFIES_EXPRESSION
                    } else {
                        NodeTag::TS_AS_EXPRESSION
                    },
                    Span::new(left.span.start, type_annotation.span.end),
                    &[left.value(), type_annotation.value()],
                )?;
                self.last_assignment_target = assignment_target;
                continue;
            }

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
        if self.options.language.is_typescript()
            && !self.options.language.is_jsx()
            && self.current.kind == TokenKind::Lt
        {
            return self.parse_type_assertion();
        }
        if let Some(operator) = unary_operator(self.current.kind) {
            let start = self.take().start;
            if self.reports_ecmascript_early_errors()
                && self.context.grammar().generator()
                && self.current.kind == TokenKind::Yield
            {
                self.error(
                    self.current_span(),
                    "yield expressions are not valid unary-expression operands",
                );
            }
            let argument = self.parse_unary_expression()?;
            if operator == UnaryOperator::Delete && self.context.grammar().strict() {
                if self.last_node_tag == Some(NodeTag::IDENTIFIER) {
                    self.error(
                        argument.span,
                        "deleting an unqualified identifier is forbidden in strict mode",
                    );
                } else if self.is_private_member_target(argument)
                    && self.reports_private_early_errors()
                {
                    self.error(argument.span, "deleting a private member is forbidden");
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
            let assignment_target = self.last_assignment_target;
            self.validate_assignment_target(
                argument.span,
                assignment_target,
                AssignmentTargetPolicy::Update,
            );
            let operator = self.tape.push_u32(operator as u32)?;
            let prefix = self.tape.push_bool(true)?;
            return self.node(
                NodeTag::UPDATE_EXPRESSION,
                Span::new(start, argument.span.end),
                &[operator, prefix, argument.value()],
            );
        }
        if self.current.kind == TokenKind::Await && self.context.grammar().allow_await() {
            return self.parse_await_expression();
        }
        if self.current.kind == TokenKind::Yield && self.context.grammar().allow_yield() {
            return self.parse_yield_expression();
        }
        self.parse_postfix_expression()
    }

    fn parse_await_expression(&mut self) -> Result<ParsedNode, ParseError> {
        let token = self.take();
        if self.reports_ecmascript_early_errors() && self.context.grammar().parameters() {
            self.error(
                Self::token_span(token),
                "await expressions are not allowed in formal parameters",
            );
        }
        let argument = self.parse_unary_expression()?;
        self.node(
            NodeTag::AWAIT_EXPRESSION,
            Span::new(token.start, argument.span.end),
            &[argument.value()],
        )
    }

    fn parse_yield_expression(&mut self) -> Result<ParsedNode, ParseError> {
        let token = self.take();
        if self.reports_ecmascript_early_errors() && self.context.grammar().parameters() {
            self.error(
                Self::token_span(token),
                "yield expressions are not allowed in formal parameters",
            );
        }
        if self.reports_ecmascript_early_errors()
            && self.current.kind == TokenKind::Star
            && self.current.flags.line_break_before()
        {
            self.error(
                self.current_span(),
                "a line terminator is not allowed before `*` in a yield expression",
            );
        }
        let delegate =
            !self.current.flags.line_break_before() && self.eat(TokenKind::Star).is_some();
        let missing_argument = matches!(
            self.current.kind,
            TokenKind::Semicolon | TokenKind::RightBrace | TokenKind::Eof
        );
        let argument = if !delegate && (self.current.flags.line_break_before() || missing_argument)
        {
            self.tape.push_null()?
        } else if missing_argument {
            self.error(
                self.current_span(),
                "yield delegation requires an expression",
            );
            self.tape.push_null()?
        } else {
            self.parse_assignment_expression(true)?.value()
        };
        let delegate = self.tape.push_bool(delegate)?;
        self.node(
            NodeTag::YIELD_EXPRESSION,
            Span::new(token.start, self.previous_end(token.start)),
            &[argument, delegate],
        )
    }

    fn parse_type_assertion(&mut self) -> Result<ParsedNode, ParseError> {
        let start = self.expect(TokenKind::Lt).start;
        let type_annotation = self.parse_type()?;
        self.expect_type_greater();
        let expression = self.parse_unary_expression()?;
        let assignment_target = self.last_assignment_target;
        let assertion = self.node(
            NodeTag::TS_TYPE_ASSERTION,
            Span::new(start, expression.span.end),
            &[type_annotation.value(), expression.value()],
        )?;
        self.last_assignment_target = assignment_target;
        Ok(assertion)
    }

    // Postfix operations must stay in precedence order within one left-folding dispatch loop.
    #[allow(clippy::too_many_lines)]
    fn parse_postfix_expression(&mut self) -> Result<ParsedNode, ParseError> {
        self.parse_postfix_expression_until_call(false)
    }

    #[allow(clippy::too_many_lines)]
    fn parse_postfix_expression_until_call(
        &mut self,
        stop_before_call: bool,
    ) -> Result<ParsedNode, ParseError> {
        let mut expression = self.parse_primary_expression()?;
        let mut is_chain = false;
        loop {
            match self.current.kind {
                TokenKind::Dot => {
                    self.bump();
                    let property = self.parse_member_property()?;
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
                        let property = self.parse_member_property()?;
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
                TokenKind::LeftParen if stop_before_call => break,
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
                    if is_chain {
                        // The completed chain is the update operand, not the update expression's root.
                        expression = self.node(
                            NodeTag::CHAIN_EXPRESSION,
                            expression.span,
                            &[expression.value()],
                        )?;
                        self.last_assignment_target = AssignmentTargetType::Invalid;
                        is_chain = false;
                    }
                    let assignment_target = self.last_assignment_target;
                    self.validate_assignment_target(
                        expression.span,
                        assignment_target,
                        AssignmentTargetPolicy::Update,
                    );
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
                TokenKind::Bang
                    if self.options.language.is_typescript()
                        && !self.current.flags.line_break_before() =>
                {
                    let bang = self.take();
                    let assignment_target = self.last_assignment_target;
                    expression = self.node(
                        NodeTag::TS_NON_NULL_EXPRESSION,
                        Span::new(expression.span.start, bang.end),
                        &[expression.value()],
                    )?;
                    self.last_assignment_target = assignment_target;
                }
                _ => break,
            }
            if is_chain {
                self.last_assignment_target =
                    if self.last_node_tag == Some(NodeTag::MEMBER_EXPRESSION) {
                        AssignmentTargetType::OptionalChain
                    } else {
                        AssignmentTargetType::Invalid
                    };
            }
        }
        if is_chain {
            let assignment_target = self.last_assignment_target;
            expression = self.node(
                NodeTag::CHAIN_EXPRESSION,
                expression.span,
                &[expression.value()],
            )?;
            self.last_assignment_target = assignment_target;
        }
        Ok(expression)
    }

    fn parse_primary_expression(&mut self) -> Result<ParsedNode, ParseError> {
        match self.current.kind {
            TokenKind::Async if self.followed_by_token_without_line_break(TokenKind::Function) => {
                self.parse_function(false, true)
            }
            kind if Self::is_identifier_name(kind) => self.parse_identifier_reference(),
            TokenKind::Yield if !self.context.grammar().allow_yield() => {
                self.parse_identifier_reference()
            }
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
                self.node(NodeTag::THIS_EXPRESSION, Self::token_span(token), &[])
            }
            TokenKind::Super => {
                let token = self.take();
                if !matches!(
                    self.current.kind,
                    TokenKind::LeftParen | TokenKind::Dot | TokenKind::LeftBracket
                ) {
                    self.error(
                        Self::token_span(token),
                        "super must be followed by a call or property access",
                    );
                }
                if self.reports_ecmascript_early_errors() && !self.context.grammar().allow_super() {
                    self.error(
                        Self::token_span(token),
                        "super is only allowed in methods and derived constructors",
                    );
                }
                if self.current.kind == TokenKind::LeftParen
                    && (self.context.grammar().accessor()
                        || self.reports_ecmascript_early_errors()
                            && !self.context.grammar().allow_super_call())
                {
                    self.error(
                        Self::token_span(token),
                        "direct super calls are only allowed in class constructors",
                    );
                }
                self.node(NodeTag::SUPER, Self::token_span(token), &[])
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

    // JSX opening, child, and closing modes coordinate lexer state in one grammar routine.
    #[allow(clippy::too_many_lines)]
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
                        let value = self.tape.push_source_slice(Self::token_span(text))?;
                        children.push(
                            self.node(NodeTag::JSX_TEXT, Self::token_span(text), &[value])?
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
        if !Self::is_identifier_name(token.kind) {
            self.error(Self::token_span(token), "expected a JSX name");
        }
        let name = self.tape.push_source_slice(Self::token_span(token))?;
        self.node(NodeTag::JSX_IDENTIFIER, Self::token_span(token), &[name])
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
        let assignment_patterns = self.assignment_pattern_checkpoint();
        let start = self.take().start;
        let expression = self.parse_expression(true)?;
        let semantic_tag = self.last_node_tag;
        let assignment_target = self.last_assignment_target;
        let end = self.expect(TokenKind::RightParen).end;
        // Parentheses preserve simple targets but stop cover expressions from becoming patterns.
        self.rollback_assignment_patterns(assignment_patterns);
        if self.options.preserve_parentheses {
            let parenthesized = self.node(
                NodeTag::PARENTHESIZED_EXPRESSION,
                Span::new(start, end),
                &[expression.value()],
            )?;
            self.last_node_tag = semantic_tag;
            self.last_assignment_target = assignment_target;
            Ok(parenthesized)
        } else {
            self.last_assignment_target = assignment_target;
            Ok(expression)
        }
    }

    fn parse_array_expression(&mut self) -> Result<ParsedNode, ParseError> {
        let assignment_patterns = self.assignment_pattern_checkpoint();
        let start = self.take().start;
        let mut elements = Vec::new();
        while !matches!(self.current.kind, TokenKind::RightBracket | TokenKind::Eof) {
            if self.eat(TokenKind::Comma).is_some() {
                elements.push(self.tape.push_null()?);
                continue;
            }
            let (element, assignment_target) = if self.eat(TokenKind::Ellipsis).is_some() {
                let argument = self.parse_assignment_expression(true)?;
                let assignment_target = self.last_assignment_target;
                let spread =
                    self.node(NodeTag::SPREAD_ELEMENT, argument.span, &[argument.value()])?;
                self.assignment_pattern_candidates
                    .push(AssignmentPatternCandidate {
                        node: spread.node,
                        tag: NodeTag::REST_ELEMENT,
                        group_start: usize::MAX,
                        error: None,
                    });
                (spread, assignment_target)
            } else {
                let element = self.parse_assignment_expression(true)?;
                (element, self.last_assignment_target)
            };
            if assignment_target == AssignmentTargetType::OptionalChain {
                self.assignment_pattern_candidates
                    .push(AssignmentPatternCandidate {
                        node: element.node,
                        tag: NodeTag::CHAIN_EXPRESSION,
                        group_start: usize::MAX,
                        error: Some(AssignmentPatternError::InvalidTarget(element.span)),
                    });
            }
            elements.push(element.value());
            if self.eat(TokenKind::Comma).is_none() {
                break;
            }
        }
        let end = self.expect(TokenKind::RightBracket).end;
        let elements = self.tape.push_list(&elements)?;
        let expression = self.node(
            NodeTag::ARRAY_EXPRESSION,
            Span::new(start, end),
            &[elements],
        )?;
        self.register_assignment_pattern(
            assignment_patterns,
            expression.node,
            NodeTag::ARRAY_PATTERN,
        );
        Ok(expression)
    }

    #[allow(clippy::too_many_lines)]
    fn parse_object_expression(&mut self) -> Result<ParsedNode, ParseError> {
        let assignment_patterns = self.assignment_pattern_checkpoint();
        let start = self.take().start;
        let mut properties = Vec::new();
        while !matches!(self.current.kind, TokenKind::RightBrace | TokenKind::Eof) {
            if self.eat(TokenKind::Ellipsis).is_some() {
                let argument = self.parse_assignment_expression(true)?;
                let assignment_target = self.last_assignment_target;
                let spread =
                    self.node(NodeTag::SPREAD_ELEMENT, argument.span, &[argument.value()])?;
                self.assignment_pattern_candidates
                    .push(AssignmentPatternCandidate {
                        node: spread.node,
                        tag: NodeTag::REST_ELEMENT,
                        group_start: usize::MAX,
                        error: None,
                    });
                if assignment_target == AssignmentTargetType::OptionalChain {
                    self.assignment_pattern_candidates
                        .push(AssignmentPatternCandidate {
                            node: spread.node,
                            tag: NodeTag::CHAIN_EXPRESSION,
                            group_start: usize::MAX,
                            error: Some(AssignmentPatternError::InvalidTarget(argument.span)),
                        });
                }
                properties.push(spread.value());
            } else if let Some(accessor) = self.current_accessor_kind(false) {
                let property_start = self.current.start;
                let property = self.parse_object_accessor(property_start, accessor)?;
                // This marker reports only if cover grammar later reinterprets the object as a pattern.
                self.assignment_pattern_candidates
                    .push(AssignmentPatternCandidate {
                        node: property.node,
                        tag: NodeTag::PROPERTY,
                        group_start: usize::MAX,
                        error: Some(AssignmentPatternError::Accessor(property.span)),
                    });
                properties.push(property.value());
            } else {
                let property_start = self.current.start;
                let property_name_escaped = self.current.flags.escaped();
                let async_token = if self.current.kind == TokenKind::Async {
                    Some(self.take())
                } else {
                    None
                };
                let async_modifier = async_token.is_some_and(|token| {
                    !token.flags.escaped()
                        && !self.current.flags.line_break_before()
                        && (self.current.kind == TokenKind::Star
                            || Self::is_property_name_start(self.current.kind, false))
                });
                let generator = self.current.kind == TokenKind::Star
                    && async_token.is_none_or(|_| async_modifier);
                let asynchronous = async_modifier;
                if generator {
                    self.bump();
                }
                let property_name = if generator || asynchronous {
                    self.parse_property_name(false)?
                } else if let Some(token) = async_token {
                    ParsedPropertyName {
                        key: self.identifier_from_span(Self::token_span(token))?,
                        computed: false,
                        shorthand: true,
                    }
                } else {
                    self.parse_property_name(false)?
                };
                let key = property_name.key;
                let computed = property_name.computed;
                let (value, method, shorthand, assignment_target) = if generator || asynchronous {
                    let method_patterns = self.assignment_pattern_checkpoint();
                    let value = self.parse_method_function_with_super_call(
                        key.span.start,
                        generator,
                        asynchronous,
                        None,
                        false,
                        MethodBodyPolicy::Block,
                    )?;
                    self.rollback_assignment_patterns(method_patterns);
                    (value, true, false, self.last_assignment_target)
                } else if self.current.kind == TokenKind::LeftParen {
                    let method_patterns = self.assignment_pattern_checkpoint();
                    let value = self.parse_method_function_with_super_call(
                        key.span.start,
                        false,
                        false,
                        None,
                        false,
                        MethodBodyPolicy::Block,
                    )?;
                    self.rollback_assignment_patterns(method_patterns);
                    (value, true, false, self.last_assignment_target)
                } else if self.eat(TokenKind::Colon).is_some() {
                    let value = self.parse_assignment_expression(true)?;
                    (value, false, false, self.last_assignment_target)
                } else {
                    if property_name.shorthand {
                        self.report_identifier_reference_early_errors(
                            key.span,
                            property_name_escaped,
                        );
                    } else {
                        self.error(key.span, "property name requires `:` or method parameters");
                    }
                    let mut value = self.identifier_from_span(key.span)?;
                    if self.eat(TokenKind::Eq).is_some() {
                        let default_patterns = self.assignment_pattern_checkpoint();
                        let right = self.parse_assignment_expression(true)?;
                        self.rollback_assignment_patterns(default_patterns);
                        value = self.node(
                            NodeTag::ASSIGNMENT_PATTERN,
                            Span::new(key.span.start, right.span.end),
                            &[value.value(), right.value()],
                        )?;
                    }
                    (
                        value,
                        false,
                        property_name.shorthand,
                        self.last_assignment_target,
                    )
                };
                if assignment_target == AssignmentTargetType::OptionalChain {
                    self.assignment_pattern_candidates
                        .push(AssignmentPatternCandidate {
                            node: value.node,
                            tag: NodeTag::CHAIN_EXPRESSION,
                            group_start: usize::MAX,
                            error: Some(AssignmentPatternError::InvalidTarget(value.span)),
                        });
                }
                let kind = self.tape.push_u32(0)?;
                let method = self.tape.push_bool(method)?;
                let shorthand = self.tape.push_bool(shorthand)?;
                let computed = self.tape.push_bool(computed)?;
                properties.push(
                    self.node(
                        NodeTag::PROPERTY,
                        Span::new(property_start, value.span.end),
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
        let expression = self.node(
            NodeTag::OBJECT_EXPRESSION,
            Span::new(start, end),
            &[properties],
        )?;
        self.register_assignment_pattern(
            assignment_patterns,
            expression.node,
            NodeTag::OBJECT_PATTERN,
        );
        Ok(expression)
    }

    fn parse_object_accessor(
        &mut self,
        start: u32,
        accessor: AccessorKind,
    ) -> Result<ParsedNode, ParseError> {
        self.bump();
        let property_name = self.parse_property_name(false)?;
        let method_patterns = self.assignment_pattern_checkpoint();
        let function = self.parse_method_function_with_super_call(
            property_name.key.span.start,
            false,
            false,
            Some(accessor),
            false,
            MethodBodyPolicy::Block,
        )?;
        self.rollback_assignment_patterns(method_patterns);
        let kind = self.tape.push_u32(accessor.method_kind())?;
        let method = self.tape.push_bool(false)?;
        let shorthand = self.tape.push_bool(false)?;
        let computed = self.tape.push_bool(property_name.computed)?;
        self.node(
            NodeTag::PROPERTY,
            Span::new(start, function.span.end),
            &[
                property_name.key.value(),
                function.value(),
                kind,
                method,
                shorthand,
                computed,
            ],
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
        let direct_import_call =
            self.current.kind == TokenKind::Import && self.import_starts_direct_call();
        let callee = self.parse_postfix_expression_until_call(true)?;
        if direct_import_call {
            self.error(
                callee.span,
                "import calls cannot be used directly as new callees",
            );
        }
        if self.options.language.is_typescript()
            && matches!(self.current.kind, TokenKind::Lt | TokenKind::ShiftLeft)
        {
            return self.parse_typescript_new_expression(start, callee);
        }
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

    #[cold]
    fn parse_typescript_new_expression(
        &mut self,
        start: u32,
        callee: ParsedNode,
    ) -> Result<ParsedNode, ParseError> {
        debug_assert!(self.options.language.is_typescript());
        debug_assert!(matches!(
            self.current.kind,
            TokenKind::Lt | TokenKind::ShiftLeft
        ));

        // Type arguments overlap relational expressions, so every speculative side effect must roll back together.
        let current = self.current;
        let lexer = self.lexer.checkpoint();
        let tape = self.tape.checkpoint();
        let context = self.context.checkpoint();
        let assignment_patterns = self.assignment_pattern_checkpoint();
        let last_node_tag = self.last_node_tag;
        let last_assignment_target = self.last_assignment_target;

        if self.current.kind == TokenKind::ShiftLeft {
            self.current.kind = TokenKind::Lt;
            self.current.end = self.current.start + 1;
            self.lexer.set_position(self.current.end as usize);
        }
        let (type_arguments, end, closed, compound_closer) = self.parse_new_type_arguments()?;
        if closed && compound_closer.is_none() && self.can_follow_new_type_arguments() {
            self.context.commit(context);
            if self.current.kind == TokenKind::Dot {
                self.error(
                    self.current_span(),
                    "property access cannot directly follow new-expression type arguments",
                );
            }
            if self.current.kind == TokenKind::QuestionDot {
                self.error(
                    self.current_span(),
                    "an optional chain cannot directly follow a new expression",
                );
            }
            let (arguments, end) = if self.eat(TokenKind::LeftParen).is_some() {
                let arguments = self.parse_argument_list()?;
                let right_paren = self.expect(TokenKind::RightParen);
                let end = if right_paren.kind == TokenKind::RightParen {
                    right_paren.end
                } else {
                    right_paren.start.max(callee.span.end)
                };
                (arguments, end)
            } else {
                (self.tape.push_list(&[])?, end)
            };
            return self.node(
                NodeTag::TS_NEW_EXPRESSION,
                Span::new(start, end),
                &[callee.value(), arguments, type_arguments],
            );
        }

        self.tape.rollback(tape)?;
        self.context.rollback(context);
        self.lexer.rollback(lexer);
        self.current = current;
        self.rollback_assignment_patterns(assignment_patterns);
        self.last_node_tag = last_node_tag;
        self.last_assignment_target = last_assignment_target;
        if let Some(token) = compound_closer.filter(|token| {
            matches!(
                token.kind,
                TokenKind::ShiftRightEq | TokenKind::ShiftRightUnsignedEq
            )
        }) {
            self.error(
                Self::token_span(token),
                "invalid assignment target after a new expression",
            );
        }
        let arguments = self.tape.push_list(&[])?;
        self.node(
            NodeTag::NEW_EXPRESSION,
            Span::new(start, self.previous_end(callee.span.end)),
            &[callee.value(), arguments],
        )
    }

    fn parse_import_expression_or_meta(&mut self) -> Result<ParsedNode, ParseError> {
        let import = self.take();
        if self.eat(TokenKind::Dot).is_some() {
            let property_token = self.current;
            let property_span = Self::token_span(property_token);
            let property_name =
                (!property_token.flags.escaped()).then(|| self.source_text(property_span));
            let is_meta = property_name == Some("meta");
            let phase = match property_name {
                Some("source") => Some(ImportPhase::Source),
                Some("defer") => Some(ImportPhase::Defer),
                _ => None,
            };
            if let Some(phase) = phase {
                self.bump();
                return self.parse_phase_import_expression(import.start, property_span.end, phase);
            }

            let property = self.parse_identifier()?;
            let meta = self.identifier_from_span(Self::token_span(import))?;
            if !is_meta {
                self.error(
                    property.span,
                    "the import meta-property must be `import.meta`",
                );
            } else if !matches!(
                self.options.source_kind,
                SourceKind::Module | SourceKind::Unambiguous
            ) {
                self.error(property.span, "import.meta is only allowed in modules");
            }
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

    fn parse_phase_import_expression(
        &mut self,
        start: u32,
        property_end: u32,
        phase: ImportPhase,
    ) -> Result<ParsedNode, ParseError> {
        if self.eat(TokenKind::LeftParen).is_none() {
            self.error(
                self.current_span(),
                format!("`import.{}` must be called", phase.name()),
            );
            let source = self.tape.push_null()?;
            let options = self.tape.push_null()?;
            let phase = self.tape.push_u32(phase.wire_value())?;
            return self.node(
                NodeTag::PHASE_IMPORT_EXPRESSION,
                Span::new(start, property_end),
                &[source, options, phase],
            );
        }

        let mut arguments = Vec::new();
        let mut argument_count = 0_usize;
        while !matches!(self.current.kind, TokenKind::RightParen | TokenKind::Eof) {
            let tape_checkpoint = self.tape.checkpoint();
            let assignment_patterns = self.assignment_pattern_checkpoint();
            let argument = if let Some(spread) = self.eat(TokenKind::Ellipsis) {
                self.error(
                    Self::token_span(spread),
                    format!(
                        "spread arguments are not allowed in `import.{}`",
                        phase.name()
                    ),
                );
                let argument = self.parse_assignment_expression(true)?;
                self.node(NodeTag::SPREAD_ELEMENT, argument.span, &[argument.value()])?
            } else {
                self.parse_assignment_expression(true)?
            };
            argument_count += 1;
            if arguments.len() < 2 {
                arguments.push(argument.value());
            } else {
                self.tape.rollback(tape_checkpoint)?;
                self.rollback_assignment_patterns(assignment_patterns);
            }
            if self.eat(TokenKind::Comma).is_none() {
                break;
            }
        }
        let end = self.expect(TokenKind::RightParen).end;
        if !(1..=2).contains(&argument_count) {
            self.error(
                Span::new(start, end),
                format!(
                    "`import.{}` requires exactly one or two arguments",
                    phase.name()
                ),
            );
        }
        let source = if let Some(&source) = arguments.first() {
            source
        } else {
            self.tape.push_null()?
        };
        let options = if let Some(&options) = arguments.get(1) {
            options
        } else {
            self.tape.push_null()?
        };
        let phase = self.tape.push_u32(phase.wire_value())?;
        self.node(
            NodeTag::PHASE_IMPORT_EXPRESSION,
            Span::new(start, end),
            &[source, options, phase],
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
            let raw = self.tape.push_source_slice(Self::token_span(first))?;
            let tail = self.tape.push_bool(true)?;
            quasis.push(
                self.node(
                    NodeTag::TEMPLATE_ELEMENT,
                    Self::token_span(first),
                    &[raw, tail],
                )?
                .value(),
            );
            let quasis = self.tape.push_list(&quasis)?;
            let expressions = self.tape.push_list(&expressions)?;
            return self.node(
                NodeTag::TEMPLATE_LITERAL,
                Self::token_span(first),
                &[quasis, expressions],
            );
        }

        self.bump();
        let raw = self.tape.push_source_slice(Self::token_span(first))?;
        let tail = self.tape.push_bool(false)?;
        quasis.push(
            self.node(
                NodeTag::TEMPLATE_ELEMENT,
                Self::token_span(first),
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
            let raw = self.tape.push_source_slice(Self::token_span(segment))?;
            let tail = self.tape.push_bool(is_tail)?;
            quasis.push(
                self.node(
                    NodeTag::TEMPLATE_ELEMENT,
                    Self::token_span(segment),
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
        let raw = self.tape.push_source_slice(Self::token_span(token))?;
        let kind = self.tape.push_u32(match token.kind {
            TokenKind::String => 1,
            TokenKind::True | TokenKind::False => 2,
            TokenKind::Null => 3,
            TokenKind::BigInt => 4,
            TokenKind::NoSubstitutionTemplate => 5,
            _ => 0,
        })?;
        self.node(NodeTag::LITERAL, Self::token_span(token), &[raw, kind])
    }

    fn parse_regexp_literal(&mut self) -> Result<ParsedNode, ParseError> {
        let slash = self.current;
        let flag_errors = self.reports_ecmascript_early_errors();
        let token = self.lexer.scan_regexp_with_flag_errors(slash, flag_errors);
        self.current = self.lexer.next_token();
        let raw = self.tape.push_source_slice(Self::token_span(token))?;
        let kind = self.tape.push_u32(6)?;
        self.node(NodeTag::LITERAL, Self::token_span(token), &[raw, kind])
    }

    fn parse_identifier(&mut self) -> Result<ParsedNode, ParseError> {
        let token = self.take();
        if !Self::is_identifier_name(token.kind) {
            self.error(Self::token_span(token), "expected an identifier");
        }
        let name = self.tape.push_source_slice(Self::token_span(token))?;
        self.node(NodeTag::IDENTIFIER, Self::token_span(token), &[name])
    }

    fn parse_identifier_reference(&mut self) -> Result<ParsedNode, ParseError> {
        let token = self.current;
        let span = Self::token_span(token);
        self.report_identifier_reference_early_errors(span, token.flags.escaped());
        if !self.reports_ecmascript_early_errors()
            && self.context.grammar().strict()
            && self.is_strict_reserved_identifier(span, token.flags.escaped())
        {
            self.error(
                span,
                "strict mode reserved word cannot be used as an identifier",
            );
        }
        let restricted_assignment_target = self.reports_ecmascript_early_errors()
            && self.context.grammar().strict()
            && (self.identifier_name_matches(span, "eval", token.flags.escaped())
                || self.identifier_name_matches(span, "arguments", token.flags.escaped()));
        self.bump();
        let name = self.tape.push_source_slice(span)?;
        let identifier = self.node(NodeTag::IDENTIFIER, span, &[name])?;
        if restricted_assignment_target {
            self.last_assignment_target = AssignmentTargetType::RestrictedIdentifier;
        }
        Ok(identifier)
    }

    fn report_identifier_reference_early_errors(&mut self, span: Span, escaped: bool) {
        if !self.reports_ecmascript_early_errors() {
            return;
        }
        if self.is_escaped_reserved_identifier(span, escaped) {
            self.error(span, "reserved word cannot be used as an identifier");
        }
        let await_reserved = self.context.grammar().async_function()
            || self.context.grammar().module()
            || self.context.grammar().class() && !self.context.grammar().function();
        if await_reserved && self.identifier_name_matches(span, "await", escaped) {
            self.error(
                span,
                "await cannot be used as an identifier in an async function",
            );
        }
        if self.context.grammar().generator()
            && self.identifier_name_matches(span, "yield", escaped)
        {
            self.error(
                span,
                "yield cannot be used as an identifier in a generator function",
            );
        }
        if self.context.grammar().strict() && self.is_strict_reserved_identifier(span, escaped) {
            self.error(
                span,
                "strict mode reserved word cannot be used as an identifier",
            );
        }
    }

    fn parse_member_property(&mut self) -> Result<ParsedNode, ParseError> {
        if self.current.kind == TokenKind::PrivateIdentifier {
            let (node, name) = self.parse_private_identifier()?;
            let name_span = Span::new(node.span.start.saturating_add(1), node.span.end);
            let _ = self.context.use_private(name, name_span);
            return Ok(node);
        }
        let token = self.take();
        self.parse_identifier_name_from(token)
    }

    fn parse_identifier_name(&mut self) -> Result<ParsedNode, ParseError> {
        let token = self.take();
        self.parse_identifier_name_from(token)
    }

    fn parse_identifier_name_from(&mut self, token: Token) -> Result<ParsedNode, ParseError> {
        if !Self::is_member_identifier_name(token.kind) {
            self.error(Self::token_span(token), "expected an identifier");
        }
        self.identifier_from_span(Self::token_span(token))
    }

    fn parse_class_private_identifier(&mut self) -> Result<ParsedNode, ParseError> {
        let (node, name) = self.parse_private_identifier()?;
        let name_span = Span::new(node.span.start.saturating_add(1), node.span.end);
        if name == "constructor" && self.reports_private_early_errors() {
            self.error(node.span, "private name `#constructor` is not allowed");
        }
        let _ = self.context.declare_private(name, name_span);
        Ok(node)
    }

    fn parse_private_identifier(&mut self) -> Result<(ParsedNode, Cow<'s, str>), ParseError> {
        let token = self.take();
        let span = Self::token_span(token);
        let name_span = Span::new(token.start.saturating_add(1), token.end);
        let raw = self.source_text(name_span);
        let (name, value) = if token.flags.escaped() {
            let name = decode_static_property_name(raw).unwrap_or_else(|| raw.to_owned());
            let value = self.tape.push_string(&name)?;
            (Cow::Owned(name), value)
        } else {
            (Cow::Borrowed(raw), self.tape.push_source_slice(name_span)?)
        };
        let node = self.node(NodeTag::PRIVATE_IDENTIFIER, span, &[value])?;
        Ok((node, name))
    }

    fn parse_property_name(
        &mut self,
        class_element: bool,
    ) -> Result<ParsedPropertyName, ParseError> {
        if self.eat(TokenKind::LeftBracket).is_some() {
            let assignment_patterns = self.assignment_pattern_checkpoint();
            let key = self.parse_assignment_expression(true)?;
            self.expect(TokenKind::RightBracket);
            self.rollback_assignment_patterns(assignment_patterns);
            return Ok(ParsedPropertyName {
                key,
                computed: true,
                shorthand: false,
            });
        }
        let shorthand = Self::is_identifier_name(self.current.kind);
        let key = if class_element && self.current.kind == TokenKind::PrivateIdentifier {
            self.parse_class_private_identifier()?
        } else if matches!(
            self.current.kind,
            TokenKind::String | TokenKind::Number | TokenKind::BigInt
        ) {
            self.parse_literal()?
        } else {
            self.parse_identifier_name()?
        };
        Ok(ParsedPropertyName {
            key,
            computed: false,
            shorthand,
        })
    }

    fn identifier_from_span(&mut self, span: Span) -> Result<ParsedNode, ParseError> {
        let name = self.tape.push_source_slice(span)?;
        self.node(NodeTag::IDENTIFIER, span, &[name])
    }

    fn parse_binding_identifier(
        &mut self,
        binding_kind: BindingKind,
    ) -> Result<ParsedNode, ParseError> {
        self.parse_binding_identifier_impl(binding_kind, false)
    }

    fn parse_binding_identifier_with_optional(
        &mut self,
        binding_kind: BindingKind,
    ) -> Result<ParsedNode, ParseError> {
        self.parse_binding_identifier_impl(binding_kind, true)
    }

    fn parse_binding_identifier_impl(
        &mut self,
        binding_kind: BindingKind,
        allow_optional: bool,
    ) -> Result<ParsedNode, ParseError> {
        let token = self.take();
        let span = Self::token_span(token);
        let escaped = token.flags.escaped();
        if !Self::is_identifier_name(token.kind) {
            self.error(span, "expected a binding identifier");
        }
        let name_text = self
            .source
            .get(token.start as usize..token.end as usize)
            .unwrap_or_default();
        let await_reserved = self.context.grammar().async_function()
            || self.context.grammar().module()
            || self.context.grammar().class() && !self.context.grammar().function();
        if self.reports_ecmascript_early_errors()
            && self.is_escaped_reserved_identifier(span, escaped)
        {
            self.error(span, "reserved word cannot be used as a binding identifier");
        }
        if self.reports_ecmascript_early_errors()
            && await_reserved
            && self.identifier_name_matches(span, "await", escaped)
        {
            self.error(span, "await cannot be bound in an async function");
        }
        if self.reports_ecmascript_early_errors()
            && self.context.grammar().generator()
            && self.identifier_name_matches(span, "yield", escaped)
        {
            self.error(span, "yield cannot be bound in a generator function");
        }
        if self.context.grammar().strict()
            && (matches!(name_text, "eval" | "arguments")
                || self.is_strict_reserved_identifier(span, escaped))
        {
            self.error(span, "identifier cannot be bound in strict mode");
        }
        let _ = self.context.declare_binding(name_text, binding_kind, span);
        let name = self.tape.push_source_slice(span)?;
        let optional_marker = if self.options.language.is_typescript() && allow_optional {
            self.eat(TokenKind::Question)
        } else {
            None
        };
        let annotation =
            if self.options.language.is_typescript() && self.eat(TokenKind::Colon).is_some() {
                Some(self.parse_type_annotation()?)
            } else {
                None
            };
        if optional_marker.is_some() || annotation.is_some() {
            let end = annotation.as_ref().map_or_else(
                || {
                    optional_marker
                        .as_ref()
                        .map_or(token.end, |marker| marker.end)
                },
                |annotation| annotation.span.end,
            );
            let annotation = if let Some(annotation) = annotation {
                annotation.value()
            } else {
                self.tape.push_null()?
            };
            let optional = self.tape.push_bool(optional_marker.is_some())?;
            return self.node(
                NodeTag::IDENTIFIER,
                Span::new(token.start, end),
                &[name, annotation, optional],
            );
        }
        self.node(NodeTag::IDENTIFIER, span, &[name])
    }

    // Array and object pattern recovery is mutually recursive and benefits from staying adjacent.
    #[allow(clippy::too_many_lines)]
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
                    let rest = self.eat(TokenKind::Ellipsis).is_some();
                    let element = if rest {
                        let argument = self.parse_binding_pattern(binding_kind)?;
                        self.parse_binding_rest_element(argument)?
                    } else {
                        self.parse_binding_element(binding_kind)?
                    };
                    elements.push(element.value());
                    let Some(comma) = self.eat(TokenKind::Comma) else {
                        break;
                    };
                    if rest {
                        self.error(Self::token_span(comma), "rest element must be last");
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
                    let rest = self.eat(TokenKind::Ellipsis).is_some();
                    if rest {
                        let argument = self.parse_binding_identifier(binding_kind)?;
                        properties.push(self.parse_binding_rest_element(argument)?.value());
                    } else {
                        let property_start = self.current.start;
                        let property_name = self.parse_property_name(false)?;
                        let key = property_name.key;
                        let shorthand =
                            property_name.shorthand && self.current.kind != TokenKind::Colon;
                        let value = if self.eat(TokenKind::Colon).is_some() {
                            self.parse_binding_element(binding_kind)?
                        } else {
                            if !property_name.shorthand {
                                self.error(key.span, "property name requires a binding target");
                            }
                            let binding =
                                self.binding_identifier_from_span(key.span, binding_kind)?;
                            self.parse_binding_default(binding)?
                        };
                        let property_kind = self.tape.push_u32(0)?;
                        let method = self.tape.push_bool(false)?;
                        let shorthand = self.tape.push_bool(shorthand)?;
                        let computed = self.tape.push_bool(property_name.computed)?;
                        properties.push(
                            self.node(
                                NodeTag::PROPERTY,
                                Span::new(property_start, value.span.end),
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
                    let Some(comma) = self.eat(TokenKind::Comma) else {
                        break;
                    };
                    if rest {
                        self.error(Self::token_span(comma), "rest property must be last");
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
            _ => self.parse_binding_identifier(binding_kind),
        }
    }

    fn parse_binding_element(
        &mut self,
        binding_kind: BindingKind,
    ) -> Result<ParsedNode, ParseError> {
        let pattern = self.parse_binding_pattern(binding_kind)?;
        self.parse_binding_default(pattern)
    }

    fn parse_binding_default(&mut self, pattern: ParsedNode) -> Result<ParsedNode, ParseError> {
        if self.eat(TokenKind::Eq).is_none() {
            return Ok(pattern);
        }
        let right = self.parse_assignment_expression(true)?;
        self.node(
            NodeTag::ASSIGNMENT_PATTERN,
            Span::new(pattern.span.start, right.span.end),
            &[pattern.value(), right.value()],
        )
    }

    fn parse_binding_rest_element(
        &mut self,
        mut argument: ParsedNode,
    ) -> Result<ParsedNode, ParseError> {
        if let Some(equals) = self.eat(TokenKind::Eq) {
            self.error(
                Self::token_span(equals),
                "rest element cannot have a default",
            );
            let right = self.parse_assignment_expression(true)?;
            argument = self.node(
                NodeTag::ASSIGNMENT_PATTERN,
                Span::new(argument.span.start, right.span.end),
                &[argument.value(), right.value()],
            )?;
        }
        self.node(NodeTag::REST_ELEMENT, argument.span, &[argument.value()])
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
        let escaped = name_text.contains('\\');
        if self.reports_ecmascript_early_errors()
            && self.is_escaped_reserved_identifier(span, escaped)
        {
            self.error(span, "reserved word cannot be used as a binding identifier");
        }
        if self.context.grammar().strict()
            && (matches!(name_text, "eval" | "arguments")
                || self.is_strict_reserved_identifier(span, escaped))
        {
            self.error(span, "identifier cannot be bound in strict mode");
        }
        let _ = self.context.declare_binding(name_text, binding_kind, span);
        self.identifier_from_span(span)
    }

    fn parse_type_annotation(&mut self) -> Result<ParsedNode, ParseError> {
        let annotation = self.parse_type()?;
        self.node(
            NodeTag::TS_TYPE_ANNOTATION,
            annotation.span,
            &[annotation.value()],
        )
    }

    fn parse_type(&mut self) -> Result<ParsedNode, ParseError> {
        let check_type = self.parse_union_type()?;
        if self.eat(TokenKind::Extends).is_none() {
            return Ok(check_type);
        }

        let extends_type = self.parse_union_type()?;
        self.expect(TokenKind::Question);
        let true_type = self.parse_type()?;
        self.expect(TokenKind::Colon);
        let false_type = self.parse_type()?;
        self.node(
            NodeTag::TS_CONDITIONAL_TYPE,
            Span::new(check_type.span.start, false_type.span.end),
            &[
                check_type.value(),
                extends_type.value(),
                true_type.value(),
                false_type.value(),
            ],
        )
    }

    fn parse_union_type(&mut self) -> Result<ParsedNode, ParseError> {
        let first = self.parse_intersection_type()?;
        if self.eat(TokenKind::Pipe).is_none() {
            return Ok(first);
        }

        let mut types = vec![first.value()];
        let end = loop {
            let item = self.parse_intersection_type()?;
            let item_end = item.span.end;
            types.push(item.value());
            if self.eat(TokenKind::Pipe).is_none() {
                break item_end;
            }
        };
        let types = self.tape.push_list(&types)?;
        self.node(
            NodeTag::TS_UNION_TYPE,
            Span::new(first.span.start, end),
            &[types],
        )
    }

    fn parse_intersection_type(&mut self) -> Result<ParsedNode, ParseError> {
        let first = self.parse_type_postfix()?;
        if self.eat(TokenKind::Amp).is_none() {
            return Ok(first);
        }

        let mut types = vec![first.value()];
        let end = loop {
            let item = self.parse_type_postfix()?;
            let item_end = item.span.end;
            types.push(item.value());
            if self.eat(TokenKind::Amp).is_none() {
                break item_end;
            }
        };
        let types = self.tape.push_list(&types)?;
        self.node(
            NodeTag::TS_INTERSECTION_TYPE,
            Span::new(first.span.start, end),
            &[types],
        )
    }

    fn parse_type_postfix(&mut self) -> Result<ParsedNode, ParseError> {
        let mut type_node = if matches!(
            self.current.kind,
            TokenKind::Keyof | TokenKind::Readonly | TokenKind::Unique
        ) {
            let operator = self.take();
            let annotation = self.parse_type_postfix()?;
            let operator_name = self.tape.push_source_slice(Self::token_span(operator))?;
            self.node(
                NodeTag::TS_TYPE_OPERATOR,
                Span::new(operator.start, annotation.span.end),
                &[operator_name, annotation.value()],
            )?
        } else {
            self.parse_type_primary()?
        };

        while self.eat(TokenKind::LeftBracket).is_some() {
            if let Some(right_bracket) = self.eat(TokenKind::RightBracket) {
                type_node = self.node(
                    NodeTag::TS_ARRAY_TYPE,
                    Span::new(type_node.span.start, right_bracket.end),
                    &[type_node.value()],
                )?;
                continue;
            }

            let index_type = self.parse_type()?;
            let end = self.expect(TokenKind::RightBracket).end;
            type_node = self.node(
                NodeTag::TS_INDEXED_ACCESS_TYPE,
                Span::new(type_node.span.start, end),
                &[type_node.value(), index_type.value()],
            )?;
        }
        Ok(type_node)
    }

    fn parse_type_primary(&mut self) -> Result<ParsedNode, ParseError> {
        if let Some(tag) = self.current_type_keyword_tag() {
            let token = self.take();
            return self.node(tag, Self::token_span(token), &[]);
        }

        match self.current.kind {
            TokenKind::String
            | TokenKind::Number
            | TokenKind::BigInt
            | TokenKind::True
            | TokenKind::False => {
                let literal = self.parse_literal()?;
                self.node(NodeTag::TS_LITERAL_TYPE, literal.span, &[literal.value()])
            }
            TokenKind::LeftParen if self.looks_like_function_type() => {
                self.parse_function_type(false)
            }
            TokenKind::LeftParen => {
                let start = self.take().start;
                let annotation = self.parse_type()?;
                let end = self.expect(TokenKind::RightParen).end;
                self.node(
                    NodeTag::TS_PARENTHESIZED_TYPE,
                    Span::new(start, end),
                    &[annotation.value()],
                )
            }
            TokenKind::LeftBracket => self.parse_tuple_type(),
            TokenKind::LeftBrace => self.parse_type_literal(),
            TokenKind::Lt => self.parse_function_type(true),
            TokenKind::Infer => self.parse_infer_type(),
            kind if Self::is_type_reference_name(kind) => self.parse_type_reference(),
            _ => self.invalid_type(),
        }
    }

    fn parse_infer_type(&mut self) -> Result<ParsedNode, ParseError> {
        let start = self.expect(TokenKind::Infer).start;
        let type_parameter = self.parse_type_parameter()?;
        self.node(
            NodeTag::TS_INFER_TYPE,
            Span::new(start, type_parameter.span.end),
            &[type_parameter.value()],
        )
    }

    fn parse_type_reference(&mut self) -> Result<ParsedNode, ParseError> {
        let start = self.current.start;
        let mut type_name = self.parse_type_identifier()?;
        while self.eat(TokenKind::Dot).is_some() {
            let right = self.parse_type_identifier()?;
            type_name = self.node(
                NodeTag::TS_QUALIFIED_NAME,
                Span::new(type_name.span.start, right.span.end),
                &[type_name.value(), right.value()],
            )?;
        }
        let (type_arguments, end) = if self.current.kind == TokenKind::Lt {
            self.parse_type_arguments()?
        } else {
            (self.tape.push_null()?, type_name.span.end)
        };
        self.node(
            NodeTag::TS_TYPE_REFERENCE,
            Span::new(start, end),
            &[type_name.value(), type_arguments],
        )
    }

    fn parse_const_assertion_type(&mut self) -> Result<ParsedNode, ParseError> {
        let token = self.expect(TokenKind::Const);
        let name = self.tape.push_source_slice(Self::token_span(token))?;
        let identifier = self.node(NodeTag::IDENTIFIER, Self::token_span(token), &[name])?;
        let type_arguments = self.tape.push_null()?;
        self.node(
            NodeTag::TS_TYPE_REFERENCE,
            Self::token_span(token),
            &[identifier.value(), type_arguments],
        )
    }

    fn parse_type_arguments(&mut self) -> Result<(ValueRef, u32), ParseError> {
        let (arguments, end, _, _) = self.parse_type_arguments_impl(false)?;
        Ok((arguments, end))
    }

    fn parse_new_type_arguments(
        &mut self,
    ) -> Result<(ValueRef, u32, bool, Option<Token>), ParseError> {
        self.parse_type_arguments_impl(true)
    }

    fn parse_type_arguments_impl(
        &mut self,
        diagnose_empty: bool,
    ) -> Result<(ValueRef, u32, bool, Option<Token>), ParseError> {
        let start = self.expect(TokenKind::Lt).start;
        let mut arguments = Vec::new();
        if diagnose_empty && self.current_is_type_greater() {
            self.error(self.current_span(), "type argument list cannot be empty");
        }
        while !self.current_is_type_greater() && self.current.kind != TokenKind::Eof {
            arguments.push(self.parse_type()?.value());
            if self.eat(TokenKind::Comma).is_none() {
                break;
            }
        }
        let closed = self.current_is_type_greater();
        let compound_closer = matches!(
            self.current.kind,
            TokenKind::GtEq | TokenKind::ShiftRightEq | TokenKind::ShiftRightUnsignedEq
        )
        .then_some(self.current);
        let end = self.expect_type_greater();
        let arguments = self.tape.push_list(&arguments)?;
        let instantiation = self.node(
            NodeTag::TS_TYPE_PARAMETER_INSTANTIATION,
            Span::new(start, end),
            &[arguments],
        )?;
        Ok((instantiation.value(), end, closed, compound_closer))
    }

    fn parse_type_parameters(&mut self) -> Result<ValueRef, ParseError> {
        let Some(left_angle) = self.eat(TokenKind::Lt) else {
            return Ok(self.tape.push_null()?);
        };

        let mut parameters = Vec::new();
        if self.current_is_type_greater() {
            self.error(self.current_span(), "type parameter list cannot be empty");
        }
        while !self.current_is_type_greater() && self.current.kind != TokenKind::Eof {
            parameters.push(self.parse_type_parameter()?.value());
            if self.eat(TokenKind::Comma).is_none() {
                break;
            }
        }
        let end = self.expect_type_greater();
        let parameters = self.tape.push_list(&parameters)?;
        Ok(self
            .node(
                NodeTag::TS_TYPE_PARAMETER_DECLARATION,
                Span::new(left_angle.start, end),
                &[parameters],
            )?
            .value())
    }

    fn parse_type_parameter(&mut self) -> Result<ParsedNode, ParseError> {
        let start = self.current.start;
        let is_const = self.eat(TokenKind::Const).is_some();
        let is_in = self.eat(TokenKind::In).is_some();
        let is_out = self.eat(TokenKind::Out).is_some();
        let name_token = self.take();
        if !Self::is_identifier_name(name_token.kind) {
            self.error(
                Self::token_span(name_token),
                "expected a type parameter name",
            );
        }
        let name = self.identifier_from_span(Self::token_span(name_token))?;
        let constraint = if self.eat(TokenKind::Extends).is_some() {
            self.parse_type()?.value()
        } else {
            self.tape.push_null()?
        };
        let default = if self.eat(TokenKind::Eq).is_some() {
            self.parse_type()?.value()
        } else {
            self.tape.push_null()?
        };
        let end = self.previous_end(name_token.end);
        let is_const = self.tape.push_bool(is_const)?;
        let is_in = self.tape.push_bool(is_in)?;
        let is_out = self.tape.push_bool(is_out)?;
        self.node(
            NodeTag::TS_TYPE_PARAMETER,
            Span::new(start, end),
            &[name.value(), is_const, is_in, is_out, constraint, default],
        )
    }

    fn parse_function_type(&mut self, generic: bool) -> Result<ParsedNode, ParseError> {
        let start = self.current.start;
        let type_parameters = if generic {
            self.parse_type_parameters()?
        } else {
            self.tape.push_null()?
        };
        let (parameters, _) = self.parse_type_signature_parameters()?;
        self.expect(TokenKind::Arrow);
        let return_type = self.parse_type_annotation()?;
        self.node(
            NodeTag::TS_FUNCTION_TYPE,
            Span::new(start, return_type.span.end),
            &[type_parameters, parameters, return_type.value()],
        )
    }

    fn parse_type_signature_parameters(&mut self) -> Result<(ValueRef, u32), ParseError> {
        self.expect(TokenKind::LeftParen);
        let mut parameters = Vec::new();
        while !matches!(self.current.kind, TokenKind::RightParen | TokenKind::Eof) {
            let rest = self.eat(TokenKind::Ellipsis);
            let start = rest.map_or(self.current.start, |token| token.start);
            let name_token = self.take();
            if !Self::is_identifier_name(name_token.kind) {
                self.error(Self::token_span(name_token), "expected a parameter name");
            }
            let name = self.tape.push_source_slice(Self::token_span(name_token))?;
            let optional = self.eat(TokenKind::Question).is_some();
            let annotation = if self.eat(TokenKind::Colon).is_some() {
                self.parse_type_annotation()?
            } else {
                self.error(self.current_span(), "expected a type annotation");
                let invalid = self.invalid_type()?;
                self.node(
                    NodeTag::TS_TYPE_ANNOTATION,
                    invalid.span,
                    &[invalid.value()],
                )?
            };
            let optional = self.tape.push_bool(optional)?;
            let identifier = self.node(
                NodeTag::IDENTIFIER,
                Span::new(name_token.start, annotation.span.end),
                &[name, annotation.value(), optional],
            )?;
            let parameter = if rest.is_some() {
                self.node(
                    NodeTag::REST_ELEMENT,
                    Span::new(start, identifier.span.end),
                    &[identifier.value()],
                )?
            } else {
                identifier
            };
            parameters.push(parameter.value());
            if self.eat(TokenKind::Comma).is_none() {
                break;
            }
        }
        let end = self.expect(TokenKind::RightParen).end;
        Ok((self.tape.push_list(&parameters)?, end))
    }

    fn parse_tuple_type(&mut self) -> Result<ParsedNode, ParseError> {
        let start = self.take().start;
        let mut elements = Vec::new();
        while !matches!(self.current.kind, TokenKind::RightBracket | TokenKind::Eof) {
            let rest = self.eat(TokenKind::Ellipsis);
            let element_start = rest.map_or(self.current.start, |token| token.start);
            let mut element = if self.looks_like_named_tuple_member() {
                let label = self.parse_type_identifier()?;
                let optional = self.eat(TokenKind::Question).is_some();
                self.expect(TokenKind::Colon);
                let element_type = self.parse_type()?;
                let optional = self.tape.push_bool(optional)?;
                self.node(
                    NodeTag::TS_NAMED_TUPLE_MEMBER,
                    Span::new(label.span.start, element_type.span.end),
                    &[label.value(), element_type.value(), optional],
                )?
            } else {
                self.parse_type()?
            };
            if rest.is_some() {
                element = self.node(
                    NodeTag::REST_ELEMENT,
                    Span::new(element_start, element.span.end),
                    &[element.value()],
                )?;
            }
            elements.push(element.value());
            if self.eat(TokenKind::Comma).is_none() {
                break;
            }
        }
        let end = self.expect(TokenKind::RightBracket).end;
        let elements = self.tape.push_list(&elements)?;
        self.node(NodeTag::TS_TUPLE_TYPE, Span::new(start, end), &[elements])
    }

    fn parse_type_literal(&mut self) -> Result<ParsedNode, ParseError> {
        let start = self.expect(TokenKind::LeftBrace).start;
        if self.looks_like_mapped_type() {
            return self.parse_mapped_type(start);
        }
        let (members, end) = self.parse_type_members()?;
        self.node(NodeTag::TS_TYPE_LITERAL, Span::new(start, end), &[members])
    }

    fn parse_type_members(&mut self) -> Result<(ValueRef, u32), ParseError> {
        let mut members = Vec::new();
        while !matches!(self.current.kind, TokenKind::RightBrace | TokenKind::Eof) {
            if matches!(self.current.kind, TokenKind::Semicolon | TokenKind::Comma) {
                self.bump();
                continue;
            }
            members.push(self.parse_type_member()?.value());
            if matches!(self.current.kind, TokenKind::Semicolon | TokenKind::Comma) {
                self.bump();
            } else if self.current.kind != TokenKind::RightBrace {
                self.error(self.current_span(), "expected a type member separator");
            }
        }
        let end = self.expect(TokenKind::RightBrace).end;
        let members = self.tape.push_list(&members)?;
        Ok((members, end))
    }

    fn parse_interface_body(&mut self) -> Result<ParsedNode, ParseError> {
        let start = self.expect(TokenKind::LeftBrace).start;
        let (body, end) = self.parse_type_members()?;
        self.node(NodeTag::TS_INTERFACE_BODY, Span::new(start, end), &[body])
    }

    fn parse_mapped_type(&mut self, start: u32) -> Result<ParsedNode, ParseError> {
        let readonly = self.eat(TokenKind::Readonly);
        self.expect(TokenKind::LeftBracket);
        let name_token = self.take();
        if !Self::is_identifier_name(name_token.kind) {
            self.error(
                Self::token_span(name_token),
                "expected a mapped type parameter",
            );
        }
        let key = self.identifier_from_span(Self::token_span(name_token))?;
        self.expect(TokenKind::In);
        let constraint = self.parse_type()?;
        self.expect(TokenKind::RightBracket);
        let optional = self.eat(TokenKind::Question).is_some();
        self.expect(TokenKind::Colon);
        let annotation = self.parse_type()?;
        let end = self.expect(TokenKind::RightBrace).end;
        let name_type = self.tape.push_null()?;
        let readonly = if readonly.is_some() {
            self.tape.push_bool(true)?
        } else {
            self.tape.push_null()?
        };
        let optional = self.tape.push_bool(optional)?;
        self.node(
            NodeTag::TS_MAPPED_TYPE,
            Span::new(start, end),
            &[
                key.value(),
                constraint.value(),
                name_type,
                annotation.value(),
                readonly,
                optional,
            ],
        )
    }

    fn parse_type_member(&mut self) -> Result<ParsedNode, ParseError> {
        let start = self.current.start;
        let readonly = self.eat(TokenKind::Readonly).is_some();
        let key = self.parse_type_member_key()?;
        let optional = self.eat(TokenKind::Question).is_some();
        if matches!(self.current.kind, TokenKind::Lt | TokenKind::LeftParen) {
            let type_parameters = if self.current.kind == TokenKind::Lt {
                self.parse_type_parameters()?
            } else {
                self.tape.push_null()?
            };
            let (parameters, parameters_end) = self.parse_type_signature_parameters()?;
            let return_type = if self.eat(TokenKind::Colon).is_some() {
                self.parse_type_annotation()?.value()
            } else {
                self.tape.push_null()?
            };
            let end = self.previous_end(parameters_end);
            let computed = self.tape.push_bool(false)?;
            let optional = self.tape.push_bool(optional)?;
            return self.node(
                NodeTag::TS_METHOD_SIGNATURE,
                Span::new(start, end),
                &[
                    key.value(),
                    type_parameters,
                    parameters,
                    return_type,
                    computed,
                    optional,
                ],
            );
        }

        self.expect(TokenKind::Colon);
        let annotation = self.parse_type_annotation()?;
        let computed = self.tape.push_bool(false)?;
        let optional = self.tape.push_bool(optional)?;
        let readonly = self.tape.push_bool(readonly)?;
        self.node(
            NodeTag::TS_PROPERTY_SIGNATURE,
            Span::new(start, annotation.span.end),
            &[
                key.value(),
                annotation.value(),
                computed,
                optional,
                readonly,
            ],
        )
    }

    fn parse_type_member_key(&mut self) -> Result<ParsedNode, ParseError> {
        if matches!(self.current.kind, TokenKind::String | TokenKind::Number) {
            return self.parse_literal();
        }
        let token = self.take();
        if !Self::is_identifier_name(token.kind) && token.kind != TokenKind::New {
            self.error(Self::token_span(token), "expected a type member name");
        }
        let name = self.tape.push_source_slice(Self::token_span(token))?;
        self.node(NodeTag::IDENTIFIER, Self::token_span(token), &[name])
    }

    fn parse_type_alias_declaration(&mut self) -> Result<ParsedNode, ParseError> {
        let start = self.expect(TokenKind::Type).start;
        let id = self.parse_binding_identifier(BindingKind::Type)?;
        let type_parameters = self.parse_type_parameters()?;
        self.expect(TokenKind::Eq);
        let annotation = self.parse_type()?;
        let end = self.consume_semicolon();
        self.node(
            NodeTag::TS_TYPE_ALIAS_DECLARATION,
            Span::new(start, end),
            &[id.value(), type_parameters, annotation.value()],
        )
    }

    fn parse_interface_declaration(&mut self) -> Result<ParsedNode, ParseError> {
        let start = self.expect(TokenKind::Interface).start;
        let id = self.parse_binding_identifier(BindingKind::Type)?;
        let type_parameters = self.parse_type_parameters()?;
        let mut heritage = Vec::new();
        if self.eat(TokenKind::Extends).is_some() {
            loop {
                heritage.push(self.parse_interface_heritage()?.value());
                if self.eat(TokenKind::Comma).is_none() {
                    break;
                }
            }
        }
        let body = self.parse_interface_body()?;
        let heritage = self.tape.push_list(&heritage)?;
        self.node(
            NodeTag::TS_INTERFACE_DECLARATION,
            Span::new(start, body.span.end),
            &[id.value(), type_parameters, heritage, body.value()],
        )
    }

    fn parse_interface_heritage(&mut self) -> Result<ParsedNode, ParseError> {
        self.parse_heritage(NodeTag::TS_INTERFACE_HERITAGE, false)
    }

    fn parse_heritage(
        &mut self,
        tag: NodeTag,
        diagnose_empty_type_arguments: bool,
    ) -> Result<ParsedNode, ParseError> {
        let start = self.current.start;
        let mut expression = self.parse_type_identifier()?;
        while self.eat(TokenKind::Dot).is_some() {
            let right = self.parse_type_identifier()?;
            let computed = self.tape.push_bool(false)?;
            let optional = self.tape.push_bool(false)?;
            expression = self.node(
                NodeTag::MEMBER_EXPRESSION,
                Span::new(expression.span.start, right.span.end),
                &[expression.value(), right.value(), computed, optional],
            )?;
        }
        if self.current.kind == TokenKind::ShiftLeft {
            self.current.kind = TokenKind::Lt;
            self.current.end = self.current.start + 1;
            self.lexer.set_position(self.current.end as usize);
        }
        let (type_arguments, end) = if self.current.kind == TokenKind::Lt {
            let (arguments, end, _, _) =
                self.parse_type_arguments_impl(diagnose_empty_type_arguments)?;
            (arguments, end)
        } else {
            (self.tape.push_null()?, expression.span.end)
        };
        self.node(
            tag,
            Span::new(start, end),
            &[expression.value(), type_arguments],
        )
    }

    fn parse_enum_declaration(&mut self, is_const: bool) -> Result<ParsedNode, ParseError> {
        let start = if is_const {
            let start = self.expect(TokenKind::Const).start;
            self.expect(TokenKind::Enum);
            start
        } else {
            self.expect(TokenKind::Enum).start
        };
        let id = self.parse_binding_identifier(BindingKind::Type)?;
        let body_start = self.expect(TokenKind::LeftBrace).start;
        let mut members = Vec::new();
        while !matches!(self.current.kind, TokenKind::RightBrace | TokenKind::Eof) {
            let member_start = self.current.start;
            let member_id = if matches!(self.current.kind, TokenKind::String | TokenKind::Number) {
                self.parse_literal()?
            } else {
                self.parse_type_member_key()?
            };
            let initializer = if self.eat(TokenKind::Eq).is_some() {
                self.parse_assignment_expression(true)?.value()
            } else {
                self.tape.push_null()?
            };
            let member_end = self.previous_end(member_id.span.end);
            members.push(
                self.node(
                    NodeTag::TS_ENUM_MEMBER,
                    Span::new(member_start, member_end),
                    &[member_id.value(), initializer],
                )?
                .value(),
            );
            if self.eat(TokenKind::Comma).is_none() {
                break;
            }
        }
        let end = self.expect(TokenKind::RightBrace).end;
        let _ = self.eat(TokenKind::Semicolon);
        let members = self.tape.push_list(&members)?;
        let body = self.node(
            NodeTag::TS_ENUM_BODY,
            Span::new(body_start, end),
            &[members],
        )?;
        let is_const = self.tape.push_bool(is_const)?;
        let declare = self.tape.push_bool(false)?;
        self.node(
            NodeTag::TS_ENUM_DECLARATION,
            Span::new(start, end),
            &[id.value(), body.value(), is_const, declare],
        )
    }

    fn parse_module_declaration(&mut self) -> Result<ParsedNode, ParseError> {
        let keyword = self.take();
        let start = keyword.start;
        let mut id = if self.current.kind == TokenKind::String {
            self.parse_literal()?
        } else {
            self.parse_type_identifier()?
        };
        while self.eat(TokenKind::Dot).is_some() {
            let right = self.parse_type_identifier()?;
            id = self.node(
                NodeTag::TS_QUALIFIED_NAME,
                Span::new(id.span.start, right.span.end),
                &[id.value(), right.value()],
            )?;
        }
        let block_start = self.expect(TokenKind::LeftBrace).start;
        self.context.enter_scope(ScopeKind::Type);
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
        let module_body = self.node(
            NodeTag::TS_MODULE_BLOCK,
            Span::new(block_start, end),
            &[body],
        )?;
        let declare = self.tape.push_bool(false)?;
        let kind = self
            .tape
            .push_u32(u32::from(keyword.kind == TokenKind::Module))?;
        self.node(
            NodeTag::TS_MODULE_DECLARATION,
            Span::new(start, end),
            &[id.value(), module_body.value(), declare, kind],
        )
    }

    fn parse_type_identifier(&mut self) -> Result<ParsedNode, ParseError> {
        let token = self.take();
        if !Self::is_type_reference_name(token.kind) {
            self.error(Self::token_span(token), "expected a type name");
        }
        let name = self.tape.push_source_slice(Self::token_span(token))?;
        self.node(NodeTag::IDENTIFIER, Self::token_span(token), &[name])
    }

    fn invalid_type(&mut self) -> Result<ParsedNode, ParseError> {
        let span = self.current_span();
        self.error(span, "expected a type");
        if !matches!(
            self.current.kind,
            TokenKind::Eof
                | TokenKind::Semicolon
                | TokenKind::Comma
                | TokenKind::RightParen
                | TokenKind::RightBracket
                | TokenKind::RightBrace
                | TokenKind::Eq
                | TokenKind::Question
                | TokenKind::Colon
                | TokenKind::Pipe
                | TokenKind::Amp
                | TokenKind::Gt
                | TokenKind::ShiftRight
                | TokenKind::ShiftRightUnsigned
        ) {
            self.bump();
        }
        let name = self.tape.push_string("<invalid>")?;
        let identifier = self.node(NodeTag::IDENTIFIER, span, &[name])?;
        let parameters = self.tape.push_null()?;
        self.node(
            NodeTag::TS_TYPE_REFERENCE,
            span,
            &[identifier.value(), parameters],
        )
    }

    fn expect_type_greater(&mut self) -> u32 {
        if let Some(end) = self.eat_type_greater() {
            return end;
        }
        self.error(self.current_span(), "expected `>`");
        self.current.start
    }

    fn eat_type_greater(&mut self) -> Option<u32> {
        let start = self.current.start;
        match self.current.kind {
            TokenKind::Gt => Some(self.take().end),
            TokenKind::ShiftRight => {
                self.current.start = start + 1;
                self.current.kind = TokenKind::Gt;
                self.current.flags = TokenFlags::default();
                Some(start + 1)
            }
            TokenKind::ShiftRightUnsigned => {
                self.current.start = start + 1;
                self.current.kind = TokenKind::ShiftRight;
                self.current.flags = TokenFlags::default();
                Some(start + 1)
            }
            TokenKind::GtEq => {
                self.current.start = start + 1;
                self.current.kind = TokenKind::Eq;
                self.current.flags = TokenFlags::default();
                Some(start + 1)
            }
            TokenKind::ShiftRightEq => {
                self.current.start = start + 1;
                self.current.kind = TokenKind::GtEq;
                self.current.flags = TokenFlags::default();
                Some(start + 1)
            }
            TokenKind::ShiftRightUnsignedEq => {
                self.current.start = start + 1;
                self.current.kind = TokenKind::ShiftRightEq;
                self.current.flags = TokenFlags::default();
                Some(start + 1)
            }
            _ => None,
        }
    }

    const fn current_is_type_greater(&self) -> bool {
        matches!(
            self.current.kind,
            TokenKind::Gt
                | TokenKind::GtEq
                | TokenKind::ShiftRight
                | TokenKind::ShiftRightEq
                | TokenKind::ShiftRightUnsigned
                | TokenKind::ShiftRightUnsignedEq
        )
    }

    fn looks_like_function_type(&self) -> bool {
        let Some(rest) = self.source.get(self.current.start as usize..) else {
            return false;
        };
        let bytes = rest.as_bytes();
        let mut depth = 0_u32;
        let mut quote = None;
        let mut escaped = false;
        for (index, &byte) in bytes.iter().enumerate() {
            if let Some(delimiter) = quote {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == delimiter {
                    quote = None;
                }
                continue;
            }
            if matches!(byte, b'\'' | b'"' | b'`') {
                quote = Some(byte);
                continue;
            }
            match byte {
                b'(' => depth += 1,
                b')' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return rest
                            .get(index + 1..)
                            .is_some_and(|tail| tail.trim_start().starts_with("=>"));
                    }
                }
                _ => {}
            }
        }
        false
    }

    fn looks_like_named_tuple_member(&self) -> bool {
        if !Self::is_identifier_name(self.current.kind) {
            return false;
        }
        self.source
            .get(self.current.end as usize..)
            .is_some_and(|rest| matches!(rest.trim_start().as_bytes().first(), Some(b':' | b'?')))
    }

    fn looks_like_mapped_type(&self) -> bool {
        self.current.kind == TokenKind::LeftBracket
            || self.current.kind == TokenKind::Readonly
                && self
                    .source
                    .get(self.current.end as usize..)
                    .is_some_and(|rest| rest.trim_start().starts_with('['))
    }

    fn current_type_keyword_tag(&self) -> Option<NodeTag> {
        let tag = match self.current.kind {
            TokenKind::Any => NodeTag::TS_ANY_KEYWORD,
            TokenKind::Boolean => NodeTag::TS_BOOLEAN_KEYWORD,
            TokenKind::Never => NodeTag::TS_NEVER_KEYWORD,
            TokenKind::Null => NodeTag::TS_NULL_KEYWORD,
            TokenKind::NumberKeyword => NodeTag::TS_NUMBER_KEYWORD,
            TokenKind::Object => NodeTag::TS_OBJECT_KEYWORD,
            TokenKind::StringKeyword => NodeTag::TS_STRING_KEYWORD,
            TokenKind::Symbol => NodeTag::TS_SYMBOL_KEYWORD,
            TokenKind::This => NodeTag::TS_THIS_TYPE,
            TokenKind::Undefined => NodeTag::TS_UNDEFINED_KEYWORD,
            TokenKind::Unknown => NodeTag::TS_UNKNOWN_KEYWORD,
            TokenKind::Void => NodeTag::TS_VOID_KEYWORD,
            TokenKind::Identifier => match self
                .source
                .get(self.current.start as usize..self.current.end as usize)
            {
                Some("bigint") => NodeTag::TS_BIGINT_KEYWORD,
                Some("intrinsic") => NodeTag::TS_INTRINSIC_KEYWORD,
                _ => return None,
            },
            _ => return None,
        };
        Some(tag)
    }

    const fn is_type_reference_name(kind: TokenKind) -> bool {
        Self::is_identifier_name(kind) || matches!(kind, TokenKind::This | TokenKind::Void)
    }

    fn invalid_expression(&mut self) -> Result<ParsedNode, ParseError> {
        let token = self.take();
        self.error(Self::token_span(token), "expected an expression");
        let name = self.tape.push_string("<invalid>")?;
        self.node(NodeTag::IDENTIFIER, Self::token_span(token), &[name])
    }

    fn node(
        &mut self,
        tag: NodeTag,
        span: Span,
        fields: &[ValueRef],
    ) -> Result<ParsedNode, ParseError> {
        let node = self.tape.push_node(tag, span, 0, fields)?;
        let assignment_target = match tag {
            NodeTag::IDENTIFIER | NodeTag::MEMBER_EXPRESSION => AssignmentTargetType::Simple,
            NodeTag::CALL_EXPRESSION => AssignmentTargetType::WebCompat,
            _ => AssignmentTargetType::Invalid,
        };
        self.last_node_tag = Some(tag);
        self.last_assignment_target = assignment_target;
        Ok(ParsedNode { node, span })
    }

    fn validate_assignment_target(
        &mut self,
        span: Span,
        target: AssignmentTargetType,
        policy: AssignmentTargetPolicy,
    ) {
        if !self.reports_ecmascript_early_errors() {
            return;
        }
        let valid = match target {
            AssignmentTargetType::Simple => true,
            AssignmentTargetType::WebCompat => {
                !self.context.grammar().strict() && policy.allows_web_compat()
            }
            AssignmentTargetType::OptionalChain => {
                self.options.syntax_extensions.optional_chaining_assign
                    && policy.allows_optional_chain()
            }
            AssignmentTargetType::RestrictedIdentifier | AssignmentTargetType::Invalid => false,
        };
        if !valid {
            self.error(span, policy.diagnostic());
        }
    }

    const fn assignment_pattern_checkpoint(&self) -> AssignmentPatternCheckpoint {
        AssignmentPatternCheckpoint {
            candidate_len: self.assignment_pattern_candidates.len(),
        }
    }

    fn rollback_assignment_patterns(&mut self, checkpoint: AssignmentPatternCheckpoint) {
        self.assignment_pattern_candidates
            .truncate(checkpoint.candidate_len);
    }

    fn retain_root_assignment_pattern(
        &mut self,
        checkpoint: AssignmentPatternCheckpoint,
        root: NodeRef,
    ) {
        if self
            .assignment_pattern_candidates
            .last()
            .is_none_or(|candidate| candidate.node.offset() != root.offset())
        {
            self.rollback_assignment_patterns(checkpoint);
        }
    }

    fn register_assignment_pattern(
        &mut self,
        checkpoint: AssignmentPatternCheckpoint,
        root: NodeRef,
        tag: NodeTag,
    ) {
        self.assignment_pattern_candidates
            .push(AssignmentPatternCandidate {
                node: root,
                tag,
                group_start: checkpoint.candidate_len,
                error: None,
            });
    }

    fn retag_assignment_pattern(&mut self, root: NodeRef) -> Result<bool, ParseError> {
        let Some(group) = self.assignment_pattern_candidates.last().copied() else {
            return Ok(false);
        };
        if group.node.offset() != root.offset() {
            return Ok(false);
        }
        if let Some(error) = self.assignment_pattern_candidates[group.group_start..]
            .iter()
            .find_map(|candidate| candidate.error)
        {
            match error {
                AssignmentPatternError::Accessor(span) => self.error(
                    span,
                    "accessor properties are not allowed in assignment patterns",
                ),
                AssignmentPatternError::InvalidTarget(span) => {
                    self.error(span, "invalid assignment pattern target");
                }
            }
        }
        for candidate in &self.assignment_pattern_candidates[group.group_start..] {
            if candidate.error.is_none() {
                self.tape.retag_node(candidate.node, candidate.tag)?;
            }
        }
        self.assignment_pattern_candidates
            .truncate(group.group_start);
        Ok(true)
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

    const fn current_span(&self) -> Span {
        Self::token_span(self.current)
    }

    const fn token_span(token: Token) -> Span {
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

    fn followed_by_token_without_line_break(&self, kind: TokenKind) -> bool {
        if self.current.flags.escaped() {
            return false;
        }
        // A separate lexer keeps trivia-sensitive lookahead from mutating live diagnostics.
        let mut lookahead = Lexer::new(self.source);
        lookahead.set_position(self.current.end as usize);
        let token = lookahead.next_token();
        token.kind == kind && !token.flags.line_break_before() && !token.flags.escaped()
    }

    fn import_starts_expression(&self) -> bool {
        let mut lookahead = Lexer::new(self.source);
        lookahead.set_position(self.current.end as usize);
        matches!(
            lookahead.next_token().kind,
            TokenKind::LeftParen | TokenKind::Dot
        )
    }

    fn import_starts_direct_call(&self) -> bool {
        let mut lookahead = Lexer::new(self.source);
        lookahead.set_position(self.current.end as usize);
        let punctuation = lookahead.next_token();
        if punctuation.kind == TokenKind::LeftParen {
            return true;
        }
        if punctuation.kind != TokenKind::Dot {
            return false;
        }
        let property = lookahead.next_token();
        if property.flags.escaped()
            || !matches!(
                self.source_text(Self::token_span(property)),
                "source" | "defer"
            )
        {
            return false;
        }
        lookahead.next_token().kind == TokenKind::LeftParen
    }

    fn implements_is_followed_by_class_body(&self) -> bool {
        let mut lookahead = Lexer::new(self.source);
        lookahead.set_position(self.current.end as usize);
        lookahead.next_token().kind == TokenKind::LeftBrace
    }

    fn left_brace_is_followed_by_right_brace(&self) -> bool {
        let mut lookahead = Lexer::new(self.source);
        lookahead.set_position(self.current.end as usize);
        lookahead.next_token().kind == TokenKind::RightBrace
    }

    const fn can_follow_new_type_arguments(&self) -> bool {
        match self.current.kind {
            TokenKind::LeftParen => true,
            TokenKind::NoSubstitutionTemplate
            | TokenKind::TemplateHead
            | TokenKind::Lt
            | TokenKind::Gt
            | TokenKind::Plus
            | TokenKind::Minus => false,
            kind => {
                self.current.flags.line_break_before()
                    || binary_binding(kind, true).is_some()
                    || matches!(kind, TokenKind::As | TokenKind::Satisfies)
                    || !Self::is_expression_start(kind)
            }
        }
    }

    fn import_equals_type_only(&self, first: Token) -> Option<bool> {
        if !self.options.language.is_typescript() || !Self::is_identifier_name(first.kind) {
            return None;
        }
        // `type` remains an alias name when immediately followed by `=`.
        let mut lookahead = Lexer::new(self.source);
        lookahead.set_position(first.end as usize);
        let next = lookahead.next_token();
        if next.kind == TokenKind::Eq {
            return Some(false);
        }
        if first.kind == TokenKind::Type
            && !first.flags.escaped()
            && Self::is_identifier_name(next.kind)
            && lookahead.next_token().kind == TokenKind::Eq
        {
            return Some(true);
        }
        None
    }

    fn looks_like_export_import_equals(&self) -> bool {
        let mut lookahead = Lexer::new(self.source);
        lookahead.set_position(self.current.end as usize);
        self.import_equals_type_only(lookahead.next_token())
            .is_some()
    }

    fn current_accessor_kind(&self, allow_private: bool) -> Option<AccessorKind> {
        if self.current.flags.escaped() {
            return None;
        }
        let accessor = match self.current.kind {
            TokenKind::Get => AccessorKind::Get,
            TokenKind::Set => AccessorKind::Set,
            _ => return None,
        };
        let mut lookahead = Lexer::new(self.source);
        lookahead.set_position(self.current.end as usize);
        let next = lookahead.next_token().kind;
        (allow_private && next == TokenKind::PrivateIdentifier
            || matches!(
                next,
                TokenKind::LeftBracket | TokenKind::String | TokenKind::Number | TokenKind::BigInt
            )
            || Self::is_member_identifier_name(next))
        .then_some(accessor)
    }

    fn current_accessibility_modifier(&self) -> Option<AccessibilityModifier> {
        if self.current_typescript_modifier_matches(TokenKind::Public, "public") {
            Some(AccessibilityModifier::Public)
        } else if self.current_typescript_modifier_matches(TokenKind::Protected, "protected") {
            Some(AccessibilityModifier::Protected)
        } else if self.current_typescript_modifier_matches(TokenKind::Private, "private") {
            Some(AccessibilityModifier::Private)
        } else {
            None
        }
    }

    fn current_typescript_modifier_matches(&self, kind: TokenKind, name: &str) -> bool {
        self.current.kind == kind
            || self.current.kind == TokenKind::Identifier
                && self.current.flags.escaped()
                && self.identifier_name_matches(self.current_span(), name, true)
    }

    fn typescript_modifier_has_class_member_follower(&self, allow_line_break: bool) -> bool {
        let mut lookahead = Lexer::new(self.source);
        lookahead.set_position(self.current.end as usize);
        let follower = lookahead.next_token();
        (allow_line_break || !follower.flags.line_break_before())
            && (matches!(
                follower.kind,
                TokenKind::LeftBrace | TokenKind::Star | TokenKind::Ellipsis
            ) || Self::is_property_name_start(follower.kind, true))
    }

    fn diagnose_typescript_modifier_order(
        &mut self,
        rank: u8,
        last_rank: &mut Option<u8>,
        duplicate: bool,
        span: Span,
    ) {
        if self.options.semantic_errors {
            if duplicate {
                self.error(span, "duplicate TypeScript class member modifier");
            }
            if last_rank.is_some_and(|previous| previous > rank) {
                self.error(span, "TypeScript class member modifiers are out of order");
            }
        }
        *last_rank = Some(last_rank.map_or(rank, |previous| previous.max(rank)));
    }

    fn diagnose_typescript_class_member_modifiers(
        &mut self,
        modifiers: TypeScriptModifiers,
        key_span: Span,
        method: bool,
        constructor: bool,
        class_has_super: bool,
    ) {
        if !self.options.semantic_errors {
            return;
        }
        if modifiers.readonly && method {
            self.error(key_span, "class methods cannot have the readonly modifier");
        }
        if modifiers.r#override && constructor {
            self.error(
                key_span,
                "class constructors cannot have the override modifier",
            );
        }
        if modifiers.r#override && !class_has_super {
            self.error(
                key_span,
                "override requires the containing class to extend another class",
            );
        }
        if modifiers.accessibility.is_some() && self.source_text(key_span).starts_with('#') {
            self.error(
                key_span,
                "private class elements cannot have an accessibility modifier",
            );
        }
    }

    fn static_property_name_matches(&self, span: Span, expected: &str) -> bool {
        let raw = self.source_text(span);
        if raw == expected {
            return true;
        }
        if !raw.starts_with(['\'', '"']) && !raw.contains('\\') {
            return false;
        }
        decode_static_property_name(raw).is_some_and(|name| name == expected)
    }

    fn identifier_name_matches(&self, span: Span, expected: &str, escaped: bool) -> bool {
        let raw = self.source_text(span);
        if !escaped {
            return raw == expected;
        }
        decode_static_property_name(raw).is_some_and(|name| name == expected)
    }

    fn is_escaped_reserved_identifier(&self, span: Span, escaped: bool) -> bool {
        // Escaped keywords stay lexer identifiers so legal IdentifierName positions can accept them.
        escaped
            && decode_static_property_name(self.source_text(span)).is_some_and(|name| {
                matches!(
                    name.as_str(),
                    "break"
                        | "case"
                        | "catch"
                        | "class"
                        | "const"
                        | "continue"
                        | "debugger"
                        | "default"
                        | "delete"
                        | "do"
                        | "else"
                        | "enum"
                        | "export"
                        | "extends"
                        | "false"
                        | "finally"
                        | "for"
                        | "function"
                        | "if"
                        | "import"
                        | "in"
                        | "instanceof"
                        | "new"
                        | "null"
                        | "return"
                        | "super"
                        | "switch"
                        | "this"
                        | "throw"
                        | "true"
                        | "try"
                        | "typeof"
                        | "var"
                        | "void"
                        | "while"
                        | "with"
                )
            })
    }

    fn is_strict_reserved_identifier(&self, span: Span, escaped: bool) -> bool {
        let raw = self.source_text(span);
        if !escaped {
            return matches!(
                raw,
                "implements"
                    | "interface"
                    | "let"
                    | "package"
                    | "private"
                    | "protected"
                    | "public"
                    | "static"
                    | "yield"
            );
        }
        decode_static_property_name(raw).is_some_and(|name| {
            matches!(
                name.as_str(),
                "implements"
                    | "interface"
                    | "let"
                    | "package"
                    | "private"
                    | "protected"
                    | "public"
                    | "static"
                    | "yield"
            )
        })
    }

    fn is_private_member_target(&self, expression: ParsedNode) -> bool {
        if self.last_node_tag != Some(NodeTag::MEMBER_EXPRESSION) {
            return false;
        }
        let mut lexer = Lexer::new(self.source);
        lexer.set_position(expression.span.start as usize);
        let mut last = lexer.next_token();
        let mut last_semantic = last;
        while last.end < expression.span.end && last.kind != TokenKind::Eof {
            last = lexer.next_token();
            if last.kind != TokenKind::RightParen {
                last_semantic = last;
            }
        }
        last_semantic.kind == TokenKind::PrivateIdentifier
    }

    const fn reports_private_early_errors(&self) -> bool {
        !self.options.language.is_typescript() || self.options.semantic_errors
    }

    const fn reports_ecmascript_early_errors(&self) -> bool {
        self.options.semantic_errors
    }

    fn source_text(&self, span: Span) -> &'s str {
        self.source
            .get(span.start as usize..span.end as usize)
            .unwrap_or_default()
    }

    fn looks_like_async_arrow(&self) -> bool {
        let mut lookahead = Lexer::new(self.source);
        lookahead.set_position(self.current.end as usize);
        let first = lookahead.next_token();
        if first.flags.line_break_before() {
            return false;
        }
        if first.kind != TokenKind::LeftParen {
            return Self::is_identifier_name(first.kind)
                && matches!(lookahead.next_token(), token if token.kind == TokenKind::Arrow && !token.flags.line_break_before());
        }

        let mut depth = 1_u32;
        loop {
            let token = lookahead.next_token();
            match token.kind {
                TokenKind::LeftParen => depth = depth.saturating_add(1),
                TokenKind::RightParen => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        let arrow = lookahead.next_token();
                        return arrow.kind == TokenKind::Arrow && !arrow.flags.line_break_before();
                    }
                }
                TokenKind::Eof => return false,
                _ => {}
            }
        }
    }

    fn looks_like_empty_arrow(&self) -> bool {
        let mut lookahead = Lexer::new(self.source);
        lookahead.set_position(self.current.end as usize);
        if lookahead.next_token().kind != TokenKind::RightParen {
            return false;
        }
        // Only trivia after `)` participates in the arrow's no-LineTerminator restriction.
        let arrow = lookahead.next_token();
        arrow.kind == TokenKind::Arrow && !arrow.flags.line_break_before()
    }

    fn looks_like_parenthesized_arrow(&self) -> bool {
        let mut lookahead = Lexer::new(self.source);
        lookahead.set_position(self.current.end as usize);
        let mut depth = 1_u32;
        loop {
            let token = lookahead.next_token();
            match token.kind {
                TokenKind::LeftParen => depth = depth.saturating_add(1),
                TokenKind::RightParen => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        let arrow = lookahead.next_token();
                        return arrow.kind == TokenKind::Arrow && !arrow.flags.line_break_before();
                    }
                }
                TokenKind::Eof => return false,
                _ => {}
            }
        }
    }

    fn starts_parenthesized_rest_parameter(&self) -> bool {
        let mut lookahead = Lexer::new(self.source);
        lookahead.set_position(self.current.end as usize);
        loop {
            let parameter = lookahead.next_token();
            if parameter.kind == TokenKind::Ellipsis {
                return true;
            }
            if !Self::is_identifier_name(parameter.kind)
                || lookahead.next_token().kind != TokenKind::Comma
            {
                return false;
            }
        }
    }

    const fn is_property_name_start(kind: TokenKind, allow_private: bool) -> bool {
        allow_private && matches!(kind, TokenKind::PrivateIdentifier)
            || matches!(
                kind,
                TokenKind::LeftBracket | TokenKind::String | TokenKind::Number | TokenKind::BigInt
            )
            || Self::is_member_identifier_name(kind)
    }

    const fn is_expression_start(kind: TokenKind) -> bool {
        Self::is_identifier_name(kind)
            || matches!(
                kind,
                TokenKind::PrivateIdentifier
                    | TokenKind::Number
                    | TokenKind::BigInt
                    | TokenKind::String
                    | TokenKind::RegExp
                    | TokenKind::NoSubstitutionTemplate
                    | TokenKind::TemplateHead
                    | TokenKind::True
                    | TokenKind::False
                    | TokenKind::Null
                    | TokenKind::This
                    | TokenKind::Super
                    | TokenKind::Function
                    | TokenKind::Class
                    | TokenKind::LeftParen
                    | TokenKind::LeftBracket
                    | TokenKind::LeftBrace
                    | TokenKind::New
                    | TokenKind::Import
                    | TokenKind::Slash
                    | TokenKind::SlashEq
                    | TokenKind::Plus
                    | TokenKind::Minus
                    | TokenKind::Bang
                    | TokenKind::Tilde
                    | TokenKind::Delete
                    | TokenKind::Typeof
                    | TokenKind::Void
                    | TokenKind::PlusPlus
                    | TokenKind::MinusMinus
                    | TokenKind::Lt
                    | TokenKind::At
            )
    }

    const fn is_identifier_name(kind: TokenKind) -> bool {
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

    const fn is_member_identifier_name(kind: TokenKind) -> bool {
        Self::is_identifier_name(kind)
            || matches!(
                kind,
                TokenKind::Break
                    | TokenKind::Case
                    | TokenKind::Catch
                    | TokenKind::Class
                    | TokenKind::Const
                    | TokenKind::Continue
                    | TokenKind::Debugger
                    | TokenKind::Default
                    | TokenKind::Delete
                    | TokenKind::Do
                    | TokenKind::Else
                    | TokenKind::Export
                    | TokenKind::Extends
                    | TokenKind::False
                    | TokenKind::Finally
                    | TokenKind::For
                    | TokenKind::Function
                    | TokenKind::If
                    | TokenKind::Import
                    | TokenKind::In
                    | TokenKind::Instanceof
                    | TokenKind::New
                    | TokenKind::Null
                    | TokenKind::Return
                    | TokenKind::Super
                    | TokenKind::Switch
                    | TokenKind::This
                    | TokenKind::Throw
                    | TokenKind::True
                    | TokenKind::Try
                    | TokenKind::Typeof
                    | TokenKind::Var
                    | TokenKind::Void
                    | TokenKind::While
                    | TokenKind::With
                    | TokenKind::Yield
            )
    }
}

fn has_use_strict_directive(source: &str, position: usize) -> bool {
    // Most function bodies do not start with a directive; avoid a second lexer on that hot path.
    if source
        .get(position..)
        .and_then(|rest| rest.bytes().find(|byte| !byte.is_ascii_whitespace()))
        .is_none_or(|byte| byte.is_ascii() && !matches!(byte, b'\'' | b'"' | b'#' | b'/'))
    {
        return false;
    }
    let mut lexer = Lexer::new(source);
    lexer.set_position(position);
    let mut directive = lexer.next_token();
    while directive.kind == TokenKind::String {
        let raw = source
            .get(directive.start as usize..directive.end as usize)
            .unwrap_or_default();
        let is_use_strict = matches!(raw, "\"use strict\"" | "'use strict'");
        let next = lexer.next_token();
        if next.kind == TokenKind::Semicolon {
            if is_use_strict {
                return true;
            }
            directive = lexer.next_token();
            continue;
        }
        if is_use_strict {
            return matches!(next.kind, TokenKind::RightBrace | TokenKind::Eof)
                || next.flags.line_break_before();
        }
        if next.flags.line_break_before() {
            directive = next;
            continue;
        }
        return false;
    }
    false
}

fn decode_static_property_name(raw: &str) -> Option<String> {
    let string_literal = raw.starts_with(['\'', '"']);
    let content =
        if string_literal && raw.len() >= 2 && raw.as_bytes().last() == raw.as_bytes().first() {
            &raw[1..raw.len() - 1]
        } else {
            raw
        };
    let mut input = content.chars().peekable();
    let mut decoded = String::with_capacity(content.len());
    while let Some(character) = input.next() {
        if character != '\\' {
            decoded.push(character);
            continue;
        }
        let escape = input.next()?;
        match escape {
            'u' => decoded.push(decode_unicode_escape(&mut input)?),
            'x' if string_literal => decoded.push(decode_fixed_hex_escape(&mut input, 2)?),
            '\n' if string_literal => {}
            '\r' if string_literal => {
                let _ = input.next_if_eq(&'\n');
            }
            'b' if string_literal => decoded.push('\u{0008}'),
            'f' if string_literal => decoded.push('\u{000c}'),
            'n' if string_literal => decoded.push('\n'),
            'r' if string_literal => decoded.push('\r'),
            't' if string_literal => decoded.push('\t'),
            'v' if string_literal => decoded.push('\u{000b}'),
            '0' if string_literal => decoded.push('\0'),
            escaped if string_literal => decoded.push(escaped),
            _ => return None,
        }
    }
    Some(decoded)
}

fn decode_unicode_escape(input: &mut Peekable<Chars<'_>>) -> Option<char> {
    let value = if input.next_if_eq(&'{').is_some() {
        let mut value = 0_u32;
        let mut digits = 0_u8;
        loop {
            let character = input.next()?;
            if character == '}' {
                break;
            }
            value = value
                .checked_mul(16)?
                .checked_add(character.to_digit(16)?)?;
            digits = digits.checked_add(1)?;
            if digits > 6 {
                return None;
            }
        }
        (digits > 0).then_some(value)?
    } else {
        decode_fixed_hex_value(input, 4)?
    };
    char::from_u32(value)
}

fn decode_fixed_hex_escape(input: &mut Peekable<Chars<'_>>, digits: u8) -> Option<char> {
    char::from_u32(decode_fixed_hex_value(input, digits)?)
}

fn decode_fixed_hex_value(input: &mut Peekable<Chars<'_>>, digits: u8) -> Option<u32> {
    let mut value = 0_u32;
    for _ in 0..digits {
        value = value
            .checked_mul(16)?
            .checked_add(input.next()?.to_digit(16)?)?;
    }
    Some(value)
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
