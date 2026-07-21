use jetsyntax::{
    Language, ParseOptions, ParseResult, SourceKind, parse,
    tape::{FrozenTape, NodeTag, TapeValue},
};

#[derive(Clone, Copy)]
struct GrammarCase {
    name: &'static str,
    source: &'static str,
    language: Language,
    source_kind: SourceKind,
    expected_tags: &'static [NodeTag],
}

impl GrammarCase {
    const fn script(
        name: &'static str,
        source: &'static str,
        expected_tags: &'static [NodeTag],
    ) -> Self {
        Self {
            name,
            source,
            language: Language::JavaScript,
            source_kind: SourceKind::Script,
            expected_tags,
        }
    }

    const fn module(
        name: &'static str,
        source: &'static str,
        expected_tags: &'static [NodeTag],
    ) -> Self {
        Self {
            name,
            source,
            language: Language::JavaScript,
            source_kind: SourceKind::Module,
            expected_tags,
        }
    }

    fn options(self, semantic_errors: bool) -> ParseOptions {
        ParseOptions {
            language: self.language,
            source_kind: self.source_kind,
            semantic_errors,
            ..ParseOptions::default()
        }
    }
}

#[derive(Clone, Copy)]
struct ExpressionShape {
    name: &'static str,
    source: &'static str,
    root: NodeTag,
    nested_field: usize,
    nested: NodeTag,
}

/// Operator binding must preserve both precedence and left/right associativity in the emitted tree.
#[test]
fn parser_should_preserve_operator_precedence_and_associativity() {
    let cases = [
        ExpressionShape {
            name: "multiplication before addition",
            source: "a + b * c;",
            root: NodeTag::BINARY_EXPRESSION,
            nested_field: 2,
            nested: NodeTag::BINARY_EXPRESSION,
        },
        ExpressionShape {
            name: "subtraction is left associative",
            source: "a - b - c;",
            root: NodeTag::BINARY_EXPRESSION,
            nested_field: 1,
            nested: NodeTag::BINARY_EXPRESSION,
        },
        ExpressionShape {
            name: "exponentiation is right associative",
            source: "a ** b ** c;",
            root: NodeTag::BINARY_EXPRESSION,
            nested_field: 2,
            nested: NodeTag::BINARY_EXPRESSION,
        },
        ExpressionShape {
            name: "assignment is right associative",
            source: "a = b = c;",
            root: NodeTag::ASSIGNMENT_EXPRESSION,
            nested_field: 2,
            nested: NodeTag::ASSIGNMENT_EXPRESSION,
        },
        ExpressionShape {
            name: "logical and before logical or",
            source: "a || b && c;",
            root: NodeTag::LOGICAL_EXPRESSION,
            nested_field: 2,
            nested: NodeTag::LOGICAL_EXPRESSION,
        },
    ];
    let mut failures = Vec::new();

    for case in cases {
        if let Err(reason) = check_expression_shape(case) {
            failures.push(format!("{}: {reason}", case.name));
        }
    }

    assert_failures_empty(&failures);
}

/// Every ECMAScript statement family must produce its corresponding tape node without diagnostics.
#[test]
fn parser_should_accept_every_statement_family() {
    let cases = [
        GrammarCase::script("empty", ";", &[NodeTag::EMPTY_STATEMENT]),
        GrammarCase::script("block", "{ value; }", &[NodeTag::BLOCK_STATEMENT]),
        GrammarCase::script("expression", "value;", &[NodeTag::EXPRESSION_STATEMENT]),
        GrammarCase::script("debugger", "debugger;", &[NodeTag::DEBUGGER_STATEMENT]),
        GrammarCase::script(
            "declarations",
            "var a; let b = 1; const c = 2;",
            &[NodeTag::VARIABLE_DECLARATION, NodeTag::VARIABLE_DECLARATOR],
        ),
        GrammarCase::script(
            "if and else",
            "if (test) yes; else no;",
            &[NodeTag::IF_STATEMENT],
        ),
        GrammarCase::script(
            "switch",
            "switch (value) { case 1: hit; break; default: miss; }",
            &[
                NodeTag::SWITCH_STATEMENT,
                NodeTag::SWITCH_CASE,
                NodeTag::BREAK_STATEMENT,
            ],
        ),
        GrammarCase::script("throw", "throw value;", &[NodeTag::THROW_STATEMENT]),
        GrammarCase::script(
            "try catch finally",
            "try { work(); } catch (error) { recover(error); } finally { cleanup(); }",
            &[NodeTag::TRY_STATEMENT, NodeTag::CATCH_CLAUSE],
        ),
        GrammarCase::script(
            "loops",
            "while (ready) step(); do step(); while (ready); for (;;); for (key in object); for (value of list);",
            &[
                NodeTag::WHILE_STATEMENT,
                NodeTag::DO_WHILE_STATEMENT,
                NodeTag::FOR_STATEMENT,
                NodeTag::FOR_IN_STATEMENT,
                NodeTag::FOR_OF_STATEMENT,
            ],
        ),
        GrammarCase::script(
            "labels and jumps",
            "outer: while (ready) { if (skip) continue outer; break outer; }",
            &[
                NodeTag::LABELED_STATEMENT,
                NodeTag::WHILE_STATEMENT,
                NodeTag::CONTINUE_STATEMENT,
                NodeTag::BREAK_STATEMENT,
            ],
        ),
        GrammarCase::script("with", "with (object) value;", &[NodeTag::WITH_STATEMENT]),
        GrammarCase::script(
            "function and return",
            "function identity(value) { return value; }",
            &[NodeTag::FUNCTION_DECLARATION, NodeTag::RETURN_STATEMENT],
        ),
        GrammarCase::script(
            "class declaration",
            "class Point { constructor(x) { this.x = x; } }",
            &[NodeTag::CLASS_DECLARATION, NodeTag::METHOD_DEFINITION],
        ),
    ];

    assert_clean_cases(&cases);
}

