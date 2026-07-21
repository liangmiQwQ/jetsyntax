//! `JetSyntax`'s compact, append-only AST transfer format.
//!
//! The tape is a sequence of stable `u32` words. It deliberately does not mirror Rust structs:
//! native and JavaScript decoders consume the same schema-ordered values without depending on
//! compiler layout, pointer width, or allocator behavior.
//!
//! # Postfix layout
//!
//! Every value is completed before its parent and is addressed by its word offset. A parser can
//! therefore emit a Pratt-parser left-hand side immediately, then append a binary/member/call node
//! that refers back to it. No AST buffering or record reordering is required.
//!
//! # Wire invariants
//!
//! - The first [`HEADER_WORDS`] words are a versioned header.
//! - All node and list references point backward to the start of a completed record.
//! - The root is the final record, is a node, and is the only unreferenced record.
//! - Every non-root record is referenced exactly once, so a valid tape is one `ESTree` tree.
//! - Nodes and lists carry an exact word length and an immediate reference count.
//! - Spans and source slices use UTF-8 byte offsets into the caller-owned source.
//! - Decoded strings use byte offsets into the tape's packed UTF-8 string pool.
//! - Wire bytes are little-endian, regardless of host endianness.
//! - Node field order is defined by `JetSyntax`'s decoder schema, not encoded field names.

use std::{
    error::Error,
    fmt,
    sync::atomic::{AtomicU64, Ordering},
};

pub const MAGIC: u32 = 0x4A53_5450;
pub const FORMAT_VERSION: u32 = 1;
pub const HEADER_WORDS: usize = 12;

const FLAG_SOURCE_UTF8: u32 = 1 << 0;
const FLAG_POOL_UTF8: u32 = 1 << 1;
const WIRE_FLAGS: u32 = FLAG_SOURCE_UTF8 | FLAG_POOL_UTF8;

const HEADER_MAGIC: usize = 0;
const HEADER_VERSION: usize = 1;
const HEADER_SIZE: usize = 2;
const HEADER_FLAGS: usize = 3;
const HEADER_TOTAL_WORDS: usize = 4;
const HEADER_RECORD_END: usize = 5;
const HEADER_POOL_BYTES: usize = 6;
const HEADER_ROOT: usize = 7;
const HEADER_SOURCE_BYTES: usize = 8;
const HEADER_NODE_COUNT: usize = 9;
const HEADER_VALUE_COUNT: usize = 10;
const HEADER_RESERVED: usize = 11;

const KIND_SHIFT: u32 = 28;
const KIND_MASK: u32 = 0xF000_0000;
const NODE_FLAGS_SHIFT: u32 = 16;
const NODE_FLAGS_MASK: u32 = 0x00FF_0000;
const NODE_TAG_MASK: u32 = 0x0000_FFFF;
const INLINE_U32_MASK: u32 = 0x0FFF_FFFF;

const KIND_NODE: u32 = 1;
const KIND_LIST: u32 = 2;
const KIND_NULL: u32 = 3;
const KIND_BOOL: u32 = 4;
const KIND_INLINE_U32: u32 = 5;
const KIND_U32: u32 = 6;
const KIND_F64: u32 = 7;
const KIND_SOURCE_SLICE: u32 = 8;
const KIND_POOL_STRING: u32 = 9;

static NEXT_BUILDER_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    #[must_use]
    pub const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }
}

/// Stable `JetSyntax` node identifier. Existing values must never be renumbered.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct NodeTag(u16);

impl NodeTag {
    pub const PROGRAM: Self = Self(1);
    pub const IDENTIFIER: Self = Self(2);
    pub const PRIVATE_IDENTIFIER: Self = Self(3);
    pub const LITERAL: Self = Self(4);
    pub const EXPRESSION_STATEMENT: Self = Self(5);
    pub const BLOCK_STATEMENT: Self = Self(6);
    pub const EMPTY_STATEMENT: Self = Self(7);
    pub const DEBUGGER_STATEMENT: Self = Self(8);
    pub const WITH_STATEMENT: Self = Self(9);
    pub const RETURN_STATEMENT: Self = Self(10);
    pub const LABELED_STATEMENT: Self = Self(11);
    pub const BREAK_STATEMENT: Self = Self(12);
    pub const CONTINUE_STATEMENT: Self = Self(13);
    pub const IF_STATEMENT: Self = Self(14);
    pub const SWITCH_STATEMENT: Self = Self(15);
    pub const SWITCH_CASE: Self = Self(16);
    pub const THROW_STATEMENT: Self = Self(17);
    pub const TRY_STATEMENT: Self = Self(18);
    pub const CATCH_CLAUSE: Self = Self(19);
    pub const WHILE_STATEMENT: Self = Self(20);
    pub const DO_WHILE_STATEMENT: Self = Self(21);
    pub const FOR_STATEMENT: Self = Self(22);
    pub const FOR_IN_STATEMENT: Self = Self(23);
    pub const FOR_OF_STATEMENT: Self = Self(24);
    pub const FUNCTION_DECLARATION: Self = Self(25);
    pub const FUNCTION_EXPRESSION: Self = Self(26);
    pub const ARROW_FUNCTION_EXPRESSION: Self = Self(27);
    pub const VARIABLE_DECLARATION: Self = Self(28);
    pub const VARIABLE_DECLARATOR: Self = Self(29);
    pub const THIS_EXPRESSION: Self = Self(30);
    pub const ARRAY_EXPRESSION: Self = Self(31);
    pub const OBJECT_EXPRESSION: Self = Self(32);
    pub const PROPERTY: Self = Self(33);
    pub const SEQUENCE_EXPRESSION: Self = Self(34);
    pub const UNARY_EXPRESSION: Self = Self(35);
    pub const UPDATE_EXPRESSION: Self = Self(36);
    pub const BINARY_EXPRESSION: Self = Self(37);
    pub const LOGICAL_EXPRESSION: Self = Self(38);
    pub const ASSIGNMENT_EXPRESSION: Self = Self(39);
    pub const ASSIGNMENT_PATTERN: Self = Self(40);
    pub const CONDITIONAL_EXPRESSION: Self = Self(41);
    pub const NEW_EXPRESSION: Self = Self(42);
    pub const CALL_EXPRESSION: Self = Self(43);
    pub const MEMBER_EXPRESSION: Self = Self(44);
    pub const CHAIN_EXPRESSION: Self = Self(45);
    pub const YIELD_EXPRESSION: Self = Self(46);
    pub const AWAIT_EXPRESSION: Self = Self(47);
    pub const TEMPLATE_LITERAL: Self = Self(48);
    pub const TEMPLATE_ELEMENT: Self = Self(49);
    pub const TAGGED_TEMPLATE_EXPRESSION: Self = Self(50);
    pub const SPREAD_ELEMENT: Self = Self(51);
    pub const REST_ELEMENT: Self = Self(52);
    pub const ARRAY_PATTERN: Self = Self(53);
    pub const OBJECT_PATTERN: Self = Self(54);
    pub const META_PROPERTY: Self = Self(55);
    pub const IMPORT_EXPRESSION: Self = Self(56);
    pub const CLASS_DECLARATION: Self = Self(57);
    pub const CLASS_EXPRESSION: Self = Self(58);
    pub const CLASS_BODY: Self = Self(59);
    pub const METHOD_DEFINITION: Self = Self(60);
    pub const PROPERTY_DEFINITION: Self = Self(61);
    pub const STATIC_BLOCK: Self = Self(62);
    pub const IMPORT_DECLARATION: Self = Self(63);
    pub const IMPORT_SPECIFIER: Self = Self(64);
    pub const IMPORT_DEFAULT_SPECIFIER: Self = Self(65);
    pub const IMPORT_NAMESPACE_SPECIFIER: Self = Self(66);
    pub const EXPORT_NAMED_DECLARATION: Self = Self(67);
    pub const EXPORT_DEFAULT_DECLARATION: Self = Self(68);
    pub const EXPORT_ALL_DECLARATION: Self = Self(69);
    pub const EXPORT_SPECIFIER: Self = Self(70);
    pub const SUPER: Self = Self(71);
    pub const PARENTHESIZED_EXPRESSION: Self = Self(72);
    pub const IMPORT_ATTRIBUTE: Self = Self(73);

