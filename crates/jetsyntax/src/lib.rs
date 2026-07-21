//! Experimental, independently implemented JavaScript, TypeScript, JSX, and TSX parser.
//!
//! `JetSyntax` emits a compact postfix tape while parsing. The native Rust API owns that tape, and
//! language bindings decode the same stable wire format without depending on Rust struct layout.

pub mod lexer;
pub mod parser;
pub mod tape;

mod operator;

pub use parser::{Diagnostic, ParseError, ParseResult, Severity, parse};

/// Syntax accepted by the parser.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Language {
    #[default]
    JavaScript,
    JavaScriptJsx,
    TypeScript,
    TypeScriptJsx,
    TypeScriptDefinition,
}

impl Language {
    #[must_use]
    pub const fn is_typescript(self) -> bool {
        matches!(
            self,
            Self::TypeScript | Self::TypeScriptJsx | Self::TypeScriptDefinition
        )
    }

    #[must_use]
    pub const fn is_jsx(self) -> bool {
        matches!(self, Self::JavaScriptJsx | Self::TypeScriptJsx)
    }
}

/// Top-level parsing semantics.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SourceKind {
    Script,
    #[default]
    Module,
    Unambiguous,
    CommonJs,
}

/// Native parser controls.
#[derive(Clone, Copy, Debug)]
pub struct ParseOptions {
    pub language: Language,
    pub source_kind: SourceKind,
    pub preserve_parentheses: bool,
    pub allow_return_outside_function: bool,
    pub semantic_errors: bool,
    pub syntax_extensions: SyntaxExtensions,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SyntaxExtensions {
    pub typescript_js_compatibility: bool,
    pub optional_chaining_assign: bool,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            language: Language::JavaScript,
            source_kind: SourceKind::Module,
            preserve_parentheses: true,
            allow_return_outside_function: false,
            semantic_errors: false,
            syntax_extensions: SyntaxExtensions::default(),
        }
    }
}