/// Functions, classes, and modules cover declarations and expressions that introduce grammar context.
#[test]
fn parser_should_accept_functions_classes_and_modules() {
    let cases = [
        GrammarCase::script(
            "function expression",
            "const identity = function (value) { return value; };",
            &[NodeTag::FUNCTION_EXPRESSION, NodeTag::RETURN_STATEMENT],
        ),
        GrammarCase::script(
            "arrow function",
            "const add = (left, right) => left + right;",
            &[NodeTag::ARROW_FUNCTION_EXPRESSION],
        ),
        GrammarCase::script(
            "derived class members",
            "class Child extends Parent { #value = 1; static count = 0; method() { return super.method(); } }",
            &[
                NodeTag::CLASS_DECLARATION,
                NodeTag::CLASS_BODY,
                NodeTag::PROPERTY_DEFINITION,
                NodeTag::METHOD_DEFINITION,
                NodeTag::PRIVATE_IDENTIFIER,
            ],
        ),
        GrammarCase::script(
            "class expression",
            "const Child = class extends Parent { static { initialize(); } };",
            &[NodeTag::CLASS_EXPRESSION, NodeTag::STATIC_BLOCK],
        ),
        GrammarCase::module(
            "imports",
            "import defaultValue, * as namespace from 'package'; import { value as renamed } from 'other';",
            &[
                NodeTag::IMPORT_DECLARATION,
                NodeTag::IMPORT_DEFAULT_SPECIFIER,
                NodeTag::IMPORT_NAMESPACE_SPECIFIER,
                NodeTag::IMPORT_SPECIFIER,
            ],
        ),
        GrammarCase::module(
            "exports",
            "export const value = 1; export { value as renamed }; export * from 'other'; export default value;",
            &[
                NodeTag::EXPORT_NAMED_DECLARATION,
                NodeTag::EXPORT_SPECIFIER,
                NodeTag::EXPORT_ALL_DECLARATION,
                NodeTag::EXPORT_DEFAULT_DECLARATION,
            ],
        ),
        GrammarCase::module(
            "dynamic import and metadata",
            "const module = import('package'); const url = import.meta.url;",
            &[NodeTag::IMPORT_EXPRESSION, NodeTag::META_PROPERTY],
        ),
    ];

    assert_clean_cases(&cases);
}

/// Empty arrow parameter lists accept either expression or block bodies in nested expression positions.
#[test]
fn parser_should_accept_zero_parameter_arrow_functions() {
    let cases = [
        GrammarCase::script(
            "expression and block bodies",
            "const expression = () => value; const block = () => { return value; };",
            &[
                NodeTag::ARROW_FUNCTION_EXPRESSION,
                NodeTag::BLOCK_STATEMENT,
                NodeTag::RETURN_STATEMENT,
            ],
        ),
        GrammarCase::script(
            "comment trivia",
            "const callback = (/* parameters */) /* arrow */ => value;",
            &[NodeTag::ARROW_FUNCTION_EXPRESSION],
        ),
        GrammarCase::script(
            "nested and parenthesized",
            "promise.then(() => value); const invoked = (() => value)();",
            &[
                NodeTag::ARROW_FUNCTION_EXPRESSION,
                NodeTag::CALL_EXPRESSION,
                NodeTag::PARENTHESIZED_EXPRESSION,
            ],
        ),
    ];

    assert_clean_cases(&cases);
}

/// Line terminators before `=>` stay invalid, and truncated bodies retain a recoverable tape.
#[test]
fn parser_should_recover_invalid_zero_parameter_arrow_functions() {
    let cases = [
        GrammarCase::script("direct line break", "const callback = ()\n=> value;", &[]),
        GrammarCase::script(
            "comment line break",
            "const callback = ()/*\n*/=> value;",
            &[],
        ),
        GrammarCase::script(
            "truncated body",
            "const callback = () =>",
            &[NodeTag::ARROW_FUNCTION_EXPRESSION],
        ),
    ];

    assert_diagnostic_cases(&cases, false);
}

/// Parameter and `var` bindings belong to their nearest function and must not leak into siblings.
#[test]
fn parser_should_isolate_function_scopes() {
    let cases = [GrammarCase::script(
        "sibling function scopes",
        "function first(value) { var local; }\
         function second(value) { var local; }\
         class Example { first(value) { var local; } second(value) { var local; } }\
         const firstArrow = (value) => { var local; return value; };\
         const secondArrow = async (value) => { var local; return value; };",
        &[
            NodeTag::FUNCTION_DECLARATION,
            NodeTag::METHOD_DEFINITION,
            NodeTag::ARROW_FUNCTION_EXPRESSION,
        ],
    )];

    assert_clean_cases(&cases);
}

/// Binding and assignment positions must distinguish patterns from array/object expressions.
#[test]
fn parser_should_accept_binding_and_assignment_patterns() {
    let cases = [
        GrammarCase::script(
            "array binding",
            "const [first, , third = 3, ...rest] = values;",
            &[
                NodeTag::ARRAY_PATTERN,
                NodeTag::ASSIGNMENT_PATTERN,
                NodeTag::REST_ELEMENT,
            ],
        ),
        GrammarCase::script(
            "object binding",
            "const { value: renamed = 1, shorthand, ...rest } = object;",
            &[
                NodeTag::OBJECT_PATTERN,
                NodeTag::ASSIGNMENT_PATTERN,
                NodeTag::REST_ELEMENT,
            ],
        ),
        GrammarCase::script(
            "parameter patterns",
            "function read({ value }, [first], fallback = 0, ...rest) { return value; }",
            &[
                NodeTag::OBJECT_PATTERN,
                NodeTag::ARRAY_PATTERN,
                NodeTag::ASSIGNMENT_PATTERN,
                NodeTag::REST_ELEMENT,
            ],
        ),
        GrammarCase::script(
            "assignment patterns",
            "({ value, nested: [first] } = source);",
            &[
                NodeTag::OBJECT_PATTERN,
                NodeTag::ARRAY_PATTERN,
                NodeTag::ASSIGNMENT_EXPRESSION,
            ],
        ),
    ];

    assert_clean_cases(&cases);
}