    pub const JSX_IDENTIFIER: Self = Self(256);
    pub const JSX_MEMBER_EXPRESSION: Self = Self(257);
    pub const JSX_NAMESPACED_NAME: Self = Self(258);
    pub const JSX_ELEMENT: Self = Self(259);
    pub const JSX_FRAGMENT: Self = Self(260);
    pub const JSX_OPENING_ELEMENT: Self = Self(261);
    pub const JSX_CLOSING_ELEMENT: Self = Self(262);
    pub const JSX_OPENING_FRAGMENT: Self = Self(263);
    pub const JSX_CLOSING_FRAGMENT: Self = Self(264);
    pub const JSX_ATTRIBUTE: Self = Self(265);
    pub const JSX_SPREAD_ATTRIBUTE: Self = Self(266);
    pub const JSX_EXPRESSION_CONTAINER: Self = Self(267);
    pub const JSX_EMPTY_EXPRESSION: Self = Self(268);
    pub const JSX_TEXT: Self = Self(269);
    pub const JSX_SPREAD_CHILD: Self = Self(270);

    // TypeScript tags occupy 512..=4095. New tags are appended within that range.
    pub const TS_TYPE_ANNOTATION: Self = Self(512);
    pub const TS_TYPE_REFERENCE: Self = Self(513);
    pub const TS_QUALIFIED_NAME: Self = Self(514);
    pub const TS_UNION_TYPE: Self = Self(515);
    pub const TS_INTERSECTION_TYPE: Self = Self(516);
    pub const TS_LITERAL_TYPE: Self = Self(517);
    pub const TS_ARRAY_TYPE: Self = Self(518);
    pub const TS_TUPLE_TYPE: Self = Self(519);
    pub const TS_FUNCTION_TYPE: Self = Self(520);
    pub const TS_CONDITIONAL_TYPE: Self = Self(521);
    pub const TS_MAPPED_TYPE: Self = Self(522);
    pub const TS_TYPE_LITERAL: Self = Self(523);
    pub const TS_INTERFACE_DECLARATION: Self = Self(524);
    pub const TS_TYPE_ALIAS_DECLARATION: Self = Self(525);
    pub const TS_ENUM_DECLARATION: Self = Self(526);
    pub const TS_MODULE_DECLARATION: Self = Self(527);
    pub const TS_AS_EXPRESSION: Self = Self(528);
    pub const TS_SATISFIES_EXPRESSION: Self = Self(529);
    pub const TS_NON_NULL_EXPRESSION: Self = Self(530);
    pub const TS_PARENTHESIZED_TYPE: Self = Self(531);
    pub const TS_INDEXED_ACCESS_TYPE: Self = Self(532);
    pub const TS_TYPE_OPERATOR: Self = Self(533);
    pub const TS_TYPE_PARAMETER: Self = Self(534);
    pub const TS_PROPERTY_SIGNATURE: Self = Self(535);
    pub const TS_METHOD_SIGNATURE: Self = Self(536);
    pub const TS_ENUM_MEMBER: Self = Self(537);
    pub const TS_NAMED_TUPLE_MEMBER: Self = Self(538);
    pub const TS_INTERFACE_BODY: Self = Self(539);
    pub const TS_MODULE_BLOCK: Self = Self(540);
    pub const TS_TYPE_PARAMETER_DECLARATION: Self = Self(541);
    pub const TS_TYPE_PARAMETER_INSTANTIATION: Self = Self(542);
    pub const TS_ANY_KEYWORD: Self = Self(543);
    pub const TS_BIGINT_KEYWORD: Self = Self(544);
    pub const TS_BOOLEAN_KEYWORD: Self = Self(545);
    pub const TS_INTRINSIC_KEYWORD: Self = Self(546);
    pub const TS_NEVER_KEYWORD: Self = Self(547);
    pub const TS_NUMBER_KEYWORD: Self = Self(548);
    pub const TS_OBJECT_KEYWORD: Self = Self(549);
    pub const TS_STRING_KEYWORD: Self = Self(550);
    pub const TS_SYMBOL_KEYWORD: Self = Self(551);
    pub const TS_THIS_TYPE: Self = Self(552);
    pub const TS_UNDEFINED_KEYWORD: Self = Self(553);
    pub const TS_UNKNOWN_KEYWORD: Self = Self(554);
    pub const TS_VOID_KEYWORD: Self = Self(555);
    pub const TS_INFER_TYPE: Self = Self(556);
    pub const TS_ENUM_BODY: Self = Self(557);
    pub const TS_INTERFACE_HERITAGE: Self = Self(558);
    pub const TS_NULL_KEYWORD: Self = Self(559);
    pub const TS_TYPE_ASSERTION: Self = Self(560);

    #[must_use]
    pub const fn new(value: u16) -> Option<Self> {
        if value == 0 { None } else { Some(Self(value)) }
    }

    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// A builder-local handle. Only [`ValueRef::offset`] is serialized.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ValueRef {
    builder_id: u64,
    record_id: u64,
    offset: u32,
}

impl ValueRef {
    #[must_use]
    pub const fn offset(self) -> u32 {
        self.offset
    }
}

/// A builder-local handle that is known to refer to a node record.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct NodeRef(ValueRef);

impl NodeRef {
    #[must_use]
    pub const fn offset(self) -> u32 {
        self.0.offset
    }

    #[must_use]
    pub const fn as_value(self) -> ValueRef {
        self.0
    }
}

impl From<NodeRef> for ValueRef {
    fn from(node: NodeRef) -> Self {
        node.as_value()
    }
}

#[derive(Clone, Copy, Debug)]
struct BuilderRecord {
    id: u64,
    offset: u32,
    incoming: u32,
}

/// A speculative parse snapshot. Record identities make stale branch handles detectable.
#[derive(Clone, Copy, Debug)]
pub struct Checkpoint {
    builder_id: u64,
    words_len: usize,
    pool_len: usize,
    records_len: usize,
    last_record_id: Option<u64>,
    node_count: u32,
}

#[derive(Debug)]
pub enum TapeError {
    TooLarge,
    InvalidTag,
    InvalidSpan(Span),
    SourceRange(Span),
    ForeignReference,
    ForeignCheckpoint,
    RootMustBeFinalNode,
    InvalidRecordOffset(u32),
    ZeroCopyPoolUnsupported,
    InvalidHeader(&'static str),
    UnsupportedVersion(u32),
    MalformedRecord { offset: u32, reason: &'static str },
}

impl fmt::Display for TapeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => formatter.write_str("tape exceeds the u32 wire format"),
            Self::InvalidTag => formatter.write_str("node tag zero is reserved"),
            Self::InvalidSpan(span) => {
                write!(formatter, "invalid span {}..{}", span.start, span.end)
            }
            Self::SourceRange(span) => {
                write!(
                    formatter,
                    "source range {}..{} is out of bounds",
                    span.start, span.end
                )
            }
            Self::ForeignReference => {
                formatter.write_str("value reference is stale or belongs to another tape builder")
            }
            Self::ForeignCheckpoint => {
                formatter.write_str("checkpoint is stale or belongs to another tape branch")
            }
            Self::RootMustBeFinalNode => {
                formatter.write_str("the tape root must be its final completed node")
            }
            Self::InvalidRecordOffset(offset) => {
                write!(formatter, "word {offset} is not the start of a tape record")
            }
            Self::ZeroCopyPoolUnsupported => {
                formatter.write_str("zero-copy string-pool views require a little-endian host")
            }
            Self::InvalidHeader(reason) => write!(formatter, "invalid tape header: {reason}"),
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported tape version {version}")
            }
            Self::MalformedRecord { offset, reason } => {
                write!(formatter, "malformed record at word {offset}: {reason}")
            }
        }
    }
}

impl Error for TapeError {}

