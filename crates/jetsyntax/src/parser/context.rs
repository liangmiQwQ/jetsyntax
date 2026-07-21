#![allow(dead_code)]

use std::collections::HashMap;

use crate::{lexer::TokenKind, tape::Span};

const STRICT: u16 = 1 << 0;
const MODULE: u16 = 1 << 1;
const ASYNC: u16 = 1 << 2;
const GENERATOR: u16 = 1 << 3;
const CLASS: u16 = 1 << 4;
const FUNCTION: u16 = 1 << 5;
const ALLOW_IN: u16 = 1 << 6;
const ALLOW_AWAIT: u16 = 1 << 7;
const ALLOW_YIELD: u16 = 1 << 8;
const AMBIENT: u16 = 1 << 9;
const ACCESSOR: u16 = 1 << 10;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GrammarContext(u16);

impl GrammarContext {
    #[must_use]
    pub(crate) const fn new(module: bool, ambient: bool) -> Self {
        let mut bits = ALLOW_IN;
        if module {
            bits |= MODULE | STRICT | ALLOW_AWAIT;
        }
        if ambient {
            bits |= AMBIENT;
        }
        Self(bits)
    }

    #[must_use]
    pub(crate) const fn strict(self) -> bool {
        self.has(STRICT)
    }

    #[must_use]
    pub(crate) const fn module(self) -> bool {
        self.has(MODULE)
    }

    #[must_use]
    pub(crate) const fn async_function(self) -> bool {
        self.has(ASYNC)
    }

    #[must_use]
    pub(crate) const fn generator(self) -> bool {
        self.has(GENERATOR)
    }

    #[must_use]
    pub(crate) const fn class(self) -> bool {
        self.has(CLASS)
    }

    #[must_use]
    pub(crate) const fn function(self) -> bool {
        self.has(FUNCTION)
    }

    #[must_use]
    pub(crate) const fn allow_in(self) -> bool {
        self.has(ALLOW_IN)
    }

    #[must_use]
    pub(crate) const fn allow_await(self) -> bool {
        self.has(ALLOW_AWAIT)
    }

    #[must_use]
    pub(crate) const fn allow_yield(self) -> bool {
        self.has(ALLOW_YIELD)
    }

    #[must_use]
    pub(crate) const fn ambient(self) -> bool {
        self.has(AMBIENT)
    }

    #[must_use]
    pub(crate) const fn accessor(self) -> bool {
        self.has(ACCESSOR)
    }

    #[must_use]
    pub(crate) const fn with_strict(self, enabled: bool) -> Self {
        self.with(STRICT, enabled)
    }

    #[must_use]
    pub(crate) const fn with_module(self, enabled: bool) -> Self {
        self.with(MODULE, enabled)
    }

    #[must_use]
    pub(crate) const fn with_async_function(self, enabled: bool) -> Self {
        self.with(ASYNC, enabled)
    }

    #[must_use]
    pub(crate) const fn with_generator(self, enabled: bool) -> Self {
        self.with(GENERATOR, enabled)
    }

    #[must_use]
    pub(crate) const fn with_class(self, enabled: bool) -> Self {
        self.with(CLASS, enabled)
    }

    #[must_use]
    pub(crate) const fn with_function(self, enabled: bool) -> Self {
        self.with(FUNCTION, enabled)
    }

    #[must_use]
    pub(crate) const fn with_allow_in(self, enabled: bool) -> Self {
        self.with(ALLOW_IN, enabled)
    }

    #[must_use]
    pub(crate) const fn with_allow_await(self, enabled: bool) -> Self {
        self.with(ALLOW_AWAIT, enabled)
    }

    #[must_use]
    pub(crate) const fn with_allow_yield(self, enabled: bool) -> Self {
        self.with(ALLOW_YIELD, enabled)
    }

    #[must_use]
    pub(crate) const fn with_ambient(self, enabled: bool) -> Self {
        self.with(AMBIENT, enabled)
    }

    #[must_use]
    pub(crate) const fn with_accessor(self, enabled: bool) -> Self {
        self.with(ACCESSOR, enabled)
    }

    const fn has(self, flag: u16) -> bool {
        self.0 & flag != 0
    }

