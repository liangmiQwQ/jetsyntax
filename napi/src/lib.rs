use jetsyntax::{Language, ParseOptions, SourceKind, SyntaxExtensions, parse};
use napi::bindgen_prelude::Uint32Array;
use napi_derive::napi;

#[napi(object)]
#[derive(Default)]
pub struct BindingOptions {
    /// One of `js`, `jsx`, `ts`, `tsx`, or `dts`.
    pub lang: Option<String>,
    /// One of `script`, `module`, `unambiguous`, or `commonjs`.
    pub source_type: Option<String>,
    pub preserve_parens: Option<bool>,
    pub allow_return_outside_function: Option<bool>,
    pub range: Option<bool>,
    pub semantic_errors: Option<bool>,
    pub typescript_js_compatibility: Option<bool>,
    pub optional_chaining_assign: Option<bool>,
}

#[napi(object)]
pub struct TapeTransferResult {
    /// Stable `JetSyntax` postfix tape words. The JavaScript wrapper decodes this into `ESTree`.
    pub tape: Uint32Array,
    pub diagnostics: Vec<String>,
}

/// Parses source and transfers `JetSyntax`'s stable postfix tape.
///
/// # Errors
///
/// Returns an error for unsupported options or a native parser/tape failure.
#[napi(js_name = "parseToTape")]
#[allow(clippy::needless_pass_by_value)]
pub fn parse_to_tape(
    source: String,
    options: Option<BindingOptions>,
) -> napi::Result<TapeTransferResult> {
    let options = options.unwrap_or_default();
    let parse_options = ParseOptions {
        language: parse_language(options.lang.as_deref())?,
        source_kind: parse_source_kind(options.source_type.as_deref())?,
        preserve_parentheses: options.preserve_parens.unwrap_or(true),
        allow_return_outside_function: options.allow_return_outside_function.unwrap_or(false),
        semantic_errors: options.semantic_errors.unwrap_or(false),
        syntax_extensions: SyntaxExtensions {
            typescript_js_compatibility: options.typescript_js_compatibility.unwrap_or(false),
            optional_chaining_assign: options.optional_chaining_assign.unwrap_or(false),
        },
    };
    let result = parse(&source, parse_options).map_err(|error| {
        napi::Error::from_reason(format!("JetSyntax failed to parse source: {error}"))
    })?;

    Ok(TapeTransferResult {
        tape: result.tape.into_words().into(),
        diagnostics: result
            .diagnostics
            .into_iter()
            .map(|diagnostic| diagnostic.message)
            .collect(),
    })
}

fn parse_language(value: Option<&str>) -> napi::Result<Language> {
    match value.unwrap_or("js") {
        "js" => Ok(Language::JavaScript),
        "jsx" => Ok(Language::JavaScriptJsx),
        "ts" => Ok(Language::TypeScript),
        "tsx" => Ok(Language::TypeScriptJsx),
        "dts" => Ok(Language::TypeScriptDefinition),
        value => Err(napi::Error::from_reason(format!(
            "unsupported language `{value}`; expected js, jsx, ts, tsx, or dts"
        ))),
    }
}

fn parse_source_kind(value: Option<&str>) -> napi::Result<SourceKind> {
    match value.unwrap_or("module") {
        "script" => Ok(SourceKind::Script),
        "module" => Ok(SourceKind::Module),
        "unambiguous" => Ok(SourceKind::Unambiguous),
        "commonjs" => Ok(SourceKind::CommonJs),
        value => Err(napi::Error::from_reason(format!(
            "unsupported sourceType `{value}`; expected script, module, unambiguous, or commonjs"
        ))),
    }
}