/// Mutable postfix tape writer used directly by the parser.
#[derive(Debug)]
pub struct TapeBuilder {
    id: u64,
    next_record_id: u64,
    source_bytes: u32,
    words: Vec<u32>,
    string_pool: Vec<u8>,
    records: Vec<BuilderRecord>,
    node_count: u32,
}

impl TapeBuilder {
    #[must_use]
    pub fn new(source_bytes: u32) -> Self {
        Self {
            id: NEXT_BUILDER_ID.fetch_add(1, Ordering::Relaxed),
            next_record_id: 1,
            source_bytes,
            words: vec![0; HEADER_WORDS],
            string_pool: Vec::new(),
            records: Vec::new(),
            node_count: 0,
        }
    }

    #[must_use]
    pub fn checkpoint(&self) -> Checkpoint {
        Checkpoint {
            builder_id: self.id,
            words_len: self.words.len(),
            pool_len: self.string_pool.len(),
            records_len: self.records.len(),
            last_record_id: self.records.last().map(|record| record.id),
            node_count: self.node_count,
        }
    }

    /// Rolls the writer back to a snapshot on its current speculative branch.
    ///
    /// # Errors
    ///
    /// Returns [`TapeError::ForeignCheckpoint`] for a stale or foreign snapshot.
    pub fn rollback(&mut self, checkpoint: Checkpoint) -> Result<(), TapeError> {
        let branch_matches = match checkpoint.last_record_id {
            Some(id) => self
                .records
                .get(checkpoint.records_len.saturating_sub(1))
                .is_some_and(|record| record.id == id),
            None => checkpoint.records_len == 0,
        };
        if checkpoint.builder_id != self.id
            || checkpoint.words_len > self.words.len()
            || checkpoint.pool_len > self.string_pool.len()
            || checkpoint.records_len > self.records.len()
            || !branch_matches
        {
            return Err(TapeError::ForeignCheckpoint);
        }
        self.unmark_discarded_references(checkpoint.records_len)?;
        self.words.truncate(checkpoint.words_len);
        self.string_pool.truncate(checkpoint.pool_len);
        self.records.truncate(checkpoint.records_len);
        self.node_count = checkpoint.node_count;
        Ok(())
    }

    /// Appends a null value.
    ///
    /// # Errors
    ///
    /// Returns [`TapeError::TooLarge`] if the tape exceeds its wire limits.
    pub fn push_null(&mut self) -> Result<ValueRef, TapeError> {
        self.push_record(&[KIND_NULL << KIND_SHIFT])
    }

    /// Appends a Boolean value.
    ///
    /// # Errors
    ///
    /// Returns [`TapeError::TooLarge`] if the tape exceeds its wire limits.
    pub fn push_bool(&mut self, value: bool) -> Result<ValueRef, TapeError> {
        self.push_record(&[(KIND_BOOL << KIND_SHIFT) | u32::from(value)])
    }

    /// Appends an unsigned integer value.
    ///
    /// # Errors
    ///
    /// Returns [`TapeError::TooLarge`] if the tape exceeds its wire limits.
    pub fn push_u32(&mut self, value: u32) -> Result<ValueRef, TapeError> {
        if value <= INLINE_U32_MASK {
            self.push_record(&[(KIND_INLINE_U32 << KIND_SHIFT) | value])
        } else {
            self.push_record(&[KIND_U32 << KIND_SHIFT, value])
        }
    }

    /// Appends an IEEE-754 number without canonicalizing its bits.
    ///
    /// # Errors
    ///
    /// Returns [`TapeError::TooLarge`] if the tape exceeds its wire limits.
    pub fn push_f64(&mut self, value: f64) -> Result<ValueRef, TapeError> {
        let bytes = value.to_bits().to_le_bytes();
        let low = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let high = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        self.push_record(&[KIND_F64 << KIND_SHIFT, low, high])
    }

    /// Appends a byte slice into the caller-owned source.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid source span or an oversized tape.
    pub fn push_source_slice(&mut self, span: Span) -> Result<ValueRef, TapeError> {
        self.validate_span(span)?;
        self.push_record(&[KIND_SOURCE_SLICE << KIND_SHIFT, span.start, span.end])
    }

    /// Appends an owned UTF-8 string to the packed pool.
    ///
    /// # Errors
    ///
    /// Returns [`TapeError::TooLarge`] if the tape or pool exceeds its wire limits.
    pub fn push_string(&mut self, value: &str) -> Result<ValueRef, TapeError> {
        let start = to_u32(self.string_pool.len())?;
        let len = to_u32(value.len())?;
        let _ = start.checked_add(len).ok_or(TapeError::TooLarge)?;
        let value_ref = self.push_record(&[KIND_POOL_STRING << KIND_SHIFT, start, len])?;
        self.string_pool.extend_from_slice(value.as_bytes());
        Ok(value_ref)
    }

    /// Appends a list of previously completed values.
    ///
    /// # Errors
    ///
    /// Returns an error for a stale/foreign reference or an oversized tape.
    pub fn push_list(&mut self, items: &[ValueRef]) -> Result<ValueRef, TapeError> {
        let words = 3_usize
            .checked_add(items.len())
            .ok_or(TapeError::TooLarge)?;
        let words_u32 = to_u32(words)?;
        let items_u32 = to_u32(items.len())?;
        self.ensure_record_capacity(words)?;
        self.mark_references(items)?;
        let record_id = match self.take_record_id() {
            Ok(record_id) => record_id,
            Err(error) => {
                self.unmark_references(items);
                return Err(error);
            }
        };

        let offset = to_u32(self.words.len())?;
        self.words
            .extend_from_slice(&[KIND_LIST << KIND_SHIFT, words_u32, items_u32]);
        self.words.extend(items.iter().map(|item| item.offset));
        Ok(self.complete_record(offset, record_id))
    }

    /// Appends a node whose schema-ordered fields are previously completed values.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid tag/span/reference or an oversized tape.
    pub fn push_node(
        &mut self,
        tag: NodeTag,
        span: Span,
        flags: u8,
        fields: &[ValueRef],
    ) -> Result<NodeRef, TapeError> {
        if tag.get() == 0 {
            return Err(TapeError::InvalidTag);
        }
        self.validate_span(span)?;
        let words = 5_usize
            .checked_add(fields.len())
            .ok_or(TapeError::TooLarge)?;
        let words_u32 = to_u32(words)?;
        let fields_u32 = to_u32(fields.len())?;
        self.ensure_record_capacity(words)?;
        let next_node_count = self.node_count.checked_add(1).ok_or(TapeError::TooLarge)?;
        self.mark_references(fields)?;
        let record_id = match self.take_record_id() {
            Ok(record_id) => record_id,
            Err(error) => {
                self.unmark_references(fields);
                return Err(error);
            }
        };

        let offset = to_u32(self.words.len())?;
        self.words.extend_from_slice(&[
            (KIND_NODE << KIND_SHIFT)
                | (u32::from(flags) << NODE_FLAGS_SHIFT)
                | u32::from(tag.get()),
            words_u32,
            span.start,
            span.end,
            fields_u32,
        ]);
        self.words.extend(fields.iter().map(|field| field.offset));
        let value_ref = self.complete_record(offset, record_id);
        self.node_count = next_node_count;
        Ok(NodeRef(value_ref))
    }

