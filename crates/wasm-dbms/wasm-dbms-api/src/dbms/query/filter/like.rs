//! The like module provides the engine for matching strings against SQL LIKE patterns.

use std::str::FromStr;

use crate::prelude::{QueryError, QueryResult};

/// A marker struct representing the SQL LIKE operation.
#[derive(Debug, Clone, PartialEq)]
pub struct Like {
    /// The pattern to use for matching.
    pattern: Pattern,
}

/// A logical representation of a SQL LIKE pattern.
#[derive(Debug, Clone, PartialEq)]
struct Pattern {
    /// The tokens that make up the pattern.
    tokens: Vec<PatternToken>,
}

/// A token in a SQL LIKE [`Pattern`].
#[derive(Debug, Clone, PartialEq)]
enum PatternToken {
    /// A literal string segment.
    Literal(String),
    /// A wildcard for a single character. (Represents '_')
    WildcardSingle,
    /// A wildcard for multiple characters. (Represents '%')
    WildcardMulti,
}

impl FromStr for Pattern {
    type Err = QueryError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut tokens = Vec::new();
        let mut current_literal = String::new();
        let mut escape = false;

        for c in s.chars() {
            match c {
                '_' if !escape => {
                    if !current_literal.is_empty() {
                        tokens.push(PatternToken::Literal(std::mem::take(&mut current_literal)));
                    }
                    tokens.push(PatternToken::WildcardSingle);
                }
                '%' if !escape => {
                    if !current_literal.is_empty() {
                        tokens.push(PatternToken::Literal(std::mem::take(&mut current_literal)));
                    }
                    tokens.push(PatternToken::WildcardMulti);
                }
                '\\' if !escape => {
                    escape = true;
                }
                _ => {
                    current_literal.push(c);
                    escape = false;
                }
            }
        }

        // push remaining literal if any
        if !current_literal.is_empty() {
            tokens.push(PatternToken::Literal(current_literal));
        }

        Ok(Pattern { tokens })
    }
}

impl From<Pattern> for Like {
    fn from(pattern: Pattern) -> Self {
        Self { pattern }
    }
}

impl Like {
    /// Parses a SQL LIKE pattern into a [`Like`] struct.
    pub fn parse(pattern: impl AsRef<str>) -> QueryResult<Self> {
        let pattern = Pattern::from_str(pattern.as_ref())?;
        Ok(Self { pattern })
    }