/// Declaration initializers and binding-element defaults occupy distinct `ESTree` fields.
#[test]
#[allow(clippy::too_many_lines)]
fn parser_should_separate_declaration_initializers_from_binding_defaults() {
    let source = "const value = source, second = other;\
                  for (let index = 0; index < limit; index++) {}\
                  function defaults(value = fallback, { key } = object, [first] = list, ...rest) {}\
                  const [nested = 1, { item: renamed = fallback }, [inner] = list] = source;";
    let parsed = parse(
        source,
        ParseOptions {
            source_kind: SourceKind::Script,
            ..ParseOptions::default()
        },
    )
    .expect("parse binding defaults");
    inspect_tape("binding defaults", &parsed).expect("valid binding-default tape");
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);

    let declarators = parsed
        .tape
        .validation()
        .filter_map(|record| match record.expect("valid record").value {
            TapeValue::Node {
                tag: NodeTag::VARIABLE_DECLARATOR,
                fields,
                ..
            } => Some(fields.to_vec()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(declarators.len(), 4);
    for (declarator, expected_init) in
        declarators[..3]
            .iter()
            .zip([NodeTag::IDENTIFIER, NodeTag::IDENTIFIER, NodeTag::LITERAL])
    {
        assert_eq!(
            node_tag(&parsed.tape, declarator[0]),
            Ok(NodeTag::IDENTIFIER)
        );
        assert_eq!(node_tag(&parsed.tape, declarator[1]), Ok(expected_init));
    }
    assert_eq!(
        node_tag(&parsed.tape, declarators[3][0]),
        Ok(NodeTag::ARRAY_PATTERN)
    );
    assert_eq!(
        node_tag(&parsed.tape, declarators[3][1]),
        Ok(NodeTag::IDENTIFIER)
    );

    let function = parsed
        .tape
        .validation()
        .find_map(|record| match record.expect("valid record").value {
            TapeValue::Node {
                tag: NodeTag::FUNCTION_DECLARATION,
                fields,
                ..
            } => Some(fields.to_vec()),
            _ => None,
        })
        .expect("function declaration");
    let parameters = list_items(&parsed.tape, function[1]).expect("function parameters");
    assert_eq!(parameters.len(), 4);
    assert_eq!(
        node_tag(&parsed.tape, parameters[0]),
        Ok(NodeTag::ASSIGNMENT_PATTERN)
    );
    assert_eq!(
        node_tag(&parsed.tape, parameters[1]),
        Ok(NodeTag::ASSIGNMENT_PATTERN)
    );
    assert_eq!(
        node_tag(&parsed.tape, parameters[2]),
        Ok(NodeTag::ASSIGNMENT_PATTERN)
    );
    assert_eq!(
        node_tag(&parsed.tape, parameters[3]),
        Ok(NodeTag::REST_ELEMENT)
    );
    let object_default = node_fields(&parsed.tape, parameters[1], NodeTag::ASSIGNMENT_PATTERN)
        .expect("object default");
    let array_default = node_fields(&parsed.tape, parameters[2], NodeTag::ASSIGNMENT_PATTERN)
        .expect("array default");
    assert_eq!(
        node_tag(&parsed.tape, object_default[0]),
        Ok(NodeTag::OBJECT_PATTERN)
    );
    assert_eq!(
        node_tag(&parsed.tape, array_default[0]),
        Ok(NodeTag::ARRAY_PATTERN)
    );
    let rest = node_fields(&parsed.tape, parameters[3], NodeTag::REST_ELEMENT).expect("rest");
    assert_eq!(node_tag(&parsed.tape, rest[0]), Ok(NodeTag::IDENTIFIER));

    let nested = node_fields(&parsed.tape, declarators[3][0], NodeTag::ARRAY_PATTERN)
        .expect("nested pattern");
    let nested = list_items(&parsed.tape, nested[0]).expect("nested elements");
    assert_eq!(
        node_tag(&parsed.tape, nested[0]),
        Ok(NodeTag::ASSIGNMENT_PATTERN)
    );
    assert_eq!(
        node_tag(&parsed.tape, nested[1]),
        Ok(NodeTag::OBJECT_PATTERN)
    );
    assert_eq!(
        node_tag(&parsed.tape, nested[2]),
        Ok(NodeTag::ASSIGNMENT_PATTERN)
    );
}

/// Rest bindings reject defaults while retaining a validated recovery tree.
#[test]
fn parser_should_diagnose_rest_binding_defaults() {
    assert_diagnostic_cases(
        &[
            GrammarCase::script(
                "parameter rest default",
                "function invalid(...rest = fallback) {}",
                &[NodeTag::REST_ELEMENT, NodeTag::ASSIGNMENT_PATTERN],
            ),
            GrammarCase::script(
                "array rest default",
                "const [...rest = fallback] = source;",
                &[NodeTag::ARRAY_PATTERN, NodeTag::REST_ELEMENT],
            ),
            GrammarCase::script(
                "object rest default",
                "const { ...rest = fallback } = source;",
                &[NodeTag::OBJECT_PATTERN, NodeTag::REST_ELEMENT],
            ),
        ],
        false,
    );
}

/// Catch parameters use binding-pattern nodes without conflicting with the enclosing scope.
#[test]
fn parser_should_preserve_catch_binding_patterns_and_scope() {
    let source = "let message;\
                  try {} catch ({ message, code = 1, ...rest }) {}\
                  try {} catch ([first, , third = 3, ...tail]) {}\
                  try {} catch {}";
    let parsed = parse(
        source,
        ParseOptions {
            source_kind: SourceKind::Script,
            semantic_errors: true,
            ..ParseOptions::default()
        },
    )
    .expect("parse catch patterns");
    inspect_tape("catch patterns", &parsed).expect("valid catch-pattern tape");
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);

    let program =
        node_fields(&parsed.tape, parsed.tape.header().root, NodeTag::PROGRAM).expect("Program");
    let body = list_items(&parsed.tape, program[0]).expect("Program body");

    let object_try =
        node_fields(&parsed.tape, body[1], NodeTag::TRY_STATEMENT).expect("object try");
    let object_catch =
        node_fields(&parsed.tape, object_try[1], NodeTag::CATCH_CLAUSE).expect("object catch");
    let object = node_fields(&parsed.tape, object_catch[0], NodeTag::OBJECT_PATTERN)
        .expect("object pattern");
    let properties = list_items(&parsed.tape, object[0]).expect("object properties");
    assert_eq!(properties.len(), 3);
    assert_eq!(node_tag(&parsed.tape, properties[0]), Ok(NodeTag::PROPERTY));
    let default_property =
        node_fields(&parsed.tape, properties[1], NodeTag::PROPERTY).expect("default property");
    assert_eq!(
        node_tag(&parsed.tape, default_property[1]),
        Ok(NodeTag::ASSIGNMENT_PATTERN)
    );
    assert_eq!(
        node_tag(&parsed.tape, properties[2]),
        Ok(NodeTag::REST_ELEMENT)
    );

    let array_try = node_fields(&parsed.tape, body[2], NodeTag::TRY_STATEMENT).expect("array try");
    let array_catch =
        node_fields(&parsed.tape, array_try[1], NodeTag::CATCH_CLAUSE).expect("array catch");
    let array =
        node_fields(&parsed.tape, array_catch[0], NodeTag::ARRAY_PATTERN).expect("array pattern");
    let elements = list_items(&parsed.tape, array[0]).expect("array elements");
    assert_eq!(elements.len(), 4);
    assert!(matches!(
        parsed.tape.value_at(elements[1]),
        Ok(TapeValue::Null)
    ));
    assert_eq!(
        node_tag(&parsed.tape, elements[2]),
        Ok(NodeTag::ASSIGNMENT_PATTERN)
    );
    assert_eq!(
        node_tag(&parsed.tape, elements[3]),
        Ok(NodeTag::REST_ELEMENT)
    );

    let optional_try =
        node_fields(&parsed.tape, body[3], NodeTag::TRY_STATEMENT).expect("optional try");
    let optional_catch =
        node_fields(&parsed.tape, optional_try[1], NodeTag::CATCH_CLAUSE).expect("optional catch");
    assert!(matches!(
        parsed.tape.value_at(optional_catch[0]),
        Ok(TapeValue::Null)
    ));
}

/// Catch parameters reject initializers and commas following rest bindings while recovering a tree.
#[test]
fn parser_should_diagnose_invalid_catch_bindings() {
    let cases = [
        (
            "try {} catch (error = fallback) {}",
            "expected RightParen, found Eq",
        ),
        (
            "try {} catch ([...rest, tail]) {}",
            "rest element must be last",
        ),
        ("try {} catch ([...rest,]) {}", "rest element must be last"),
        (
            "try {} catch ({ ...rest, }) {}",
            "rest property must be last",
        ),
    ];

    for (source, expected) in cases {
        let parsed = parse(source, ParseOptions::default()).expect("recover invalid catch");
        inspect_tape("invalid catch", &parsed).expect("valid recovered catch tape");
        assert_eq!(
            parsed
                .diagnostics
                .first()
                .map(|diagnostic| &*diagnostic.message),
            Some(expected)
        );
    }
}

/// Regular expressions and template literals require parser-directed rescanning after `/`, `}`, and tags.
#[test]
fn parser_should_accept_regular_expressions_and_templates() {
    let cases = [
        GrammarCase::script(
            "regular expression",
            "const matcher = /a(?:b|c)+/giu;",
            &[NodeTag::LITERAL],
        ),
        GrammarCase::script(
            "plain template",
            "const message = `plain text`;",
            &[NodeTag::TEMPLATE_LITERAL, NodeTag::TEMPLATE_ELEMENT],
        ),
        GrammarCase::script(
            "interpolated template",
            "const message = `hello ${name}, ${count + 1}`;",
            &[NodeTag::TEMPLATE_LITERAL, NodeTag::TEMPLATE_ELEMENT],
        ),
        GrammarCase::script(
            "tagged template",
            "html`<p>${content}</p>`;",
            &[
                NodeTag::TAGGED_TEMPLATE_EXPRESSION,
                NodeTag::TEMPLATE_LITERAL,
            ],
        ),
    ];

    assert_clean_cases(&cases);
}

/// Optional member, element, and call chains must be wrapped once in a chain expression.
#[test]
fn parser_should_accept_optional_chaining() {
    let cases = [
        GrammarCase::script(
            "optional member",
            "object?.value;",
            &[NodeTag::CHAIN_EXPRESSION, NodeTag::MEMBER_EXPRESSION],
        ),
        GrammarCase::script(
            "optional element",
            "object?.[key];",
            &[NodeTag::CHAIN_EXPRESSION, NodeTag::MEMBER_EXPRESSION],
        ),
        GrammarCase::script(
            "optional call",
            "callback?.(argument);",
            &[NodeTag::CHAIN_EXPRESSION, NodeTag::CALL_EXPRESSION],
        ),
        GrammarCase::script(
            "mixed optional chain",
            "object?.method?.(argument)?.result;",
            &[
                NodeTag::CHAIN_EXPRESSION,
                NodeTag::MEMBER_EXPRESSION,
                NodeTag::CALL_EXPRESSION,
            ],
        ),
    ];

    assert_clean_cases(&cases);
}

/// Dot member properties accept `IdentifierName` keywords and declared private names.
#[test]
fn parser_should_accept_keyword_and_private_member_names() {
    let cases = [
        GrammarCase::script(
            "every keyword IdentifierName",
            "object.break.case.catch.class.const.continue.debugger.default.delete.do.else.export.extends.false.finally.for.function.if.import.in.instanceof.new.null.return.super.switch.this.throw.true.try.typeof.var.void.while.with.yield.async.await.let.static.of.get.set.as.satisfies.accessor.using.declare.abstract.interface.type.enum.namespace.module.implements.infer.keyof.readonly.unique.unknown.never.any.boolean.number.string.symbol.object.undefined.is.asserts.public.protected.private.override.out.meta.from.require;",
            &[NodeTag::MEMBER_EXPRESSION],
        ),
        GrammarCase::script(
            "optional keyword IdentifierName",
            "iterator?.return;",
            &[NodeTag::CHAIN_EXPRESSION, NodeTag::MEMBER_EXPRESSION],
        ),
        GrammarCase::script(
            "declared private member",
            "class C { read(object) { return object.#field; } optional(object) { return object?.#field; } #field = 1; }",
            &[
                NodeTag::CLASS_DECLARATION,
                NodeTag::PRIVATE_IDENTIFIER,
                NodeTag::CHAIN_EXPRESSION,
                NodeTag::MEMBER_EXPRESSION,
            ],
        ),
    ];

    assert_clean_cases(&cases);
}

/// Private member names retain class-scope declaration diagnostics.
#[test]
fn parser_should_diagnose_invalid_private_member_scope() {
    let cases = [
        (
            "object.#field;",
            "private name is only valid inside a class",
        ),
        (
            "class C { read(object) { return object.#missing; } }",
            "private name `missing` is not declared",
        ),
    ];

    for (source, expected_message) in cases {
        let parsed = parse(source, ParseOptions::default()).expect("recover parse");
        assert!(
            parsed
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message == expected_message),
            "missing `{expected_message}` in {:?}",
            parsed.diagnostics
        );
    }
}