    /// Seals the final node as the root and validates the complete wire tape.
    ///
    /// # Errors
    ///
    /// Returns an error if `root` is stale, non-final, or the resulting tree is malformed.
    pub fn finish(mut self, root: NodeRef) -> Result<FrozenTape, TapeError> {
        self.validate_reference(root.as_value())?;
        if self
            .records
            .last()
            .is_none_or(|record| record.offset != root.offset() || record.id != root.0.record_id)
        {
            return Err(TapeError::RootMustBeFinalNode);
        }
        self.validate_tree_references()?;

        let record_end = to_u32(self.words.len())?;
        let pool_bytes = to_u32(self.string_pool.len())?;
        for chunk in self.string_pool.chunks(4) {
            let mut bytes = [0; 4];
            bytes[..chunk.len()].copy_from_slice(chunk);
            self.words.push(u32::from_le_bytes(bytes));
        }

        self.words[HEADER_MAGIC] = MAGIC;
        self.words[HEADER_VERSION] = FORMAT_VERSION;
        self.words[HEADER_SIZE] = to_u32(HEADER_WORDS)?;
        self.words[HEADER_FLAGS] = WIRE_FLAGS;
        self.words[HEADER_TOTAL_WORDS] = to_u32(self.words.len())?;
        self.words[HEADER_RECORD_END] = record_end;
        self.words[HEADER_POOL_BYTES] = pool_bytes;
        self.words[HEADER_ROOT] = root.offset();
        self.words[HEADER_SOURCE_BYTES] = self.source_bytes;
        self.words[HEADER_NODE_COUNT] = self.node_count;
        self.words[HEADER_VALUE_COUNT] = to_u32(self.records.len())?;
        self.words[HEADER_RESERVED] = 0;

        Ok(FrozenTape::from_builder(self.words, self.records))
    }

    fn push_record(&mut self, words: &[u32]) -> Result<ValueRef, TapeError> {
        self.ensure_record_capacity(words.len())?;
        let record_id = self.take_record_id()?;
        let offset = to_u32(self.words.len())?;
        self.words.extend_from_slice(words);
        Ok(self.complete_record(offset, record_id))
    }

    fn take_record_id(&mut self) -> Result<u64, TapeError> {
        let record_id = self.next_record_id;
        self.next_record_id = self
            .next_record_id
            .checked_add(1)
            .ok_or(TapeError::TooLarge)?;
        Ok(record_id)
    }

    fn complete_record(&mut self, offset: u32, record_id: u64) -> ValueRef {
        self.records.push(BuilderRecord {
            id: record_id,
            offset,
            incoming: 0,
        });
        ValueRef {
            builder_id: self.id,
            record_id,
            offset,
        }
    }

    fn ensure_record_capacity(&self, additional: usize) -> Result<(), TapeError> {
        let end = self
            .words
            .len()
            .checked_add(additional)
            .ok_or(TapeError::TooLarge)?;
        let _ = to_u32(end)?;
        Ok(())
    }

    fn mark_references(&mut self, references: &[ValueRef]) -> Result<(), TapeError> {
        for (marked, reference) in references.iter().enumerate() {
            let index = match self.reference_index(*reference) {
                Ok(index) => index,
                Err(error) => {
                    self.unmark_references(&references[..marked]);
                    return Err(error);
                }
            };
            let Some(incoming) = self.records[index].incoming.checked_add(1) else {
                self.unmark_references(&references[..marked]);
                return Err(TapeError::TooLarge);
            };
            self.records[index].incoming = incoming;
        }
        Ok(())
    }

    fn unmark_references(&mut self, references: &[ValueRef]) {
        for reference in references {
            let Ok(index) = self.reference_index(*reference) else {
                debug_assert!(false, "marked builder references must remain valid");
                continue;
            };
            let incoming = &mut self.records[index].incoming;
            debug_assert!(
                *incoming > 0,
                "marked builder references must have an incoming edge"
            );
            *incoming = incoming.saturating_sub(1);
        }
    }

    fn unmark_discarded_references(&mut self, retained_records: usize) -> Result<(), TapeError> {
        for record_index in retained_records..self.records.len() {
            let offset = to_usize(self.records[record_index].offset)?;
            let kind = (self.words[offset] & KIND_MASK) >> KIND_SHIFT;
            let references = match kind {
                KIND_NODE => offset + 5..offset + 5 + to_usize(self.words[offset + 4])?,
                KIND_LIST => offset + 3..offset + 3 + to_usize(self.words[offset + 2])?,
                _ => offset..offset,
            };
            for word in references {
                let reference = self.words[word];
                let Ok(target) = self
                    .records
                    .binary_search_by_key(&reference, |record| record.offset)
                else {
                    debug_assert!(false, "builder records only contain valid references");
                    continue;
                };
                if target >= retained_records {
                    continue;
                }
                let incoming = &mut self.records[target].incoming;
                debug_assert!(
                    *incoming > 0,
                    "discarded references must have an incoming edge"
                );
                *incoming = incoming.saturating_sub(1);
            }
        }
        Ok(())
    }

    fn validate_tree_references(&self) -> Result<(), TapeError> {
        let Some((root, records)) = self.records.split_last() else {
            return Err(TapeError::RootMustBeFinalNode);
        };
        if root.incoming != 0 {
            return Err(TapeError::MalformedRecord {
                offset: root.offset,
                reason: "root is referenced by another record",
            });
        }
        for record in records {
            let reason = match record.incoming {
                1 => continue,
                0 => "non-root record is not referenced exactly once",
                _ => "record is referenced more than once",
            };
            return Err(TapeError::MalformedRecord {
                offset: record.offset,
                reason,
            });
        }
        Ok(())
    }

    fn reference_index(&self, reference: ValueRef) -> Result<usize, TapeError> {
        if reference.builder_id != self.id {
            return Err(TapeError::ForeignReference);
        }
        let Ok(index) = self
            .records
            .binary_search_by_key(&reference.offset, |record| record.offset)
        else {
            return Err(TapeError::ForeignReference);
        };
        if self.records[index].id != reference.record_id {
            return Err(TapeError::ForeignReference);
        }
        Ok(index)
    }

    fn validate_reference(&self, reference: ValueRef) -> Result<(), TapeError> {
        self.reference_index(reference).map(|_| ())
    }

    const fn validate_span(&self, span: Span) -> Result<(), TapeError> {
        if span.start > span.end {
            return Err(TapeError::InvalidSpan(span));
        }
        if span.end > self.source_bytes {
            return Err(TapeError::SourceRange(span));
        }
        Ok(())
    }
}

fn to_u32(value: usize) -> Result<u32, TapeError> {
    u32::try_from(value).map_err(|_| TapeError::TooLarge)
}

fn to_usize(value: u32) -> Result<usize, TapeError> {
    usize::try_from(value).map_err(|_| TapeError::InvalidHeader("word offset does not fit usize"))
}

