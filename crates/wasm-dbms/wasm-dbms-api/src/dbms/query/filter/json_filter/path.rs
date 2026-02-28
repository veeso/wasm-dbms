//! JSON path parsing utilities.
//!
//! This module provides functionality for parsing JSON paths in dot notation
//! with bracket array indices (e.g., `user.items[0].name`).

use crate::dbms::query::{QueryError, QueryResult};

/// Represents a segment in a JSON path.
///
/// A JSON path is composed of a sequence of segments, where each segment
/// is either a key (for accessing object fields) or an index (for accessing
/// array elements).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathSegment {
    /// An object field key.
    Key(String),
    /// An array index.
    Index(usize),
}

/// Parses a JSON path string into a sequence of path segments.
///
/// # Path Syntax
///
/// Paths use dot notation with bracket array indices:
///
/// | Path | Meaning |
/// |------|---------|
/// | `"name"` | Root-level field `name` |
/// | `"user.name"` | Nested field `user.name` |
/// | `"items[0]"` | First element of `items` array |
/// | `"users[0].name"` | `name` field of first user |
/// | `"data[0][1]"` | Nested array access |
/// | `"[0]"` | First element of root array |
///
/// # Errors
///
/// Returns `QueryError::InvalidQuery` for invalid paths:
/// - Empty path: `""`
/// - Trailing dot: `"user."`
/// - Unclosed bracket: `"items[0"`
/// - Empty brackets: `"items[]"`
/// - Negative index: `"items[-1]"`
/// - Non-numeric index: `"items[abc]"`
///
/// # Examples
///
/// ```ignore
/// use wasm_dbms_api::dbms::query::filter::json_filter::path::{parse_path, PathSegment};
///
/// let segments = parse_path("user.items[0].name").unwrap();
/// assert_eq!(segments, vec![
///     PathSegment::Key("user".to_string()),
///     PathSegment::Key("items".to_string()),
///     PathSegment::Index(0),
///     PathSegment::Key("name".to_string()),
/// ]);
/// ```
pub fn parse_path(path: &str) -> QueryResult<Vec<PathSegment>> {
    if path.is_empty() {
        return Err(QueryError::InvalidQuery("Empty JSON path".to_string()));
    }

    let mut segments = Vec::new();
    let mut chars = path.chars().peekable();
    let mut current_key = String::new();

    while let Some(ch) = chars.next() {
        match ch {
            '.' => {
                // Flush the current key if we have one
                if !current_key.is_empty() {
                    segments.push(PathSegment::Key(current_key.clone()));
                    current_key.clear();
                } else if segments.is_empty() {
                    // Leading dot is invalid
                    return Err(QueryError::InvalidQuery(
                        "JSON path cannot start with '.'".to_string(),
                    ));
                }
                // Check for trailing or consecutive dots
                if chars.peek().is_none() {
                    return Err(QueryError::InvalidQuery(
                        "JSON path cannot end with '.'".to_string(),
                    ));
                }
                if chars.peek() == Some(&'.') {
                    return Err(QueryError::InvalidQuery(
                        "JSON path cannot have consecutive '.'".to_string(),
                    ));
                }
            }
            '[' => {
                // Flush the current key if we have one
                if !current_key.is_empty() {
                    segments.push(PathSegment::Key(current_key.clone()));
                    current_key.clear();
                }

                // Parse the index inside brackets
                let mut index_str = String::new();
                let mut found_closing = false;

                for bracket_ch in chars.by_ref() {
                    if bracket_ch == ']' {
                        found_closing = true;
                        break;
                    }
                    index_str.push(bracket_ch);
                }

                if !found_closing {
                    return Err(QueryError::InvalidQuery(
                        "Unclosed bracket in JSON path".to_string(),
                    ));
                }

                if index_str.is_empty() {
                    return Err(QueryError::InvalidQuery(
                        "Empty brackets in JSON path".to_string(),
                    ));
                }

                // Check for negative index
                if index_str.starts_with('-') {
                    return Err(QueryError::InvalidQuery(
                        "Negative array index in JSON path".to_string(),
                    ));
                }

                // Parse as usize
                let index: usize = index_str.parse().map_err(|_| {
                    QueryError::InvalidQuery(format!(
                        "Invalid array index '{}' in JSON path",
                        index_str
                    ))
                })?;

                segments.push(PathSegment::Index(index));
            }
            ']' => {
                return Err(QueryError::InvalidQuery(
                    "Unexpected ']' in JSON path".to_string(),
                ));
            }
            _ => {
                current_key.push(ch);
            }
        }
    }

    // Flush any remaining key
    if !current_key.is_empty() {
        segments.push(PathSegment::Key(current_key));
    }

    if segments.is_empty() {
        return Err(QueryError::InvalidQuery("Empty JSON path".to_string()));
    }

    Ok(segments)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===== Valid Path Tests =====

    #[test]
    fn test_parse_simple_key() {
        let segments = parse_path("name").unwrap();
        assert_eq!(segments, vec![PathSegment::Key("name".to_string())]);
    }

    #[test]
    fn test_parse_nested_keys() {
        let segments = parse_path("user.name").unwrap();
        assert_eq!(
            segments,
            vec![
                PathSegment::Key("user".to_string()),
                PathSegment::Key("name".to_string()),
            ]
        );
    }

    #[test]
    fn test_parse_deeply_nested_keys() {
        let segments = parse_path("a.b.c.d").unwrap();
        assert_eq!(
            segments,
            vec![
                PathSegment::Key("a".to_string()),
                PathSegment::Key("b".to_string()),
                PathSegment::Key("c".to_string()),
                PathSegment::Key("d".to_string()),
            ]
        );
    }

    #[test]
    fn test_parse_array_index() {
        let segments = parse_path("items[0]").unwrap();
        assert_eq!(
            segments,
            vec![PathSegment::Key("items".to_string()), PathSegment::Index(0),]
        );
    }

    #[test]
    fn test_parse_array_index_large() {
        let segments = parse_path("items[999]").unwrap();
        assert_eq!(
            segments,
            vec![
                PathSegment::Key("items".to_string()),
                PathSegment::Index(999),
            ]
        );
    }

    #[test]
    fn test_parse_mixed_path() {
        let segments = parse_path("users[0].name").unwrap();
        assert_eq!(
            segments,
            vec![
                PathSegment::Key("users".to_string()),
                PathSegment::Index(0),
                PathSegment::Key("name".to_string()),
            ]
        );
    }

    #[test]
    fn test_parse_nested_array_access() {
        let segments = parse_path("data[0][1]").unwrap();
        assert_eq!(
            segments,
            vec![
                PathSegment::Key("data".to_string()),
                PathSegment::Index(0),
                PathSegment::Index(1),
            ]
        );
    }

    #[test]
    fn test_parse_root_array_access() {
        let segments = parse_path("[0]").unwrap();
        assert_eq!(segments, vec![PathSegment::Index(0)]);
    }

    #[test]
    fn test_parse_root_nested_array_access() {
        let segments = parse_path("[0][1]").unwrap();
        assert_eq!(segments, vec![PathSegment::Index(0), PathSegment::Index(1)]);
    }

    #[test]
    fn test_parse_complex_path() {
        let segments = parse_path("users[0].addresses[1].city").unwrap();
        assert_eq!(
            segments,
            vec![
                PathSegment::Key("users".to_string()),
                PathSegment::Index(0),
                PathSegment::Key("addresses".to_string()),
                PathSegment::Index(1),
                PathSegment::Key("city".to_string()),
            ]
        );
    }

    #[test]
    fn test_parse_key_with_underscores() {
        let segments = parse_path("user_name").unwrap();
        assert_eq!(segments, vec![PathSegment::Key("user_name".to_string())]);
    }

    #[test]
    fn test_parse_key_with_numbers() {
        let segments = parse_path("field1.field2").unwrap();
        assert_eq!(
            segments,
            vec![
                PathSegment::Key("field1".to_string()),
                PathSegment::Key("field2".to_string()),
            ]
        );
    }

    // ===== Invalid Path Tests =====

    #[test]
    fn test_parse_empty_path() {
        let result = parse_path("");
        assert!(result.is_err());
        assert!(matches!(result, Err(QueryError::InvalidQuery(_))));
    }

    #[test]
    fn test_parse_trailing_dot() {
        let result = parse_path("user.");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, QueryError::InvalidQuery(msg) if msg.contains("end with")));
    }

    #[test]
    fn test_parse_leading_dot() {
        let result = parse_path(".user");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, QueryError::InvalidQuery(msg) if msg.contains("start with")));
    }

    #[test]
    fn test_parse_consecutive_dots() {
        let result = parse_path("user..name");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, QueryError::InvalidQuery(msg) if msg.contains("consecutive")));
    }

    #[test]
    fn test_parse_unclosed_bracket() {
        let result = parse_path("items[0");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, QueryError::InvalidQuery(msg) if msg.contains("Unclosed")));
    }

    #[test]
    fn test_parse_empty_brackets() {
        let result = parse_path("items[]");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, QueryError::InvalidQuery(msg) if msg.contains("Empty brackets")));
    }

    #[test]
    fn test_parse_negative_index() {
        let result = parse_path("items[-1]");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, QueryError::InvalidQuery(msg) if msg.contains("Negative")));
    }

    #[test]
    fn test_parse_non_numeric_index() {
        let result = parse_path("items[abc]");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, QueryError::InvalidQuery(msg) if msg.contains("Invalid array index"))
        );
    }

    #[test]
    fn test_parse_unexpected_closing_bracket() {
        let result = parse_path("items]0[");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, QueryError::InvalidQuery(msg) if msg.contains("Unexpected")));
    }

    #[test]
    fn test_parse_float_index() {
        let result = parse_path("items[1.5]");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, QueryError::InvalidQuery(msg) if msg.contains("Invalid array index"))
        );
    }
}