    /// Returns whether the input string matches the LIKE pattern.
    ///
    /// Uses an iterative two-pointer algorithm with single-backtrack-point
    /// for O(n*m) worst-case, O(1) extra space, and zero heap allocation.
    pub fn matches(&self, input: impl AsRef<str>) -> bool {
        let input = input.as_ref();
        let tokens = &self.pattern.tokens;
        let mut ti = 0; // token index
        let mut ii = 0; // input byte offset
        let mut star_ti: Option<usize> = None; // token index to resume after backtrack
        let mut star_ii: usize = 0; // input byte offset to resume after backtrack

        while ii < input.len() {
            if ti < tokens.len() {
                match &tokens[ti] {
                    PatternToken::Literal(s) if input[ii..].starts_with(s.as_str()) => {
                        ii += s.len();
                        ti += 1;
                    }
                    PatternToken::WildcardSingle => {
                        ii += input[ii..].chars().next().unwrap().len_utf8();
                        ti += 1;
                    }
                    PatternToken::WildcardMulti => {
                        star_ti = Some(ti + 1);
                        star_ii = ii;
                        ti += 1;
                    }
                    _ => {
                        // mismatch — extend the last '%' match by one character
                        if let Some(s_ti) = star_ti
                            && let Some(c) = input[star_ii..].chars().next()
                        {
                            star_ii += c.len_utf8();
                            ii = star_ii;
                            ti = s_ti;
                        } else {
                            return false;
                        }
                    }
                }
            } else if let Some(s_ti) = star_ti {
                if s_ti >= tokens.len() {
                    // trailing '%' matches everything remaining
                    return true;
                }
                if let Some(c) = input[star_ii..].chars().next() {
                    star_ii += c.len_utf8();
                    ii = star_ii;
                    ti = s_ti;
                } else {
                    return false;
                }
            } else {
                return false;
            }
        }

        // skip trailing WildcardMulti tokens
        while ti < tokens.len() {
            if matches!(&tokens[ti], PatternToken::WildcardMulti) {
                ti += 1;
            } else {
                break;
            }
        }

        ti == tokens.len()
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_should_parse_only_literal_pattern() {
        let pattern = Like::parse("hello").expect("failed to parse pattern");
        assert_eq!(
            pattern.pattern.tokens,
            vec![PatternToken::Literal("hello".to_string())]
        );
    }

    #[test]
    fn test_should_parse_pattern_with_wildcards() {
        let pattern = Like::parse("h_llo%world").expect("failed to parse pattern");
        assert_eq!(
            pattern.pattern.tokens,
            vec![
                PatternToken::Literal("h".to_string()),
                PatternToken::WildcardSingle,
                PatternToken::Literal("llo".to_string()),
                PatternToken::WildcardMulti,
                PatternToken::Literal("world".to_string()),
            ]
        );
    }

    #[test]
    fn test_should_parse_pattern_with_escaped_characters() {
        let pattern = Like::parse("h\\_llo\\%world\\\\").expect("failed to parse pattern");
        assert_eq!(
            pattern.pattern.tokens,
            vec![PatternToken::Literal("h_llo%world\\".to_string())]
        );
    }

    #[test]
    fn test_should_match_literal() {
        let pattern = Like::parse("hello").expect("failed to parse pattern");
        assert!(pattern.matches("hello"));
        assert!(!pattern.matches("Hello"));
    }

    #[test]
    fn test_should_not_match_empty_string_with_single_wildcard() {
        let pattern = Like::parse("_").expect("failed to parse pattern");
        assert!(!pattern.matches(""));
    }

    #[test]
    fn test_should_match_single_character_with_single_wildcard() {
        let pattern = Like::parse("_").expect("failed to parse pattern");
        assert!(pattern.matches("a"));
        assert!(pattern.matches("1"));
        assert!(!pattern.matches("ab"));
    }

    #[test]
    fn test_should_match_a_string_with_single_wildcard() {
        let pattern = Like::parse("h_llo").expect("failed to parse pattern");
        assert!(pattern.matches("hello"));
        assert!(pattern.matches("hallo"));
        assert!(!pattern.matches("hllo"));
    }

    #[test]
    fn test_should_match_any_string_with_multi_wildcard() {
        let pattern = Like::parse("h%o").expect("failed to parse pattern");
        assert!(pattern.matches("ho"));
        assert!(pattern.matches("hello"));
        assert!(pattern.matches("h123o"));
        assert!(!pattern.matches("h"));
        assert!(!pattern.matches("hello world"));
        assert!(!pattern.matches("helle"));
    }

    #[test]
    fn test_should_match_complex_pattern() {
        let pattern = Like::parse("h%o_w%rld_").expect("failed to parse pattern");
        assert!(pattern.matches("hello world!"));
        assert!(pattern.matches("h123o_w456rld!"));
        assert!(!pattern.matches("h123o_w456rd"));
    }

    #[test]
    fn test_should_match_consecutive_wildcards() {
        let pattern = Like::parse("h%%o").expect("failed to parse pattern");
        assert!(pattern.matches("ho"));
        assert!(pattern.matches("hello"));
        assert!(pattern.matches("h123o"));
        assert!(!pattern.matches("h"));
    }

    #[test]
    fn test_should_match_consecutive_single_and_multi_wildcards() {
        let pattern = Like::parse("h%_o").expect("failed to parse pattern");
        assert!(pattern.matches("hxo"));
        assert!(pattern.matches("hello"));
        assert!(pattern.matches("h123o"));
        assert!(!pattern.matches("h"));
        assert!(!pattern.matches("ho"));
    }

    #[test]
    fn test_should_match_consecutive_single_and_multi_wildcards_with_escape() {
        let pattern = Like::parse("h\\%_o%").expect("failed to parse pattern");
        assert!(pattern.matches("h%xo"));
        assert!(pattern.matches("h%lo!"));
        assert!(pattern.matches("h%ao"));
        assert!(!pattern.matches("h"));
        assert!(!pattern.matches("ho"));
        assert!(!pattern.matches("h%o"));
    }

    #[test]
    fn test_should_match_multibyte_characters() {
        // literal match with multi-byte chars
        let pattern = Like::parse("caf\u{00e9}").expect("failed to parse pattern");
        assert!(pattern.matches("caf\u{00e9}"));
        assert!(!pattern.matches("cafe"));

        // single wildcard should match one multi-byte char
        let pattern = Like::parse("caf_").expect("failed to parse pattern");
        assert!(pattern.matches("caf\u{00e9}"));
        assert!(pattern.matches("cafe"));
        assert!(!pattern.matches("caf"));
        assert!(!pattern.matches("caf\u{00e9}!"));

        // multi wildcard with emoji
        let pattern = Like::parse("%\u{1f600}%").expect("failed to parse pattern");
        assert!(pattern.matches("\u{1f600}"));
        assert!(pattern.matches("hello \u{1f600} world"));
        assert!(!pattern.matches("hello world"));

        // mixed wildcards with multi-byte chars
        let pattern = Like::parse("_\u{00e9}%\u{1f600}_").expect("failed to parse pattern");
        assert!(pattern.matches("b\u{00e9}er\u{1f600}!"));
        assert!(pattern.matches("c\u{00e9}\u{1f600}x"));
        assert!(!pattern.matches("\u{00e9}\u{1f600}x"));

        // consecutive emoji
        let pattern = Like::parse("\u{1f600}_\u{1f600}").expect("failed to parse pattern");
        assert!(pattern.matches("\u{1f600}\u{1f60d}\u{1f600}"));
        assert!(!pattern.matches("\u{1f600}\u{1f600}"));
    }
}