const fn span_in_bounds(span: Span, source_bytes: u32) -> bool {
    if span.start > span.end {
        return false;
    }
    span.end <= source_bytes
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TapeHeader {
    pub total_words: u32,
    pub record_end: u32,
    pub string_pool_bytes: u32,
    pub root: u32,
    pub source_bytes: u32,
    pub node_count: u32,
    pub value_count: u32,
}

#[derive(Clone, Debug)]
pub struct FrozenTape {
    words: Box<[u32]>,
    record_offsets: Box<[u32]>,
}

impl FrozenTape {
    fn from_builder(words: Vec<u32>, records: Vec<BuilderRecord>) -> Self {
        // Builder writes are already validated and carry exact incoming-edge counts.
        let record_offsets = records
            .into_iter()
            .map(|record| record.offset)
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            words: words.into_boxed_slice(),
            record_offsets,
        }
    }

    /// Creates and validates a tape from host-endian words.
    ///
    /// # Errors
    ///
    /// Returns an error if the header, records, references, or pool violate the wire format.
    pub fn from_words(words: Vec<u32>) -> Result<Self, TapeError> {
        let mut tape = Self {
            words: words.into_boxed_slice(),
            record_offsets: Box::default(),
        };
        let mut record_offsets = Vec::new();
        for record in tape.validation() {
            record_offsets.push(record?.offset);
        }
        tape.record_offsets = record_offsets.into_boxed_slice();
        Ok(tape)
    }

    /// Creates and validates a tape from little-endian wire bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the byte length or decoded tape is malformed.
    pub fn from_le_bytes(bytes: &[u8]) -> Result<Self, TapeError> {
        if !bytes.len().is_multiple_of(4) {
            return Err(TapeError::InvalidHeader(
                "byte length is not a multiple of four",
            ));
        }
        let words = bytes
            .chunks_exact(4)
            .map(|chunk| u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect();
        Self::from_words(words)
    }

    #[must_use]
    pub fn words(&self) -> &[u32] {
        &self.words
    }

    #[must_use]
    pub fn header(&self) -> TapeHeader {
        TapeHeader {
            total_words: self.words[HEADER_TOTAL_WORDS],
            record_end: self.words[HEADER_RECORD_END],
            string_pool_bytes: self.words[HEADER_POOL_BYTES],
            root: self.words[HEADER_ROOT],
            source_bytes: self.words[HEADER_SOURCE_BYTES],
            node_count: self.words[HEADER_NODE_COUNT],
            value_count: self.words[HEADER_VALUE_COUNT],
        }
    }

    #[must_use]
    pub fn to_le_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.words.len() * 4);
        for word in &self.words {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes
    }

    /// Decodes a record at an exact record-start offset without copying its payload.
    ///
    /// # Errors
    ///
    /// Returns [`TapeError::InvalidRecordOffset`] if `offset` is outside the record section or
    /// points inside another record.
    pub fn value_at(&self, offset: u32) -> Result<TapeValue<'_>, TapeError> {
        if self.record_offsets.binary_search(&offset).is_err() {
            return Err(TapeError::InvalidRecordOffset(offset));
        }
        let offset_usize = to_usize(offset)?;
        let header = self.words[offset_usize];
        let kind = (header & KIND_MASK) >> KIND_SHIFT;
        let value = match kind {
            KIND_NODE => {
                let words = self.words[offset_usize + 1];
                let span = Span::new(self.words[offset_usize + 2], self.words[offset_usize + 3]);
                let fields = to_usize(self.words[offset_usize + 4])?;
                let end = offset_usize + 5 + fields;
                let tag_bits = u16::try_from(header & NODE_TAG_MASK).map_err(|_| {
                    TapeError::MalformedRecord {
                        offset,
                        reason: "validated node tag exceeds u16",
                    }
                })?;
                let tag = NodeTag::new(tag_bits).ok_or(TapeError::MalformedRecord {
                    offset,
                    reason: "validated node tag is zero",
                })?;
                TapeValue::Node {
                    tag,
                    flags: u8::try_from((header & NODE_FLAGS_MASK) >> NODE_FLAGS_SHIFT)
                        .unwrap_or(0),
                    span,
                    fields: &self.words[offset_usize + 5..end],
                    words,
                }
            }
            KIND_LIST => {
                let words = self.words[offset_usize + 1];
                let items = to_usize(self.words[offset_usize + 2])?;
                let end = offset_usize + 3 + items;
                TapeValue::List {
                    items: &self.words[offset_usize + 3..end],
                    words,
                }
            }
            KIND_NULL => TapeValue::Null,
            KIND_BOOL => TapeValue::Bool(header & 1 != 0),
            KIND_INLINE_U32 => TapeValue::U32(header & INLINE_U32_MASK),
            KIND_U32 => TapeValue::U32(self.words[offset_usize + 1]),
            KIND_F64 => {
                let bits = u64::from(self.words[offset_usize + 1])
                    | (u64::from(self.words[offset_usize + 2]) << 32);
                TapeValue::F64(f64::from_bits(bits))
            }
            KIND_SOURCE_SLICE => TapeValue::SourceSlice(Span::new(
                self.words[offset_usize + 1],
                self.words[offset_usize + 2],
            )),
            KIND_POOL_STRING => TapeValue::PoolString {
                start: self.words[offset_usize + 1],
                len: self.words[offset_usize + 2],
            },
            _ => {
                return Err(TapeError::MalformedRecord {
                    offset,
                    reason: "validated record has an unknown kind",
                });
            }
        };
        Ok(value)
    }

    /// Borrows the packed UTF-8 string pool directly from the tape allocation.
    ///
    /// # Errors
    ///
    /// Returns [`TapeError::ZeroCopyPoolUnsupported`] on big-endian hosts, where the in-memory
    /// representation of the host-endian words does not match the little-endian wire bytes.
    pub fn string_pool_bytes(&self) -> Result<&[u8], TapeError> {
        #[cfg(target_endian = "big")]
        {
            Err(TapeError::ZeroCopyPoolUnsupported)
        }
        #[cfg(target_endian = "little")]
        {
            let header = self.header();
            let start = to_usize(header.record_end)?;
            let len = to_usize(header.string_pool_bytes)?;
            let pool_words = &self.words[start..];
            // SAFETY: `u8` has alignment one and may view any initialized object representation.
            // `pool_words` remains immutably borrowed for the returned lifetime. On a little-endian
            // host its bytes are exactly the packed wire bytes, and validated header bounds ensure
            // `len` excludes only the final zero padding.
            let bytes = unsafe {
                std::slice::from_raw_parts(pool_words.as_ptr().cast::<u8>(), pool_words.len() * 4)
            };
            Ok(&bytes[..len])
        }
    }

    /// Borrows a UTF-8 string-pool slice without allocating.
    ///
    /// # Errors
    ///
    /// Returns an error for an unsupported host, an out-of-bounds range, or a range that splits a
    /// UTF-8 code point.
    pub fn string_view(&self, start: u32, len: u32) -> Result<&str, TapeError> {
        let end = start
            .checked_add(len)
            .filter(|end| *end <= self.header().string_pool_bytes)
            .ok_or(TapeError::InvalidHeader("string slice is out of bounds"))?;
        let pool = self.string_pool_bytes()?;
        let start = to_usize(start)?;
        let end = to_usize(end)?;
        std::str::from_utf8(&pool[start..end])
            .map_err(|_| TapeError::InvalidHeader("string slice is not complete UTF-8"))
    }

    /// Decodes a string-pool slice.
    ///
    /// # Errors
    ///
    /// Returns an error if the range is out of bounds or does not contain complete UTF-8.
    pub fn string(&self, start: u32, len: u32) -> Result<String, TapeError> {
        #[cfg(target_endian = "little")]
        {
            self.string_view(start, len).map(str::to_owned)
        }
        #[cfg(target_endian = "big")]
        {
            let end = start
                .checked_add(len)
                .filter(|end| *end <= self.header().string_pool_bytes)
                .ok_or(TapeError::InvalidHeader("string slice is out of bounds"))?;
            let pool_start = to_usize(self.header().record_end)?;
            let mut pool = Vec::with_capacity(to_usize(self.header().string_pool_bytes)?);
            for word in &self.words[pool_start..] {
                pool.extend_from_slice(&word.to_le_bytes());
            }
            pool.truncate(to_usize(self.header().string_pool_bytes)?);
            String::from_utf8(pool[to_usize(start)?..to_usize(end)?].to_vec())
                .map_err(|_| TapeError::InvalidHeader("string slice is not complete UTF-8"))
        }
    }

    /// Validates all records and the single-root tree invariant.
    ///
    /// # Errors
    ///
    /// Returns the first malformed header, record, or reference error.
    pub fn validate(&self) -> Result<(), TapeError> {
        for record in self.validation() {
            let _ = record?;
        }
        Ok(())
    }

    #[must_use]
    pub fn validation(&self) -> ValidationIter<'_> {
        ValidationIter::new(self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TapeValue<'a> {
    Node {
        tag: NodeTag,
        flags: u8,
        span: Span,
        fields: &'a [u32],
        words: u32,
    },
    List {
        items: &'a [u32],
        words: u32,
    },
    Null,
    Bool(bool),
    U32(u32),
    F64(f64),
    SourceSlice(Span),
    PoolString {
        start: u32,
        len: u32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ValidatedRecord<'a> {
    pub offset: u32,
    pub value: TapeValue<'a>,
}

pub struct ValidationIter<'a> {
    tape: &'a FrozenTape,
    cursor: usize,
    record_end: usize,
    root: u32,
    pool_bytes: u32,
    source_bytes: u32,
    expected_nodes: u32,
    expected_values: u32,
    seen_nodes: u32,
    seen_values: u32,
    offsets: Vec<u32>,
    incoming: Vec<u8>,
    last_was_node: bool,
    pending_error: Option<TapeError>,
    finished: bool,
}

