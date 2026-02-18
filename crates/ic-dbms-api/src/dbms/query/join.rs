//! Join types for cross-table query operations.

use candid::CandidType;
use serde::{Deserialize, Serialize};

/// Specifies the JOIN operation type.
#[derive(Debug, Clone, PartialEq, Eq, CandidType, Serialize, Deserialize)]
pub enum JoinType {
    Inner,
    Left,
    Right,
    Full,
}

/// A JOIN clause specifying which table to join and on which columns.
#[derive(Debug, Clone, PartialEq, Eq, CandidType, Serialize, Deserialize)]
pub struct Join {
    /// The type of join (INNER, LEFT, RIGHT, FULL).
    pub join_type: JoinType,
    /// The table to join with.
    pub table: String,
    /// Column on the left side of the ON condition.
    /// Qualified ("users.id") or unqualified ("id", defaults to FROM table).
    pub left_column: String,
    /// Column on the right side of the ON condition.
    /// Qualified ("posts.user") or unqualified ("user", defaults to joined table).
    pub right_column: String,
}

impl Join {
    /// Creates an INNER JOIN.
    pub fn inner(table: &str, left_column: &str, right_column: &str) -> Self {
        Self {
            join_type: JoinType::Inner,
            table: table.to_string(),
            left_column: left_column.to_string(),
            right_column: right_column.to_string(),
        }
    }

    /// Creates a LEFT JOIN.
    pub fn left(table: &str, left_column: &str, right_column: &str) -> Self {
        Self {
            join_type: JoinType::Left,
            table: table.to_string(),
            left_column: left_column.to_string(),
            right_column: right_column.to_string(),
        }
    }

    /// Creates a RIGHT JOIN.
    pub fn right(table: &str, left_column: &str, right_column: &str) -> Self {
        Self {
            join_type: JoinType::Right,
            table: table.to_string(),
            left_column: left_column.to_string(),
            right_column: right_column.to_string(),
        }
    }

    /// Creates a FULL OUTER JOIN.
    pub fn full(table: &str, left_column: &str, right_column: &str) -> Self {
        Self {
            join_type: JoinType::Full,
            table: table.to_string(),
            left_column: left_column.to_string(),
            right_column: right_column.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_create_inner_join() {
        let join = Join::inner("posts", "id", "user");
        assert_eq!(join.join_type, JoinType::Inner);
        assert_eq!(join.table, "posts");
        assert_eq!(join.left_column, "id");
        assert_eq!(join.right_column, "user");
    }

    #[test]
    fn test_should_create_left_join() {
        let join = Join::left("posts", "id", "user");
        assert_eq!(join.join_type, JoinType::Left);
    }

    #[test]
    fn test_should_create_right_join() {
        let join = Join::right("posts", "id", "user");
        assert_eq!(join.join_type, JoinType::Right);
    }

    #[test]
    fn test_should_create_full_join() {
        let join = Join::full("posts", "id", "user");
        assert_eq!(join.join_type, JoinType::Full);
    }

    #[test]
    fn test_should_encode_decode_join_candid() {
        let join = Join::inner("posts", "users.id", "user");
        let encoded = candid::encode_one(&join).unwrap();
        let decoded: Join = candid::decode_one(&encoded).unwrap();
        assert_eq!(join, decoded);
    }

    #[test]
    fn test_should_encode_decode_join_type_candid() {
        for jt in [
            JoinType::Inner,
            JoinType::Left,
            JoinType::Right,
            JoinType::Full,
        ] {
            let encoded = candid::encode_one(&jt).unwrap();
            let decoded: JoinType = candid::decode_one(&encoded).unwrap();
            assert_eq!(jt, decoded);
        }
    }
}