    const fn with(self, flag: u16, enabled: bool) -> Self {
        if enabled {
            Self(self.0 | flag)
        } else {
            Self(self.0 & !flag)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub span: Span,
    pub message: String,
    pub related: Option<Span>,
    pub expected: Option<TokenKind>,
    pub found: Option<TokenKind>,
}

impl Diagnostic {
    #[must_use]
    pub fn error(span: Span, message: impl Into<String>) -> Self {
        Self::new(Severity::Error, span, message)
    }

    #[must_use]
    pub fn warning(span: Span, message: impl Into<String>) -> Self {
        Self::new(Severity::Warning, span, message)
    }

    #[must_use]
    pub const fn with_related(mut self, span: Span) -> Self {
        self.related = Some(span);
        self
    }

    #[must_use]
    pub const fn with_expected(mut self, expected: TokenKind, found: TokenKind) -> Self {
        self.expected = Some(expected);
        self.found = Some(found);
        self
    }

    fn new(severity: Severity, span: Span, message: impl Into<String>) -> Self {
        Self {
            severity,
            span,
            message: message.into(),
            related: None,
            expected: None,
            found: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScopeKind {
    Program,
    Function,
    Block,
    Class,
    Catch,
    Type,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BindingKind {
    Var,
    Function,
    Parameter,
    Lexical,
    Import,
    Type,
}

impl BindingKind {
    const fn is_var_scoped(self) -> bool {
        matches!(self, Self::Var | Self::Function | Self::Parameter)
    }

    const fn is_type(self) -> bool {
        matches!(self, Self::Type)
    }

    const fn can_merge_with(self, other: Self) -> bool {
        matches!(self, Self::Var | Self::Function)
            && matches!(other, Self::Var | Self::Function | Self::Parameter)
            || matches!(self, Self::Parameter) && matches!(other, Self::Var | Self::Function)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LabelKind {
    Statement,
    Loop,
    Switch,
}

impl LabelKind {
    const fn supports_break(self) -> bool {
        matches!(self, Self::Loop | Self::Switch)
    }

    const fn supports_continue(self) -> bool {
        matches!(self, Self::Loop)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContextCheckpoint {
    mutation_len: usize,
    diagnostic_len: usize,
    checkpoint_depth: usize,
    grammar: GrammarContext,
}

#[derive(Clone, Copy, Debug)]
struct Binding {
    kind: BindingKind,
    span: Span,
}

#[derive(Debug)]
struct Scope<'s> {
    kind: ScopeKind,
    value_bindings: HashMap<&'s str, Binding>,
    type_bindings: HashMap<&'s str, Binding>,
    private_names: HashMap<&'s str, Span>,
    private_uses: Vec<(&'s str, Span)>,
}

impl Scope<'_> {
    fn new(kind: ScopeKind) -> Self {
        Self {
            kind,
            value_bindings: HashMap::new(),
            type_bindings: HashMap::new(),
            private_names: HashMap::new(),
            private_uses: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Label<'s> {
    name: Option<&'s str>,
    kind: LabelKind,
    span: Span,
    function_depth: usize,
}

#[derive(Clone, Copy, Debug)]
enum BindingNamespace {
    Value,
    Type,
}

#[derive(Debug)]
enum Mutation<'s> {
    ScopePushed,
    ScopePopped(Scope<'s>),
    BindingInserted {
        scope: usize,
        namespace: BindingNamespace,
        name: &'s str,
    },
    PrivateDeclared {
        scope: usize,
        name: &'s str,
    },
    PrivateUsed {
        scope: usize,
    },
    LabelPushed,
    LabelPopped(Label<'s>),
    ExportDeclared(&'s str),
}

#[derive(Debug)]
pub struct ParserContext<'s> {
    grammar: GrammarContext,
    scopes: Vec<Scope<'s>>,
    labels: Vec<Label<'s>>,
    exports: HashMap<&'s str, Span>,
    diagnostics: Vec<Diagnostic>,
    mutations: Vec<Mutation<'s>>,
    checkpoints: Vec<usize>,
}

impl<'s> ParserContext<'s> {
    #[must_use]
    pub(crate) fn new(grammar: GrammarContext) -> Self {
        Self {
            grammar,
            scopes: Vec::new(),
            labels: Vec::new(),
            exports: HashMap::new(),
            diagnostics: Vec::new(),
            mutations: Vec::new(),
            checkpoints: Vec::new(),
        }
    }

    #[must_use]
    pub(crate) const fn grammar(&self) -> GrammarContext {
        self.grammar
    }

    pub(crate) const fn set_grammar(&mut self, grammar: GrammarContext) {
        self.grammar = grammar;
    }

    pub(crate) fn enter_scope(&mut self, kind: ScopeKind) {
        self.scopes.push(Scope::new(kind));
        self.record(Mutation::ScopePushed);
    }

    pub(crate) fn leave_scope(&mut self) -> Option<ScopeKind> {
        let scope = self.scopes.pop()?;
        if scope.kind == ScopeKind::Class {
            self.resolve_private_uses(&scope);
        }
        let kind = scope.kind;
        self.record(Mutation::ScopePopped(scope));
        Some(kind)
    }

    pub(crate) fn declare_binding(&mut self, name: &'s str, kind: BindingKind, span: Span) -> bool {
        if self.scopes.is_empty() {
            self.error(span, "binding declaration requires an active scope");
            return false;
        }
        let target = if kind.is_var_scoped() {
            self.scopes
                .iter()
                .rposition(|scope| matches!(scope.kind, ScopeKind::Program | ScopeKind::Function))
                .unwrap_or(0)
        } else {
            self.scopes.len() - 1
        };
        let namespace = if kind.is_type() {
            BindingNamespace::Type
        } else {
            BindingNamespace::Value
        };

        let conflict = if kind.is_type() {
            self.scopes[target].type_bindings.get(name).copied()
        } else if kind.is_var_scoped() {
            self.scopes[target..]
                .iter()
                .enumerate()
                .find_map(|(offset, scope)| {
                    let binding = scope.value_bindings.get(name).copied()?;
                    if offset == 0 && kind.can_merge_with(binding.kind) {
                        None
                    } else {
                        Some(binding)
                    }
                })
        } else {
            self.scopes[target].value_bindings.get(name).copied()
        };
        if let Some(previous) = conflict {
            self.push_diagnostic(
                Diagnostic::error(span, format!("duplicate binding `{name}`"))
                    .with_related(previous.span),
            );
            return false;
        }

        let bindings = match namespace {
            BindingNamespace::Value => &mut self.scopes[target].value_bindings,
            BindingNamespace::Type => &mut self.scopes[target].type_bindings,
        };
        if bindings.contains_key(name) {
            return true;
        }
        bindings.insert(name, Binding { kind, span });
        self.record(Mutation::BindingInserted {
            scope: target,
            namespace,
            name,
        });
        true
    }

    pub(crate) fn current_restricted_parameter_binding(&self) -> Option<Span> {
        let scope = self.scopes.last()?;
        ["eval", "arguments"].into_iter().find_map(|name| {
            let binding = scope.value_bindings.get(name)?;
            (binding.kind == BindingKind::Parameter).then_some(binding.span)
        })
    }

    pub(crate) fn declare_private(&mut self, name: &'s str, span: Span) -> bool {
        let Some(scope) = self.class_scope() else {
            self.error(span, "private name is only valid inside a class");
            return false;
        };
        if let Some(previous) = self.scopes[scope].private_names.get(name).copied() {
            self.push_diagnostic(
                Diagnostic::error(span, format!("duplicate private name `{name}`"))
                    .with_related(previous),
            );
            return false;
        }
        self.scopes[scope].private_names.insert(name, span);
        self.record(Mutation::PrivateDeclared { scope, name });
        true
    }

    pub(crate) fn use_private(&mut self, name: &'s str, span: Span) -> bool {
        let Some(scope) = self.class_scope() else {
            self.error(span, "private name is only valid inside a class");
            return false;
        };
        if self.scopes[..=scope]
            .iter()
            .rev()
            .any(|scope| scope.private_names.contains_key(name))
        {
            return true;
        }
        self.scopes[scope].private_uses.push((name, span));
        self.record(Mutation::PrivateUsed { scope });
        true
    }

    pub(crate) fn push_label(
        &mut self,
        name: Option<&'s str>,
        kind: LabelKind,
        span: Span,
    ) -> bool {
        let function_depth = self.function_depth();
        let duplicate = name.and_then(|name| {
            self.labels
                .iter()
                .rev()
                .take_while(|label| label.function_depth == function_depth)
                .find(|label| label.name == Some(name))
                .copied()
        });
        if let Some(previous) = duplicate {
            self.push_diagnostic(
                Diagnostic::error(
                    span,
                    format!("duplicate label `{}`", name.unwrap_or_default()),
                )
                .with_related(previous.span),
            );
        }
        self.labels.push(Label {
            name,
            kind,
            span,
            function_depth,
        });
        self.record(Mutation::LabelPushed);
        duplicate.is_none()
    }

    pub(crate) fn pop_label(&mut self) -> Option<LabelKind> {
        let label = self.labels.pop()?;
        let kind = label.kind;
        self.record(Mutation::LabelPopped(label));
        Some(kind)
    }

    #[must_use]
    pub(crate) fn resolve_break(&self, name: Option<&str>) -> bool {
        self.resolve_label(name, LabelKind::supports_break)
    }

    #[must_use]
    pub(crate) fn resolve_continue(&self, name: Option<&str>) -> bool {
        self.resolve_label(name, LabelKind::supports_continue)
    }

    pub(crate) fn declare_export(&mut self, name: &'s str, span: Span) -> bool {
        if let Some(previous) = self.exports.get(name).copied() {
            self.push_diagnostic(
                Diagnostic::error(span, format!("duplicate export `{name}`"))
                    .with_related(previous),
            );
            return false;
        }
        self.exports.insert(name, span);
        self.record(Mutation::ExportDeclared(name));
        true
    }

    pub(crate) fn checkpoint(&mut self) -> ContextCheckpoint {
        let checkpoint = ContextCheckpoint {
            mutation_len: self.mutations.len(),
            diagnostic_len: self.diagnostics.len(),
            checkpoint_depth: self.checkpoints.len() + 1,
            grammar: self.grammar,
        };
        self.checkpoints.push(checkpoint.mutation_len);
        checkpoint
    }

    pub(crate) fn rollback(&mut self, checkpoint: ContextCheckpoint) {
        if !self.is_current_checkpoint(checkpoint) {
            return;
        }
        while self.mutations.len() > checkpoint.mutation_len {
            let Some(mutation) = self.mutations.pop() else {
                break;
            };
            self.undo(mutation);
        }
        self.grammar = checkpoint.grammar;
        self.diagnostics.truncate(checkpoint.diagnostic_len);
        self.checkpoints.pop();
        if self.checkpoints.is_empty() {
            self.mutations.clear();
        }
    }

    pub(crate) fn commit(&mut self, checkpoint: ContextCheckpoint) {
        if !self.is_current_checkpoint(checkpoint) {
            return;
        }
        self.checkpoints.pop();
        if self.checkpoints.is_empty() {
            self.mutations.clear();
        }
    }

    pub(crate) fn push_diagnostic(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    pub(crate) fn error(&mut self, span: Span, message: impl Into<String>) {
        self.push_diagnostic(Diagnostic::error(span, message));
    }

    pub(crate) fn warning(&mut self, span: Span, message: impl Into<String>) {
        self.push_diagnostic(Diagnostic::warning(span, message));
    }

    #[must_use]
    pub(crate) fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    pub(crate) fn take_diagnostics(&mut self) -> Vec<Diagnostic> {
        std::mem::take(&mut self.diagnostics)
    }

    fn class_scope(&self) -> Option<usize> {
        self.scopes
            .iter()
            .rposition(|scope| scope.kind == ScopeKind::Class)
    }

    fn function_depth(&self) -> usize {
        self.scopes
            .iter()
            .filter(|scope| scope.kind == ScopeKind::Function)
            .count()
    }

    fn resolve_label(&self, name: Option<&str>, accepts: fn(LabelKind) -> bool) -> bool {
        let function_depth = self.function_depth();
        self.labels
            .iter()
            .rev()
            .take_while(|label| label.function_depth == function_depth)
            .any(|label| {
                name.map_or_else(
                    || accepts(label.kind),
                    |name| label.name == Some(name) && accepts(label.kind),
                )
            })
    }

    fn resolve_private_uses(&mut self, scope: &Scope<'s>) {
        for &(name, span) in &scope.private_uses {
            if scope.private_names.contains_key(name)
                || self
                    .scopes
                    .iter()
                    .rev()
                    .any(|scope| scope.private_names.contains_key(name))
            {
                continue;
            }
            if let Some(outer) = self.class_scope() {
                self.scopes[outer].private_uses.push((name, span));
                self.record(Mutation::PrivateUsed { scope: outer });
            } else {
                self.error(span, format!("private name `{name}` is not declared"));
            }
        }
    }

    fn record(&mut self, mutation: Mutation<'s>) {
        if !self.checkpoints.is_empty() {
            self.mutations.push(mutation);
        }
    }

    fn is_current_checkpoint(&self, checkpoint: ContextCheckpoint) -> bool {
        let valid = checkpoint.checkpoint_depth == self.checkpoints.len()
            && self.checkpoints.last() == Some(&checkpoint.mutation_len);
        debug_assert!(valid, "context checkpoints must be resolved in LIFO order");
        valid
    }

    fn undo(&mut self, mutation: Mutation<'s>) {
        match mutation {
            Mutation::ScopePushed => {
                self.scopes.pop();
            }
            Mutation::ScopePopped(scope) => self.scopes.push(scope),
            Mutation::BindingInserted {
                scope,
                namespace,
                name,
            } => {
                let bindings = match namespace {
                    BindingNamespace::Value => &mut self.scopes[scope].value_bindings,
                    BindingNamespace::Type => &mut self.scopes[scope].type_bindings,
                };
                bindings.remove(name);
            }
            Mutation::PrivateDeclared { scope, name } => {
                self.scopes[scope].private_names.remove(name);
            }
            Mutation::PrivateUsed { scope } => {
                self.scopes[scope].private_uses.pop();
            }
            Mutation::LabelPushed => {
                self.labels.pop();
            }
            Mutation::LabelPopped(label) => self.labels.push(label),
            Mutation::ExportDeclared(name) => {
                self.exports.remove(name);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BindingKind, GrammarContext, LabelKind, ParserContext, ScopeKind, Severity};
    use crate::tape::Span;

    #[test]
    fn grammar_context_keeps_flags_in_one_word() {
        let grammar = GrammarContext::new(true, false)
            .with_async_function(true)
            .with_generator(true)
            .with_class(true)
            .with_function(true)
            .with_allow_in(false)
            .with_allow_yield(true)
            .with_ambient(true);

        assert_eq!(std::mem::size_of::<GrammarContext>(), 2);
        assert!(grammar.strict());
        assert!(grammar.module());
        assert!(grammar.async_function());
        assert!(grammar.generator());
        assert!(grammar.class());
        assert!(grammar.function());
        assert!(!grammar.allow_in());
        assert!(grammar.allow_await());
        assert!(grammar.allow_yield());
        assert!(grammar.ambient());
    }

    #[test]
    fn duplicate_declarations_retain_the_original_span() {
        let mut context = ParserContext::new(GrammarContext::default());
        context.enter_scope(ScopeKind::Program);
        assert!(context.declare_binding("value", BindingKind::Lexical, Span::new(0, 5)));
        assert!(!context.declare_binding("value", BindingKind::Lexical, Span::new(8, 13)));
        assert!(context.declare_export("value", Span::new(15, 20)));
        assert!(!context.declare_export("value", Span::new(22, 27)));

        assert_eq!(context.diagnostics().len(), 2);
        assert_eq!(context.diagnostics()[0].related, Some(Span::new(0, 5)));
        assert_eq!(context.diagnostics()[1].related, Some(Span::new(15, 20)));
    }

    #[test]
    fn labels_resolve_only_inside_the_current_function() {
        let mut context = ParserContext::new(GrammarContext::default());
        context.enter_scope(ScopeKind::Program);
        assert!(context.push_label(Some("outer"), LabelKind::Loop, Span::new(0, 5)));
        assert!(context.resolve_break(None));
        assert!(context.resolve_continue(Some("outer")));

        context.enter_scope(ScopeKind::Function);
        assert!(!context.resolve_break(None));
        assert!(!context.resolve_continue(Some("outer")));
        assert!(context.push_label(None, LabelKind::Switch, Span::new(8, 14)));
        assert!(context.resolve_break(None));
        assert!(!context.resolve_continue(None));
    }

    #[test]
    fn class_private_uses_resolve_after_their_declaration() {
        let mut context = ParserContext::new(GrammarContext::default());
        context.enter_scope(ScopeKind::Program);
        context.enter_scope(ScopeKind::Class);
        assert!(context.use_private("#value", Span::new(4, 10)));
        assert!(context.declare_private("#value", Span::new(12, 18)));
        assert_eq!(context.leave_scope(), Some(ScopeKind::Class));
        assert!(context.diagnostics().is_empty());

        context.enter_scope(ScopeKind::Class);
        assert!(context.use_private("#missing", Span::new(20, 28)));
        context.leave_scope();
        assert_eq!(context.diagnostics()[0].severity, Severity::Error);
    }

    #[test]
    fn rollback_restores_all_speculative_state() {
        let mut context = ParserContext::new(GrammarContext::default());
        context.enter_scope(ScopeKind::Program);
        let checkpoint = context.checkpoint();
        context.set_grammar(context.grammar().with_async_function(true));
        context.enter_scope(ScopeKind::Class);
        assert!(context.declare_binding("branch", BindingKind::Lexical, Span::new(0, 6)));
        assert!(context.declare_private("#branch", Span::new(7, 14)));
        assert!(context.declare_export("branch", Span::new(15, 21)));
        assert!(context.push_label(Some("branch"), LabelKind::Statement, Span::new(22, 28)));
        context.error(Span::new(29, 30), "branch diagnostic");
        context.rollback(checkpoint);

        assert!(!context.grammar().async_function());
        assert!(context.diagnostics().is_empty());
        assert!(context.declare_export("branch", Span::new(31, 37)));
        assert!(context.declare_binding("branch", BindingKind::Lexical, Span::new(38, 44)));
        assert!(!context.resolve_break(Some("branch")));
        assert_eq!(context.leave_scope(), Some(ScopeKind::Program));
        assert_eq!(context.leave_scope(), None);
    }
}