impl<'a> ValidationIter<'a> {
    fn new(tape: &'a FrozenTape) -> Self {
        match validate_header(&tape.words) {
            Ok(header) => Self {
                tape,
                cursor: HEADER_WORDS,
                record_end: usize::try_from(header.record_end).unwrap_or(HEADER_WORDS),
                root: header.root,
                pool_bytes: header.string_pool_bytes,
                source_bytes: header.source_bytes,
                expected_nodes: header.node_count,
                expected_values: header.value_count,
                seen_nodes: 0,
                seen_values: 0,
                offsets: Vec::new(),
                incoming: Vec::new(),
                last_was_node: false,
                pending_error: None,
                finished: false,
            },
            Err(error) => Self {
                tape,
                cursor: 0,
                record_end: 0,
                root: 0,
                pool_bytes: 0,
                source_bytes: 0,
                expected_nodes: 0,
                expected_values: 0,
                seen_nodes: 0,
                seen_values: 0,
                offsets: Vec::new(),
                incoming: Vec::new(),
                last_was_node: false,
                pending_error: Some(error),
                finished: false,
            },
        }
    }

    #[allow(clippy::unnecessary_wraps)]
    fn fail(&mut self, reason: &'static str) -> Option<Result<ValidatedRecord<'a>, TapeError>> {
        self.finished = true;
        Some(Err(TapeError::MalformedRecord {
            offset: u32::try_from(self.cursor).unwrap_or(u32::MAX),
            reason,
        }))
    }

    fn mark_reference(&mut self, reference: u32) -> Result<(), &'static str> {
        if usize::try_from(reference).map_or(true, |reference| reference >= self.cursor) {
            return Err("reference does not point backward");
        }
        let Ok(index) = self.offsets.binary_search(&reference) else {
            return Err("reference does not point to a record start");
        };
        let Some(incoming) = self.incoming.get_mut(index) else {
            return Err("reference index is out of bounds");
        };
        *incoming = incoming.checked_add(1).ok_or("reference count overflow")?;
        if *incoming > 1 {
            return Err("record is referenced more than once");
        }
        Ok(())
    }

    fn finish_validation(&mut self) -> Option<Result<ValidatedRecord<'a>, TapeError>> {
        self.finished = true;
        if self.offsets.last().copied() != Some(self.root) || !self.last_was_node {
            return Some(Err(TapeError::MalformedRecord {
                offset: self.root,
                reason: "root is not the final node",
            }));
        }
        if self.seen_nodes != self.expected_nodes {
            return Some(Err(TapeError::MalformedRecord {
                offset: self.root,
                reason: "node count does not match the header",
            }));
        }
        if self.seen_values != self.expected_values {
            return Some(Err(TapeError::MalformedRecord {
                offset: self.root,
                reason: "value count does not match the header",
            }));
        }
        if self.incoming.last().copied() != Some(0) {
            return Some(Err(TapeError::MalformedRecord {
                offset: self.root,
                reason: "root is referenced by another record",
            }));
        }
        if let Some(index) = self.incoming[..self.incoming.len().saturating_sub(1)]
            .iter()
            .position(|count| *count != 1)
        {
            return Some(Err(TapeError::MalformedRecord {
                offset: self.offsets[index],
                reason: "non-root record is not referenced exactly once",
            }));
        }
        None
    }
}

impl<'a> Iterator for ValidationIter<'a> {
    type Item = Result<ValidatedRecord<'a>, TapeError>;

    #[allow(clippy::too_many_lines)]
    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }
        if let Some(error) = self.pending_error.take() {
            self.finished = true;
            return Some(Err(error));
        }
        if self.cursor == self.record_end {
            return self.finish_validation();
        }
        if self.cursor > self.record_end || self.cursor >= self.tape.words.len() {
            return self.fail("record cursor is out of bounds");
        }

        let offset = self.cursor;
        let header = self.tape.words[offset];
        let kind = (header & KIND_MASK) >> KIND_SHIFT;

        let (size, value, is_node) = match kind {
            KIND_NODE => {
                if offset
                    .checked_add(5)
                    .is_none_or(|end| end > self.record_end)
                {
                    return self.fail("truncated node header");
                }
                if header & !(KIND_MASK | NODE_FLAGS_MASK | NODE_TAG_MASK) != 0 {
                    return self.fail("node header uses reserved bits");
                }
                let tag_bits = header & NODE_TAG_MASK;
                let Ok(tag_bits) = u16::try_from(tag_bits) else {
                    return self.fail("node tag exceeds u16");
                };
                let Some(tag) = NodeTag::new(tag_bits) else {
                    return self.fail("node tag zero is reserved");
                };
                let words = self.tape.words[offset + 1];
                let span = Span::new(self.tape.words[offset + 2], self.tape.words[offset + 3]);
                let field_count = self.tape.words[offset + 4];
                let Ok(field_count_usize) = usize::try_from(field_count) else {
                    return self.fail("node field count does not fit usize");
                };
                let Some(expected_size) = 5_usize.checked_add(field_count_usize) else {
                    return self.fail("node word length overflows");
                };
                if usize::try_from(words).ok() != Some(expected_size)
                    || offset
                        .checked_add(expected_size)
                        .is_none_or(|end| end > self.record_end)
                {
                    return self.fail("node length does not exactly match its fields");
                }
                if !span_in_bounds(span, self.source_bytes) {
                    return self.fail("node span is out of bounds");
                }
                for index in offset + 5..offset + expected_size {
                    if let Err(reason) = self.mark_reference(self.tape.words[index]) {
                        return self.fail(reason);
                    }
                }
                let Some(seen_nodes) = self.seen_nodes.checked_add(1) else {
                    return self.fail("node count overflows u32");
                };
                self.seen_nodes = seen_nodes;
                (
                    expected_size,
                    TapeValue::Node {
                        tag,
                        flags: u8::try_from((header & NODE_FLAGS_MASK) >> NODE_FLAGS_SHIFT)
                            .unwrap_or(0),
                        span,
                        fields: &self.tape.words[offset + 5..offset + expected_size],
                        words,
                    },
                    true,
                )
            }
            KIND_LIST => {
                if offset
                    .checked_add(3)
                    .is_none_or(|end| end > self.record_end)
                    || header != KIND_LIST << KIND_SHIFT
                {
                    return self.fail("invalid list header");
                }
                let words = self.tape.words[offset + 1];
                let item_count = self.tape.words[offset + 2];
                let Ok(item_count_usize) = usize::try_from(item_count) else {
                    return self.fail("list item count does not fit usize");
                };
                let Some(expected_size) = 3_usize.checked_add(item_count_usize) else {
                    return self.fail("list word length overflows");
                };
                if usize::try_from(words).ok() != Some(expected_size)
                    || offset
                        .checked_add(expected_size)
                        .is_none_or(|end| end > self.record_end)
                {
                    return self.fail("list length does not exactly match its items");
                }
                for index in offset + 3..offset + expected_size {
                    if let Err(reason) = self.mark_reference(self.tape.words[index]) {
                        return self.fail(reason);
                    }
                }
                (
                    expected_size,
                    TapeValue::List {
                        items: &self.tape.words[offset + 3..offset + expected_size],
                        words,
                    },
                    false,
                )
            }
            KIND_NULL if header == KIND_NULL << KIND_SHIFT => (1, TapeValue::Null, false),
            KIND_BOOL if header & !1 == KIND_BOOL << KIND_SHIFT => {
                (1, TapeValue::Bool(header & 1 != 0), false)
            }
            KIND_INLINE_U32 => (1, TapeValue::U32(header & INLINE_U32_MASK), false),
            KIND_U32 if header == KIND_U32 << KIND_SHIFT => {
                if offset
                    .checked_add(2)
                    .is_none_or(|end| end > self.record_end)
                {
                    return self.fail("truncated u32");
                }
                (2, TapeValue::U32(self.tape.words[offset + 1]), false)
            }
            KIND_F64 if header == KIND_F64 << KIND_SHIFT => {
                if offset
                    .checked_add(3)
                    .is_none_or(|end| end > self.record_end)
                {
                    return self.fail("truncated f64");
                }
                let bits = u64::from(self.tape.words[offset + 1])
                    | (u64::from(self.tape.words[offset + 2]) << 32);
                (3, TapeValue::F64(f64::from_bits(bits)), false)
            }
            KIND_SOURCE_SLICE if header == KIND_SOURCE_SLICE << KIND_SHIFT => {
                if offset
                    .checked_add(3)
                    .is_none_or(|end| end > self.record_end)
                {
                    return self.fail("truncated source slice");
                }
                let span = Span::new(self.tape.words[offset + 1], self.tape.words[offset + 2]);
                if !span_in_bounds(span, self.source_bytes) {
                    return self.fail("source slice is out of bounds");
                }
                (3, TapeValue::SourceSlice(span), false)
            }
            KIND_POOL_STRING if header == KIND_POOL_STRING << KIND_SHIFT => {
                if offset
                    .checked_add(3)
                    .is_none_or(|end| end > self.record_end)
                {
                    return self.fail("truncated pool string");
                }
                let start = self.tape.words[offset + 1];
                let len = self.tape.words[offset + 2];
                if start
                    .checked_add(len)
                    .is_none_or(|end| end > self.pool_bytes)
                {
                    return self.fail("string pool slice is out of bounds");
                }
                (3, TapeValue::PoolString { start, len }, false)
            }
            _ => return self.fail("unknown or malformed value kind"),
        };

        let Some(next_cursor) = self.cursor.checked_add(size) else {
            return self.fail("record cursor overflows usize");
        };
        let Some(seen_values) = self.seen_values.checked_add(1) else {
            return self.fail("value count overflows u32");
        };
        self.cursor = next_cursor;
        self.seen_values = seen_values;
        self.last_was_node = is_node;
        let Ok(offset_u32) = u32::try_from(offset) else {
            return self.fail("record offset exceeds u32");
        };
        self.offsets.push(offset_u32);
        self.incoming.push(0);
        Some(Ok(ValidatedRecord {
            offset: offset_u32,
            value,
        }))
    }
}

