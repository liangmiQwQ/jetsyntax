//! Parser-owned operator classification and precedence tables.

use crate::lexer::TokenKind;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum BinaryOperator {
    Eq,
    NotEq,
    StrictEq,
    StrictNotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    ShiftLeft,
    ShiftRight,
    ShiftRightUnsigned,
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    Exponent,
    BitOr,
    BitXor,
    BitAnd,
    In,
    Instanceof,
    LogicalOr,
    LogicalAnd,
    Nullish,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BinaryBinding {
    pub operator: BinaryOperator,
    pub left: u8,
    pub right: u8,
    pub logical: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum AssignmentOperator {
    Assign,
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    Exponent,
    ShiftLeft,
    ShiftRight,
    ShiftRightUnsigned,
    BitOr,
    BitXor,
    BitAnd,
    LogicalOr,
    LogicalAnd,
    Nullish,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum UnaryOperator {
    Minus,
    Plus,
    LogicalNot,
    BitwiseNot,
    Typeof,
    Void,
    Delete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum UpdateOperator {
    Increment,
    Decrement,
}

pub const fn binary_binding(kind: TokenKind, allow_in: bool) -> Option<BinaryBinding> {
    let (operator, precedence, right_associative, logical) = match kind {
        TokenKind::PipePipe => (BinaryOperator::LogicalOr, 1, false, true),
        TokenKind::QuestionQuestion => (BinaryOperator::Nullish, 1, false, true),
        TokenKind::AmpAmp => (BinaryOperator::LogicalAnd, 2, false, true),
        TokenKind::Pipe => (BinaryOperator::BitOr, 3, false, false),
        TokenKind::Caret => (BinaryOperator::BitXor, 4, false, false),
        TokenKind::Amp => (BinaryOperator::BitAnd, 5, false, false),
        TokenKind::EqEq => (BinaryOperator::Eq, 6, false, false),
        TokenKind::BangEq => (BinaryOperator::NotEq, 6, false, false),
        TokenKind::EqEqEq => (BinaryOperator::StrictEq, 6, false, false),
        TokenKind::BangEqEq => (BinaryOperator::StrictNotEq, 6, false, false),
        TokenKind::Lt => (BinaryOperator::Lt, 7, false, false),
        TokenKind::LtEq => (BinaryOperator::LtEq, 7, false, false),
        TokenKind::Gt => (BinaryOperator::Gt, 7, false, false),
        TokenKind::GtEq => (BinaryOperator::GtEq, 7, false, false),
        TokenKind::In if allow_in => (BinaryOperator::In, 7, false, false),
        TokenKind::Instanceof => (BinaryOperator::Instanceof, 7, false, false),
        TokenKind::ShiftLeft => (BinaryOperator::ShiftLeft, 8, false, false),
        TokenKind::ShiftRight => (BinaryOperator::ShiftRight, 8, false, false),
        TokenKind::ShiftRightUnsigned => (BinaryOperator::ShiftRightUnsigned, 8, false, false),
        TokenKind::Plus => (BinaryOperator::Add, 9, false, false),
        TokenKind::Minus => (BinaryOperator::Subtract, 9, false, false),
        TokenKind::Star => (BinaryOperator::Multiply, 10, false, false),
        TokenKind::Slash => (BinaryOperator::Divide, 10, false, false),
        TokenKind::Percent => (BinaryOperator::Remainder, 10, false, false),
        TokenKind::StarStar => (BinaryOperator::Exponent, 11, true, false),
        _ => return None,
    };
    let left = precedence * 2;
    let right = if right_associative { left } else { left + 1 };
    Some(BinaryBinding {
        operator,
        left,
        right,
        logical,
    })
}

pub const fn assignment_operator(kind: TokenKind) -> Option<AssignmentOperator> {
    match kind {
        TokenKind::Eq => Some(AssignmentOperator::Assign),
        TokenKind::PlusEq => Some(AssignmentOperator::Add),
        TokenKind::MinusEq => Some(AssignmentOperator::Subtract),
        TokenKind::StarEq => Some(AssignmentOperator::Multiply),
        TokenKind::SlashEq => Some(AssignmentOperator::Divide),
        TokenKind::PercentEq => Some(AssignmentOperator::Remainder),
        TokenKind::StarStarEq => Some(AssignmentOperator::Exponent),
        TokenKind::ShiftLeftEq => Some(AssignmentOperator::ShiftLeft),
        TokenKind::ShiftRightEq => Some(AssignmentOperator::ShiftRight),
        TokenKind::ShiftRightUnsignedEq => Some(AssignmentOperator::ShiftRightUnsigned),
        TokenKind::PipeEq => Some(AssignmentOperator::BitOr),
        TokenKind::CaretEq => Some(AssignmentOperator::BitXor),
        TokenKind::AmpEq => Some(AssignmentOperator::BitAnd),
        TokenKind::PipePipeEq => Some(AssignmentOperator::LogicalOr),
        TokenKind::AmpAmpEq => Some(AssignmentOperator::LogicalAnd),
        TokenKind::QuestionQuestionEq => Some(AssignmentOperator::Nullish),
        _ => None,
    }
}

pub const fn unary_operator(kind: TokenKind) -> Option<UnaryOperator> {
    match kind {
        TokenKind::Minus => Some(UnaryOperator::Minus),
        TokenKind::Plus => Some(UnaryOperator::Plus),
        TokenKind::Bang => Some(UnaryOperator::LogicalNot),
        TokenKind::Tilde => Some(UnaryOperator::BitwiseNot),
        TokenKind::Typeof => Some(UnaryOperator::Typeof),
        TokenKind::Void => Some(UnaryOperator::Void),
        TokenKind::Delete => Some(UnaryOperator::Delete),
        _ => None,
    }
}

pub const fn update_operator(kind: TokenKind) -> Option<UpdateOperator> {
    match kind {
        TokenKind::PlusPlus => Some(UpdateOperator::Increment),
        TokenKind::MinusMinus => Some(UpdateOperator::Decrement),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{AssignmentOperator, BinaryOperator, assignment_operator, binary_binding};
    use crate::lexer::TokenKind;

    #[test]
    fn exponentiation_is_right_associative() {
        let binding = binary_binding(TokenKind::StarStar, true).expect("binding");
        assert_eq!(binding.operator, BinaryOperator::Exponent);
        assert_eq!(binding.left, binding.right);
    }

    #[test]
    fn in_operator_obeys_expression_context() {
        assert!(binary_binding(TokenKind::In, false).is_none());
        assert_eq!(
            binary_binding(TokenKind::In, true).map(|binding| binding.operator),
            Some(BinaryOperator::In)
        );
    }

    #[test]
    fn classifies_logical_assignment() {
        assert_eq!(
            assignment_operator(TokenKind::QuestionQuestionEq),
            Some(AssignmentOperator::Nullish)
        );
    }
}