/// Async and generator context controls whether `await` and `yield` are expressions or identifiers.
#[test]
fn parser_should_accept_async_functions_and_generators() {
    let cases = [
        GrammarCase::script(
            "generator",
            "function* sequence() { yield 1; yield* other(); }",
            &[NodeTag::FUNCTION_DECLARATION, NodeTag::YIELD_EXPRESSION],
        ),
        GrammarCase::script(
            "async function",
            "async function load() { return await fetchValue(); }",
            &[NodeTag::FUNCTION_DECLARATION, NodeTag::AWAIT_EXPRESSION],
        ),
        GrammarCase::script(
            "async arrow",
            "const load = async (value) => await transform(value);",
            &[
                NodeTag::ARROW_FUNCTION_EXPRESSION,
                NodeTag::AWAIT_EXPRESSION,
            ],
        ),
        GrammarCase::module(
            "top-level await",
            "const value = await load();",
            &[NodeTag::AWAIT_EXPRESSION],
        ),
        GrammarCase::module(
            "for await",
            "async function consume() { for await (const value of stream) { await use(value); } }",
            &[
                NodeTag::FUNCTION_DECLARATION,
                NodeTag::FOR_OF_STATEMENT,
                NodeTag::AWAIT_EXPRESSION,
            ],
        ),
        GrammarCase::module(
            "exported async functions",
            "export async function load() { return await fetchValue(); } export default async function* stream() { yield await next(); }",
            &[
                NodeTag::EXPORT_NAMED_DECLARATION,
                NodeTag::EXPORT_DEFAULT_DECLARATION,
                NodeTag::FUNCTION_DECLARATION,
                NodeTag::AWAIT_EXPRESSION,
                NodeTag::YIELD_EXPRESSION,
            ],
        ),
    ];

    assert_clean_cases(&cases);
}