fn validate_header(words: &[u32]) -> Result<TapeHeader, TapeError> {
    if words.len() < HEADER_WORDS {
        return Err(TapeError::InvalidHeader("truncated header"));
    }
    if words[HEADER_MAGIC] != MAGIC {
        return Err(TapeError::InvalidHeader("magic does not match"));
    }
    if words[HEADER_VERSION] != FORMAT_VERSION {
        return Err(TapeError::UnsupportedVersion(words[HEADER_VERSION]));
    }
    if words[HEADER_SIZE] != to_u32(HEADER_WORDS)? {
        return Err(TapeError::InvalidHeader("header size does not match"));
    }
    if words[HEADER_FLAGS] != WIRE_FLAGS {
        return Err(TapeError::InvalidHeader("unsupported flags"));
    }
    if to_usize(words[HEADER_TOTAL_WORDS])? != words.len() {
        return Err(TapeError::InvalidHeader("total word count does not match"));
    }

    let record_end = to_usize(words[HEADER_RECORD_END])?;
    let pool_bytes = to_usize(words[HEADER_POOL_BYTES])?;
    let pool_words = pool_bytes.div_ceil(4);
    if record_end <= HEADER_WORDS || record_end.checked_add(pool_words) != Some(words.len()) {
        return Err(TapeError::InvalidHeader(
            "record and string pool bounds do not match",
        ));
    }
    let root = to_usize(words[HEADER_ROOT])?;
    if root < HEADER_WORDS || root >= record_end {
        return Err(TapeError::InvalidHeader("root offset is out of bounds"));
    }
    if words[HEADER_VALUE_COUNT] == 0 {
        return Err(TapeError::InvalidHeader("tape has no values"));
    }
    if words[HEADER_RESERVED] != 0 {
        return Err(TapeError::InvalidHeader("reserved word is not zero"));
    }

    if pool_bytes % 4 != 0 {
        let last = words[words.len() - 1].to_le_bytes();
        for byte in &last[pool_bytes % 4..] {
            if *byte != 0 {
                return Err(TapeError::InvalidHeader("string pool padding is not zero"));
            }
        }
    }

    let mut decoded_pool = Vec::with_capacity(pool_bytes);
    for word in &words[record_end..] {
        decoded_pool.extend_from_slice(&word.to_le_bytes());
    }
    decoded_pool.truncate(pool_bytes);
    if std::str::from_utf8(&decoded_pool).is_err() {
        return Err(TapeError::InvalidHeader("string pool is not UTF-8"));
    }

    Ok(TapeHeader {
        total_words: words[HEADER_TOTAL_WORDS],
        record_end: words[HEADER_RECORD_END],
        string_pool_bytes: words[HEADER_POOL_BYTES],
        root: words[HEADER_ROOT],
        source_bytes: words[HEADER_SOURCE_BYTES],
        node_count: words[HEADER_NODE_COUNT],
        value_count: words[HEADER_VALUE_COUNT],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_and_validates_a_postfix_program() {
        let mut tape = TapeBuilder::new(12);
        let source_type = tape.push_u32(1).expect("source type");
        let name = tape
            .push_source_slice(Span::new(4, 9))
            .expect("identifier name");
        let identifier = tape
            .push_node(NodeTag::IDENTIFIER, Span::new(4, 9), 0, &[name])
            .expect("identifier");
        let body = tape.push_list(&[identifier.into()]).expect("body list");
        let program = tape
            .push_node(NodeTag::PROGRAM, Span::new(0, 12), 0, &[source_type, body])
            .expect("program");

        let tape = tape.finish(program).expect("finish tape");
        let records = tape
            .validation()
            .collect::<Result<Vec<_>, _>>()
            .expect("valid tape");
        assert_eq!(records.len(), 5);
        assert_eq!(tape.header().node_count, 2);
        assert_eq!(tape.header().root, program.offset());

        let bytes = tape.to_le_bytes();
        let decoded = FrozenTape::from_le_bytes(&bytes).expect("decode wire bytes");
        assert_eq!(decoded.words(), tape.words());
    }

    #[test]
    fn trusted_finish_matches_strict_reparse() {
        let mut builder = TapeBuilder::new(8);
        let name = builder.push_source_slice(Span::new(0, 4)).expect("name");
        let identifier = builder
            .push_node(NodeTag::IDENTIFIER, Span::new(0, 4), 0, &[name])
            .expect("identifier");
        let body = builder.push_list(&[identifier.into()]).expect("body");
        let source_type = builder.push_u32(1).expect("source type");
        let root = builder
            .push_node(NodeTag::PROGRAM, Span::new(0, 8), 0, &[body, source_type])
            .expect("program");

        let trusted = builder.finish(root).expect("trusted finish");
        let strict = FrozenTape::from_words(trusted.words().to_vec()).expect("strict reparse");

        assert_eq!(trusted.words, strict.words);
        assert_eq!(trusted.record_offsets, strict.record_offsets);
    }

    #[test]
    fn f64_is_exactly_three_words_and_round_trips() {
        let value = -12_345.25_f64;
        let mut tape = TapeBuilder::new(0);
        let number = tape.push_f64(value).expect("number");
        let program = tape
            .push_node(NodeTag::PROGRAM, Span::new(0, 0), 0, &[number])
            .expect("program");
        let tape = tape.finish(program).expect("finish tape");

        assert_eq!(
            number.offset(),
            to_u32(HEADER_WORDS).expect("header offset")
        );
        assert_eq!(program.offset(), number.offset() + 3);
        assert_eq!(tape.words()[HEADER_WORDS], KIND_F64 << KIND_SHIFT);
        let record = tape
            .validation()
            .next()
            .expect("number record")
            .expect("valid number");
        assert_eq!(record.value, TapeValue::F64(value));

        let decoded = FrozenTape::from_le_bytes(&tape.to_le_bytes()).expect("wire round trip");
        assert_eq!(decoded.words(), tape.words());
    }

    #[test]
    fn random_access_borrows_every_value_kind() {
        let mut tape = TapeBuilder::new(16);
        let null = tape.push_null().expect("null");
        let boolean = tape.push_bool(true).expect("boolean");
        let inline = tape.push_u32(7).expect("inline integer");
        let full = tape.push_u32(u32::MAX).expect("full integer");
        let number = tape.push_f64(-0.25).expect("number");
        let source = tape
            .push_source_slice(Span::new(2, 8))
            .expect("source slice");
        let string = tape.push_string("hé").expect("string");
        let child = tape
            .push_node(NodeTag::IDENTIFIER, Span::new(2, 8), 3, &[])
            .expect("child node");
        let list = tape
            .push_list(&[
                null,
                boolean,
                inline,
                full,
                number,
                source,
                string,
                child.into(),
            ])
            .expect("list");
        let root = tape
            .push_node(NodeTag::PROGRAM, Span::new(0, 16), 0, &[list])
            .expect("root");
        let tape = tape.finish(root).expect("finish tape");

        assert_eq!(tape.value_at(null.offset()).expect("null"), TapeValue::Null);
        assert_eq!(
            tape.value_at(boolean.offset()).expect("boolean"),
            TapeValue::Bool(true)
        );
        assert_eq!(
            tape.value_at(inline.offset()).expect("inline"),
            TapeValue::U32(7)
        );
        assert_eq!(
            tape.value_at(full.offset()).expect("full"),
            TapeValue::U32(u32::MAX)
        );
        assert_eq!(
            tape.value_at(number.offset()).expect("number"),
            TapeValue::F64(-0.25)
        );
        assert_eq!(
            tape.value_at(source.offset()).expect("source"),
            TapeValue::SourceSlice(Span::new(2, 8))
        );
        assert_eq!(
            tape.value_at(string.offset()).expect("string record"),
            TapeValue::PoolString { start: 0, len: 3 }
        );
        assert!(matches!(
            tape.value_at(child.offset()).expect("child"),
            TapeValue::Node {
                tag: NodeTag::IDENTIFIER,
                flags: 3,
                fields: [],
                ..
            }
        ));
        assert!(matches!(
            tape.value_at(list.offset()).expect("list"),
            TapeValue::List { items, .. } if items.len() == 8
        ));
        assert!(matches!(
            tape.value_at(root.offset()).expect("root"),
            TapeValue::Node {
                tag: NodeTag::PROGRAM,
                fields: [_],
                ..
            }
        ));
        assert_eq!(tape.string_view(0, 3).expect("borrowed string"), "hé");
        assert_eq!(tape.string(0, 3).expect("owned string"), "hé");

        #[cfg(target_endian = "little")]
        {
            let pool = tape.string_pool_bytes().expect("borrowed pool");
            let record_end = usize::try_from(tape.header().record_end).expect("record end");
            assert_eq!(pool, "hé".as_bytes());
            assert_eq!(
                pool.as_ptr(),
                tape.words()[record_end..].as_ptr().cast::<u8>()
            );
        }
    }

    #[test]
    fn random_access_rejects_non_record_and_malformed_offsets() {
        let mut tape = TapeBuilder::new(0);
        let value = tape.push_u32(u32::MAX).expect("value");
        let root = tape
            .push_node(NodeTag::PROGRAM, Span::new(0, 0), 0, &[value])
            .expect("root");
        let tape = tape.finish(root).expect("finish tape");

        for offset in [0, value.offset() + 1, tape.header().record_end, u32::MAX] {
            assert!(matches!(
                tape.value_at(offset),
                Err(TapeError::InvalidRecordOffset(actual)) if actual == offset
            ));
        }

        let mut malformed = tape.words().to_vec();
        malformed[HEADER_ROOT] = value.offset() + 1;
        assert!(FrozenTape::from_words(malformed).is_err());
    }

    #[test]
    fn rollback_invalidates_handles_from_the_discarded_branch() {
        let mut tape = TapeBuilder::new(4);
        let checkpoint = tape.checkpoint();
        let stale = tape.push_string("discarded").expect("discarded string");
        tape.rollback(checkpoint).expect("rollback");

        let replacement = tape.push_null().expect("replacement");
        assert_eq!(stale.offset(), replacement.offset());
        assert!(matches!(
            tape.push_list(&[stale]),
            Err(TapeError::ForeignReference)
        ));

        let body = tape.push_list(&[replacement]).expect("body");
        let program = tape
            .push_node(NodeTag::PROGRAM, Span::new(0, 4), 0, &[body])
            .expect("program");
        let tape = tape.finish(program).expect("finish tape");
        assert_eq!(tape.header().string_pool_bytes, 0);
    }

    #[test]
    fn rollback_removes_edges_from_discarded_records() {
        let mut tape = TapeBuilder::new(0);
        let retained = tape.push_null().expect("retained value");
        let checkpoint = tape.checkpoint();
        let _discarded = tape
            .push_node(NodeTag::IDENTIFIER, Span::new(0, 0), 0, &[retained])
            .expect("discarded node");
        tape.rollback(checkpoint).expect("rollback");

        let root = tape
            .push_node(NodeTag::PROGRAM, Span::new(0, 0), 0, &[retained])
            .expect("program");
        tape.finish(root).expect("finish tape");
    }

    #[test]
    fn rejects_forward_and_non_record_references() {
        let mut builder = TapeBuilder::new(0);
        let forward = ValueRef {
            builder_id: builder.id,
            record_id: builder.next_record_id,
            offset: to_u32(builder.words.len()).expect("future offset"),
        };
        assert!(matches!(
            builder.push_node(NodeTag::PROGRAM, Span::new(0, 0), 0, &[forward]),
            Err(TapeError::ForeignReference)
        ));

        let mut tape = TapeBuilder::new(0);
        let value = tape.push_null().expect("value");
        let program = tape
            .push_node(NodeTag::PROGRAM, Span::new(0, 0), 0, &[value])
            .expect("program");
        let tape = tape.finish(program).expect("finish tape");

        let mut forward = tape.words().to_vec();
        forward[usize::try_from(program.offset()).expect("root offset") + 5] = program.offset();
        assert!(FrozenTape::from_words(forward).is_err());

        let mut middle = tape.words().to_vec();
        middle[usize::try_from(program.offset()).expect("root offset") + 5] = value.offset() + 1;
        assert!(FrozenTape::from_words(middle).is_err());
    }

    #[test]
    fn rejects_trailing_unconsumed_node_payload() {
        let mut tape = TapeBuilder::new(0);
        let value = tape.push_null().expect("value");
        let program = tape
            .push_node(NodeTag::PROGRAM, Span::new(0, 0), 0, &[value])
            .expect("program");
        let tape = tape.finish(program).expect("finish tape");
        let mut words = tape.words().to_vec();
        let root = usize::try_from(program.offset()).expect("root offset");
        words[root + 1] += 1;

        assert!(FrozenTape::from_words(words).is_err());
    }

    #[test]
    fn trusted_finish_rejects_unreachable_records() {
        let mut tape = TapeBuilder::new(0);
        let _unused = tape.push_null().expect("unused");
        let used = tape.push_bool(true).expect("used");
        let program = tape
            .push_node(NodeTag::PROGRAM, Span::new(0, 0), 0, &[used])
            .expect("program");
        assert!(matches!(
            tape.finish(program),
            Err(TapeError::MalformedRecord {
                reason: "non-root record is not referenced exactly once",
                ..
            })
        ));
    }

    #[test]
    fn trusted_finish_rejects_shared_records() {
        let mut tape = TapeBuilder::new(0);
        let value = tape.push_null().expect("shared");
        let program = tape
            .push_node(NodeTag::PROGRAM, Span::new(0, 0), 0, &[value, value])
            .expect("program");
        assert!(matches!(
            tape.finish(program),
            Err(TapeError::MalformedRecord {
                reason: "record is referenced more than once",
                ..
            })
        ));
    }

    #[test]
    fn trusted_finish_rejects_a_non_final_root() {
        let mut tape = TapeBuilder::new(0);
        let value = tape.push_null().expect("value");
        let root = tape
            .push_node(NodeTag::PROGRAM, Span::new(0, 0), 0, &[value])
            .expect("root");
        let _trailing = tape.push_null().expect("trailing value");

        assert!(matches!(
            tape.finish(root),
            Err(TapeError::RootMustBeFinalNode)
        ));
    }
}