/// `async function` export forms may not cross a line terminator.
#[test]
fn parser_should_respect_line_breaks_in_exported_async_functions() {
    assert_diagnostic_cases(
        &[GrammarCase::module(
            "named export line break",
            "export async/*\n*/function split() {}",
            &[
                NodeTag::EXPORT_NAMED_DECLARATION,
                NodeTag::FUNCTION_DECLARATION,
            ],
        )],
        false,
    );
    assert_clean_cases(&[GrammarCase::module(
        "default export line break",
        "export default async\nfunction split() {}",
        &[
            NodeTag::EXPORT_DEFAULT_DECLARATION,
            NodeTag::FUNCTION_DECLARATION,
        ],
    )]);
}

/// Malformed exported async functions recover inside their original export wrappers.
#[test]
fn parser_should_recover_malformed_exported_async_functions() {
    assert_diagnostic_cases(
        &[
            GrammarCase::module(
                "malformed named async function export",
                "export async function broken(",
                &[
                    NodeTag::EXPORT_NAMED_DECLARATION,
                    NodeTag::FUNCTION_DECLARATION,
                ],
            ),
            GrammarCase::module(
                "malformed default async generator export",
                "export default async function* broken(",
                &[
                    NodeTag::EXPORT_DEFAULT_DECLARATION,
                    NodeTag::FUNCTION_DECLARATION,
                ],
            ),
        ],
        false,
    );
}

/// Generator methods preserve prefixes, computed names, and isolated function scopes.
#[test]
fn parser_should_accept_object_and_class_generator_methods() {
    let cases = [
        GrammarCase::script(
            "object generator methods",
            "const methods = { *plain(value) { yield value; }, *[key](value) { yield value; }, async *stream(value) { await load(value); yield value; }, async: asyncValue, get: getValue, set: setValue, static: staticValue };",
            &[
                NodeTag::OBJECT_EXPRESSION,
                NodeTag::PROPERTY,
                NodeTag::FUNCTION_EXPRESSION,
                NodeTag::YIELD_EXPRESSION,
                NodeTag::AWAIT_EXPRESSION,
            ],
        ),
        GrammarCase::script(
            "class generator methods",
            "class Methods { *plain(value) { yield value; } *[key](value) { yield value; } async *stream(value) { await load(value); yield value; } static *values(value) { yield value; } static async *entries(value) { await load(value); yield value; } static() {} async() {} get() {} set() {} }",
            &[
                NodeTag::CLASS_DECLARATION,
                NodeTag::METHOD_DEFINITION,
                NodeTag::FUNCTION_EXPRESSION,
                NodeTag::YIELD_EXPRESSION,
                NodeTag::AWAIT_EXPRESSION,
            ],
        ),
    ];

    assert_clean_cases(&cases);
}

/// Public class accessors reuse method functions while preserving ordinary `get` and `set` names.
#[test]
fn parser_should_accept_public_class_accessors() {
    let cases = [
        GrammarCase::script(
            "public class accessors",
            "class Accessors { get value() { return this._value; } set value({ next } = fallback) { this._value = next; } static get [key]() { return value; } static set \"named\"(value) {} get static() {} static get static() {} }",
            &[
                NodeTag::CLASS_DECLARATION,
                NodeTag::METHOD_DEFINITION,
                NodeTag::FUNCTION_EXPRESSION,
                NodeTag::ASSIGNMENT_PATTERN,
                NodeTag::OBJECT_PATTERN,
            ],
        ),
        GrammarCase::script(
            "ordinary get and set members",
            "class Names { get() {} set(value) {} get; set = value; static get() {} static set(value) {} }",
            &[NodeTag::METHOD_DEFINITION, NodeTag::PROPERTY_DEFINITION],
        ),
        GrammarCase::script(
            "computed special accessor names",
            "class Special { get ['constructor']() {} set ['constructor'](value) {} static get ['prototype']() {} static set ['prototype'](value) {} }",
            &[NodeTag::METHOD_DEFINITION, NodeTag::FUNCTION_EXPRESSION],
        ),
        GrammarCase::script(
            "accessor super property and nested constructor",
            "class Outer extends Base { get inherited() { return super.value; } set inherited(value) { super.value = value; } get Nested() { return class Inner extends Base { constructor() { super(); } }; } }",
            &[
                NodeTag::METHOD_DEFINITION,
                NodeTag::MEMBER_EXPRESSION,
                NodeTag::CLASS_EXPRESSION,
            ],
        ),
    ];

    assert_clean_cases(&cases);
}

/// Private accessors share one canonical private name while preserving static and Unicode keys.
#[test]
fn parser_should_accept_private_class_accessors() {
    let cases = [
        GrammarCase::script(
            "paired escaped private accessors",
            "class Accessors { get #\\u0076alue() { return this.#value; } set #value({ next } = fallback) { this.#value = next; } }",
            &[
                NodeTag::CLASS_DECLARATION,
                NodeTag::METHOD_DEFINITION,
                NodeTag::PRIVATE_IDENTIFIER,
                NodeTag::FUNCTION_EXPRESSION,
                NodeTag::ASSIGNMENT_PATTERN,
                NodeTag::OBJECT_PATTERN,
            ],
        ),
        GrammarCase::script(
            "static and Unicode private accessors",
            "class Accessors { static get #π() { return this.#π; } static set #π(value) { this.#π = value; } get #only() {} set #write(value) {} }",
            &[
                NodeTag::METHOD_DEFINITION,
                NodeTag::PRIVATE_IDENTIFIER,
                NodeTag::MEMBER_EXPRESSION,
            ],
        ),
        GrammarCase::script(
            "nested private accessor scopes",
            "class Outer { get #value() { return class Inner { get #value() { return this.#value; } set #value(next) {} }; } set #value(next) {} }",
            &[
                NodeTag::CLASS_DECLARATION,
                NodeTag::CLASS_EXPRESSION,
                NodeTag::METHOD_DEFINITION,
            ],
        ),
    ];

    assert_clean_cases(&cases);
}

/// Accessor arity and noncomputed class special-name restrictions are early errors.
#[test]
fn parser_should_diagnose_public_class_accessor_early_errors() {
    let cases = [
        GrammarCase::script(
            "getter parameter",
            "class C { get value(parameter) {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "setter without parameter",
            "class C { set value() {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "setter with two parameters",
            "class C { set value(first, second) {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "setter rest parameter",
            "class C { set value(...values) {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "setter trailing comma",
            "class C { set value(parameter,) {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "getter constructor",
            "class C { get constructor() {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "quoted setter constructor",
            "class C { set 'constructor'(value) {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "escaped quoted getter constructor",
            "class C { get \"constr\\u0075ctor\"() {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "escaped identifier setter constructor",
            "class C { set constr\\u0075ctor(value) {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "static getter prototype",
            "class C { static get prototype() {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "quoted static setter prototype",
            "class C { static set 'prototype'(value) {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "escaped static getter prototype",
            "class C { static get prot\\u006ftype() {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "direct super in getter",
            "class C extends Base { get value() { super(); } }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "direct super in static setter",
            "class C extends Base { static set value(next) { super(); } }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "strict with in getter",
            "class C { get value() { with (object) statement; } }",
            &[NodeTag::METHOD_DEFINITION, NodeTag::WITH_STATEMENT],
        ),
        GrammarCase::script(
            "strict delete in setter",
            "class C { set value(next) { delete identifier; } }",
            &[NodeTag::METHOD_DEFINITION, NodeTag::UNARY_EXPRESSION],
        ),
        GrammarCase::script(
            "escaped get introducer",
            "class C { g\\u0065t value() {} }",
            &[NodeTag::PROPERTY_DEFINITION, NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "escaped set introducer",
            "class C { s\\u0065t value(next) {} }",
            &[NodeTag::PROPERTY_DEFINITION, NodeTag::METHOD_DEFINITION],
        ),
    ];

    assert_diagnostic_cases(&cases, true);
}

/// Private accessor arity, private-name collisions, and class-only syntax are early errors.
#[test]
fn parser_should_diagnose_private_class_accessor_early_errors() {
    let cases = [
        GrammarCase::script(
            "private getter parameter",
            "class C { get #value(parameter) {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "private setter without parameter",
            "class C { set #value() {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "private setter rest parameter",
            "class C { set #value(...values) {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "private setter trailing comma",
            "class C { set #value(parameter,) {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "duplicate private getters",
            "class C { get #value() {} get #value() {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "duplicate private setters",
            "class C { set #value(next) {} set #value(next) {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "private getter after field",
            "class C { #value; get #value() {} }",
            &[NodeTag::PROPERTY_DEFINITION, NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "private field after setter",
            "class C { set #value(next) {} #value; }",
            &[NodeTag::METHOD_DEFINITION, NodeTag::PROPERTY_DEFINITION],
        ),
        GrammarCase::script(
            "private setter after method",
            "class C { #value() {} set #value(next) {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "private method after getter",
            "class C { get #value() {} #value() {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "mixed static private accessors",
            "class C { get #value() {} static set #value(next) {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "mixed escaped static private accessors",
            "class C { static get #\\u0076alue() {} set #value(next) {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "private constructor accessor",
            "class C { get #constructor() {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "direct super in private getter",
            "class C extends Base { get #value() { super(); } }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "strict private setter body",
            "class C { set #value(next) { with (object) statement; } }",
            &[NodeTag::METHOD_DEFINITION, NodeTag::WITH_STATEMENT],
        ),
        GrammarCase::script(
            "escaped private get introducer",
            "class C { g\\u0065t #value() {} }",
            &[NodeTag::PROPERTY_DEFINITION, NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "split private name",
            "class C { get # value() {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "object private getter",
            "const object = { get #value() {} };",
            &[NodeTag::OBJECT_EXPRESSION],
        ),
        GrammarCase::script(
            "object private setter inside class",
            "class C { method() { return { set #value(next) {} }; } }",
            &[NodeTag::OBJECT_EXPRESSION, NodeTag::METHOD_DEFINITION],
        ),
    ];

    assert_diagnostic_cases(&cases, true);
}

/// Object accessors preserve literal and computed names without changing ordinary property roles.
#[test]
fn parser_should_accept_public_object_accessors() {
    let cases = [
        GrammarCase::script(
            "public object accessors",
            "const accessors = { get value() { return this._value; }, set value({ next } = fallback) { this._value = next; }, get [key]() { return value; }, set 'named'(value) {}, get 0() {}, set 1n(value) {}, get return() {}, set async(value) {} };",
            &[
                NodeTag::OBJECT_EXPRESSION,
                NodeTag::PROPERTY,
                NodeTag::FUNCTION_EXPRESSION,
                NodeTag::ASSIGNMENT_PATTERN,
                NodeTag::OBJECT_PATTERN,
            ],
        ),
        GrammarCase::script(
            "ordinary get and set properties",
            "const names = { get() {}, set(value) {}, get, set, get: getter, set: setter, *generator() {}, *get() {}, async *set() {} };",
            &[
                NodeTag::OBJECT_EXPRESSION,
                NodeTag::PROPERTY,
                NodeTag::FUNCTION_EXPRESSION,
            ],
        ),
        GrammarCase::script(
            "object accessor super property and sloppy body",
            "const inherited = { get value() { with (object) return super.value; }, set value(next) { super.value = next; delete identifier; } };",
            &[
                NodeTag::PROPERTY,
                NodeTag::MEMBER_EXPRESSION,
                NodeTag::WITH_STATEMENT,
                NodeTag::UNARY_EXPRESSION,
            ],
        ),
        GrammarCase::script(
            "object accessor strict directive does not leak",
            "const strict = { get value() { 'use strict'; return this.value; }, set value(next) { 'use strict'; this.value = next; } }; with (target) statement;",
            &[
                NodeTag::PROPERTY,
                NodeTag::FUNCTION_EXPRESSION,
                NodeTag::WITH_STATEMENT,
            ],
        ),
        GrammarCase::module(
            "strict property and member identifier names",
            "const names = { static: 1, interface: 2, get public() { delete target.static; return target.static + target.interface; }, set private(value) { target.protected = value; } };",
            &[
                NodeTag::PROPERTY,
                NodeTag::MEMBER_EXPRESSION,
                NodeTag::UNARY_EXPRESSION,
            ],
        ),
        GrammarCase::script(
            "strict delete computed member containing private access",
            "class C { #key; get value() { delete target[this.#key]; return target[this.#key]; } }",
            &[
                NodeTag::METHOD_DEFINITION,
                NodeTag::MEMBER_EXPRESSION,
                NodeTag::UNARY_EXPRESSION,
            ],
        ),
    ];

    assert_clean_cases(&cases);
}

/// Object accessor arity, direct-super, and contextual introducer restrictions are early errors.
#[test]
fn parser_should_diagnose_public_object_accessor_early_errors() {
    let cases = [
        GrammarCase::script(
            "getter parameter",
            "const object = { get value(parameter) {} };",
            &[NodeTag::PROPERTY],
        ),
        GrammarCase::script(
            "setter without parameter",
            "const object = { set value() {} };",
            &[NodeTag::PROPERTY],
        ),
        GrammarCase::script(
            "setter with two parameters",
            "const object = { set value(first, second) {} };",
            &[NodeTag::PROPERTY],
        ),
        GrammarCase::script(
            "setter rest parameter",
            "const object = { set value(...values) {} };",
            &[NodeTag::PROPERTY],
        ),
        GrammarCase::script(
            "setter trailing comma",
            "const object = { set value(parameter,) {} };",
            &[NodeTag::PROPERTY],
        ),
        GrammarCase::script(
            "direct super in getter",
            "const object = { get value() { super(); } };",
            &[NodeTag::PROPERTY],
        ),
        GrammarCase::script(
            "direct super in setter",
            "const object = { set value(next) { super(); } };",
            &[NodeTag::PROPERTY],
        ),
        GrammarCase::script(
            "escaped get introducer",
            "const object = { g\\u0065t value() {} };",
            &[NodeTag::PROPERTY],
        ),
        GrammarCase::script(
            "escaped set introducer",
            "const object = { s\\u0065t value(next) {} };",
            &[NodeTag::PROPERTY],
        ),
        GrammarCase::script(
            "generator after get",
            "const object = { get *value() {} };",
            &[NodeTag::PROPERTY],
        ),
    ];

    assert_diagnostic_cases(&cases, true);
}

/// Cover grammar and strict accessor bodies retain their surrounding early errors.
#[test]
fn parser_should_diagnose_object_accessor_context_errors() {
    let cases = [
        GrammarCase::script(
            "accessor in assignment pattern",
            "0, [{ get value() {} }] = [{}];",
            &[NodeTag::ARRAY_PATTERN, NodeTag::OBJECT_PATTERN],
        ),
        GrammarCase::script(
            "accessor in for-of assignment pattern",
            "for ([{ set value(next) {} }] of source) {}",
            &[NodeTag::FOR_OF_STATEMENT, NodeTag::OBJECT_PATTERN],
        ),
        GrammarCase::script(
            "object accessor optional chain assignment",
            "[{ set value(next) {} }?.value = 1] = [2];",
            &[NodeTag::CHAIN_EXPRESSION, NodeTag::PROPERTY],
        ),
        GrammarCase::script(
            "use strict getter body",
            "const object = { get value() { 'use strict'; public = 1; } };",
            &[NodeTag::PROPERTY],
        ),
        GrammarCase::script(
            "use strict setter parameter",
            "const object = { set value(eval) { 'use strict'; } };",
            &[NodeTag::PROPERTY],
        ),
        GrammarCase::script(
            "use strict setter with default",
            "const object = { set value(next = 0) { 'use strict'; } };",
            &[NodeTag::PROPERTY, NodeTag::ASSIGNMENT_PATTERN],
        ),
        GrammarCase::module(
            "export declaration in getter",
            "const object = { get value() { export default null; } };",
            &[NodeTag::PROPERTY, NodeTag::EXPORT_DEFAULT_DECLARATION],
        ),
        GrammarCase::module(
            "import declaration in setter",
            "const object = { set value(next) { import value from './value.js'; } };",
            &[NodeTag::PROPERTY, NodeTag::IMPORT_DECLARATION],
        ),
        GrammarCase::module(
            "strict with in getter",
            "const object = { get value() { with (target) statement; } };",
            &[NodeTag::PROPERTY, NodeTag::WITH_STATEMENT],
        ),
        GrammarCase::module(
            "strict delete in setter",
            "const object = { set value(next) { delete identifier; } };",
            &[NodeTag::PROPERTY, NodeTag::UNARY_EXPRESSION],
        ),
        GrammarCase::script(
            "strict delete private member",
            "class C { #value; method() { delete this.#value; } }",
            &[NodeTag::CLASS_DECLARATION, NodeTag::UNARY_EXPRESSION],
        ),
        GrammarCase::script(
            "hashbang before strict directive",
            "#!/usr/bin/env node\n'use strict'; with (target) statement;",
            &[NodeTag::WITH_STATEMENT],
        ),
    ];

    assert_diagnostic_cases(&cases, true);
}

/// Generator methods retain binding and constructor early errors.
#[test]
fn parser_should_diagnose_generator_method_early_errors() {
    let cases = [
        GrammarCase::script(
            "duplicate object generator parameter",
            "const methods = { *invalid(value, value) {} };",
            &[NodeTag::PROPERTY, NodeTag::FUNCTION_EXPRESSION],
        ),
        GrammarCase::script(
            "yield class generator parameter",
            "class Methods { *invalid(yield) {} }",
            &[NodeTag::METHOD_DEFINITION, NodeTag::FUNCTION_EXPRESSION],
        ),
        GrammarCase::script(
            "generator constructor",
            "class Methods { *constructor() {} }",
            &[NodeTag::METHOD_DEFINITION, NodeTag::FUNCTION_EXPRESSION],
        ),
        GrammarCase::script(
            "static generator prototype",
            "class Methods { static *prototype() {} }",
            &[NodeTag::METHOD_DEFINITION, NodeTag::FUNCTION_EXPRESSION],
        ),
        GrammarCase::script(
            "private generator constructor",
            "class Methods { static async *#constructor() {} }",
            &[NodeTag::METHOD_DEFINITION, NodeTag::FUNCTION_EXPRESSION],
        ),
    ];

    assert_diagnostic_cases(&cases, true);
}

/// Property names retain their key form and method, field, or shorthand role.
#[test]
fn parser_should_accept_property_names_across_objects_patterns_and_classes() {
    let cases = [
        GrammarCase::script(
            "object property names",
            "const object = { [key]: value, return: keyword, \"text\": stringValue, 0: numeric, 1n: bigint, shorthand, [method]() { return value; }, default() { return value; } };",
            &[
                NodeTag::OBJECT_EXPRESSION,
                NodeTag::PROPERTY,
                NodeTag::FUNCTION_EXPRESSION,
            ],
        ),
        GrammarCase::script(
            "binding pattern property names",
            "const { [key]: computed, return: keyword, \"text\": stringValue, 0: numeric, 1n: bigint, shorthand = fallback } = source;",
            &[
                NodeTag::OBJECT_PATTERN,
                NodeTag::PROPERTY,
                NodeTag::ASSIGNMENT_PATTERN,
            ],
        ),
        GrammarCase::script(
            "assignment pattern property names",
            "({ [key]: computed, return: keyword, \"text\": stringValue, 0: numeric, 1n: bigint } = source);",
            &[NodeTag::OBJECT_PATTERN, NodeTag::PROPERTY],
        ),
        GrammarCase::script(
            "class property names",
            "class Properties { [field] = value; [method]() {} return() {} \"text\" = value; 0() {} 1n = value; }",
            &[
                NodeTag::CLASS_DECLARATION,
                NodeTag::PROPERTY_DEFINITION,
                NodeTag::METHOD_DEFINITION,
            ],
        ),
    ];

    assert_clean_cases(&cases);
}

/// Recoverable syntax errors must still return a validated program tape.
#[test]
fn parser_should_recover_with_a_valid_tape() {
    let cases = [
        GrammarCase::script("missing binding", "const = 1; next();", &[]),
        GrammarCase::script("missing condition", "if () { next(); }", &[]),
        GrammarCase::script("missing operand", "value + ; next();", &[]),
        GrammarCase::script("unterminated block", "function run() { return 1;", &[]),
        GrammarCase::script("throw line break", "throw\nvalue;", &[]),
        GrammarCase::script("unterminated literal", "const value = 'text", &[]),
    ];

    assert_diagnostic_cases(&cases, false);
}

/// Static-semantics checks must diagnose invalid programs after the grammar has produced valid nodes.
#[test]
fn parser_should_report_javascript_early_errors() {
    let cases = [
        GrammarCase::module(
            "duplicate lexical binding",
            "let value; let value;",
            &[NodeTag::VARIABLE_DECLARATION],
        ),
        GrammarCase::module(
            "return outside function",
            "return value;",
            &[NodeTag::RETURN_STATEMENT],
        ),
        GrammarCase::module(
            "break outside target",
            "break;",
            &[NodeTag::BREAK_STATEMENT],
        ),
        GrammarCase::module(
            "continue to non-loop label",
            "label: { continue label; }",
            &[NodeTag::LABELED_STATEMENT, NodeTag::CONTINUE_STATEMENT],
        ),
        GrammarCase::module(
            "with in strict code",
            "with (object) value;",
            &[NodeTag::WITH_STATEMENT],
        ),
        GrammarCase::module(
            "duplicate label",
            "label: label: while (true) break label;",
            &[NodeTag::LABELED_STATEMENT],
        ),
        GrammarCase::module(
            "delete identifier in strict code",
            "delete value;",
            &[NodeTag::UNARY_EXPRESSION],
        ),
        GrammarCase::module(
            "duplicate export",
            "const value = 1; export { value }; export { value };",
            &[NodeTag::EXPORT_NAMED_DECLARATION],
        ),
    ];

    assert_diagnostic_cases(&cases, true);
}

fn assert_clean_cases(cases: &[GrammarCase]) {
    let mut failures = Vec::new();
    for &case in cases {
        match parse(case.source, case.options(false)) {
            Err(error) => failures.push(format!("{}: parse failed: {error}", case.name)),
            Ok(parsed) => {
                let tags = match inspect_tape(case.name, &parsed) {
                    Ok(tags) => tags,
                    Err(reason) => {
                        failures.push(reason);
                        continue;
                    }
                };
                if !parsed.diagnostics.is_empty() {
                    failures.push(format!(
                        "{}: diagnostics: {:?}",
                        case.name, parsed.diagnostics
                    ));
                }
                for &tag in case.expected_tags {
                    if !tags.contains(&tag) {
                        failures.push(format!("{}: missing node tag {tag:?}", case.name));
                    }
                }
            }
        }
    }
    assert_failures_empty(&failures);
}

fn assert_diagnostic_cases(cases: &[GrammarCase], semantic_errors: bool) {
    let mut failures = Vec::new();
    for &case in cases {
        match parse(case.source, case.options(semantic_errors)) {
            Err(error) => failures.push(format!(
                "{}: parse failed instead of recovering: {error}",
                case.name
            )),
            Ok(parsed) => {
                let tags = match inspect_tape(case.name, &parsed) {
                    Ok(tags) => tags,
                    Err(reason) => {
                        failures.push(reason);
                        continue;
                    }
                };
                if parsed.diagnostics.is_empty() {
                    failures.push(format!("{}: expected a diagnostic", case.name));
                }
                for &tag in case.expected_tags {
                    if !tags.contains(&tag) {
                        failures.push(format!("{}: missing recovered node tag {tag:?}", case.name));
                    }
                }
            }
        }
    }
    assert_failures_empty(&failures);
}

fn inspect_tape(name: &str, parsed: &ParseResult) -> Result<Vec<NodeTag>, String> {
    parsed
        .tape
        .validate()
        .map_err(|error| format!("{name}: invalid tape: {error}"))?;
    match parsed.tape.value_at(parsed.tape.header().root) {
        Ok(TapeValue::Node {
            tag: NodeTag::PROGRAM,
            ..
        }) => {}
        Ok(value) => return Err(format!("{name}: root is not Program: {value:?}")),
        Err(error) => return Err(format!("{name}: root lookup failed: {error}")),
    }

    let mut tags = Vec::new();
    for record in parsed.tape.validation() {
        let record =
            record.map_err(|error| format!("{name}: record validation failed: {error}"))?;
        if let TapeValue::Node { tag, .. } = record.value {
            tags.push(tag);
        }
    }
    Ok(tags)
}

fn check_expression_shape(case: ExpressionShape) -> Result<(), String> {
    let parsed = parse(
        case.source,
        ParseOptions {
            source_kind: SourceKind::Script,
            ..ParseOptions::default()
        },
    )
    .map_err(|error| format!("parse failed: {error}"))?;
    inspect_tape(case.name, &parsed)?;
    if !parsed.diagnostics.is_empty() {
        return Err(format!("diagnostics: {:?}", parsed.diagnostics));
    }

    let expression = first_expression_offset(&parsed.tape)?;
    let root_fields = node_fields(&parsed.tape, expression, case.root)?;
    let nested = root_fields
        .get(case.nested_field)
        .copied()
        .ok_or_else(|| format!("{:?} lacks field {}", case.root, case.nested_field))?;
    let nested_tag = node_tag(&parsed.tape, nested)?;
    if nested_tag != case.nested {
        return Err(format!(
            "field {} of {:?} is {nested_tag:?}, expected {:?}",
            case.nested_field, case.root, case.nested
        ));
    }
    Ok(())
}

fn first_expression_offset(tape: &FrozenTape) -> Result<u32, String> {
    let program = node_fields(tape, tape.header().root, NodeTag::PROGRAM)?;
    let body_offset = program.first().copied().ok_or("Program lacks body")?;
    let statements = match tape
        .value_at(body_offset)
        .map_err(|error| error.to_string())?
    {
        TapeValue::List { items, .. } => items,
        value => return Err(format!("Program body is not a list: {value:?}")),
    };
    let statement = statements.first().copied().ok_or("Program body is empty")?;
    let statement = node_fields(tape, statement, NodeTag::EXPRESSION_STATEMENT)?;
    statement
        .first()
        .copied()
        .ok_or_else(|| "ExpressionStatement lacks expression".to_owned())
}

fn node_fields(tape: &FrozenTape, offset: u32, expected: NodeTag) -> Result<&[u32], String> {
    match tape.value_at(offset).map_err(|error| error.to_string())? {
        TapeValue::Node { tag, fields, .. } if tag == expected => Ok(fields),
        TapeValue::Node { tag, .. } => Err(format!("node is {tag:?}, expected {expected:?}")),
        value => Err(format!("record is not a node: {value:?}")),
    }
}

fn list_items(tape: &FrozenTape, offset: u32) -> Result<&[u32], String> {
    match tape.value_at(offset).map_err(|error| error.to_string())? {
        TapeValue::List { items, .. } => Ok(items),
        value => Err(format!("record is not a list: {value:?}")),
    }
}

fn node_tag(tape: &FrozenTape, offset: u32) -> Result<NodeTag, String> {
    match tape.value_at(offset).map_err(|error| error.to_string())? {
        TapeValue::Node { tag, .. } => Ok(tag),
        value => Err(format!("record is not a node: {value:?}")),
    }
}

fn assert_failures_empty(failures: &[String]) {
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
