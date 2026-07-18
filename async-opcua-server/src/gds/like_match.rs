//! Server-side evaluator for the OPC UA "LIKE" wildcard syntax (OPC-10000-4 v1.05.07 §Table 120),
//! used by `QueryApplications`/`QueryServers`' `ApplicationName`/`ApplicationUri`/`ProductUri`
//! filters (Part 12 §6.5.10/§6.5.11 -- "Supports the syntax used by the LIKE FilterOperator
//! described in OPC 10000-4"). No existing evaluator exists anywhere in this crate to reuse
//! (`operand.rs`'s `like()` only *builds* a `FilterOperator::Like` ContentFilter element for a
//! client, it doesn't evaluate one) -- see `specs/108-gds-directory-app-registry/research.md` R3.
//!
//! Grammar (case-sensitive):
//! - `%` matches any string of zero or more characters.
//! - `_` matches any single character.
//! - `\` escapes the following character (`\\`, `\%`, `\_`) for literal interpretation.
//! - `[...]` matches any single character in the listed set/ranges (e.g. `[13-68]`, `[c-f]`).
//! - `[^...]` matches any single character NOT in the listed set/ranges.

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Literal(char),
    AnySingle,
    AnyMulti,
    Set(Vec<(char, char)>, bool),
}

fn parse_pattern(pattern: &str) -> Vec<Token> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut tokens = Vec::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' => {
                if i + 1 < chars.len() {
                    tokens.push(Token::Literal(chars[i + 1]));
                    i += 2;
                } else {
                    tokens.push(Token::Literal('\\'));
                    i += 1;
                }
            }
            '%' => {
                tokens.push(Token::AnyMulti);
                i += 1;
            }
            '_' => {
                tokens.push(Token::AnySingle);
                i += 1;
            }
            '[' => {
                let start = i + 1;
                let mut j = start;
                while j < chars.len() && chars[j] != ']' {
                    j += 1;
                }
                if j >= chars.len() {
                    // No closing bracket -- treat '[' as a literal rather than erroring; this
                    // matcher is used for user-supplied filter strings, not parsed OPC UA wire
                    // data, so a malformed pattern should just fail to match, not panic/reject.
                    tokens.push(Token::Literal('['));
                    i += 1;
                    continue;
                }
                let mut content = &chars[start..j];
                let negated = content.first() == Some(&'^');
                if negated {
                    content = &content[1..];
                }
                let mut ranges = Vec::new();
                let mut k = 0;
                while k < content.len() {
                    if k + 2 < content.len() && content[k + 1] == '-' {
                        ranges.push((content[k], content[k + 2]));
                        k += 3;
                    } else {
                        ranges.push((content[k], content[k]));
                        k += 1;
                    }
                }
                tokens.push(Token::Set(ranges, negated));
                i = j + 1;
            }
            c => {
                tokens.push(Token::Literal(c));
                i += 1;
            }
        }
    }
    tokens
}

fn token_matches(token: &Token, c: char) -> bool {
    match token {
        Token::Literal(l) => *l == c,
        Token::AnySingle => true,
        Token::AnyMulti => unreachable!("AnyMulti is handled by the caller, not per-character"),
        Token::Set(ranges, negated) => {
            let hit = ranges.iter().any(|(a, b)| *a <= c && c <= *b);
            hit != *negated
        }
    }
}

/// Returns whether `value` matches the OPC UA LIKE `pattern`. An empty `pattern` is not
/// meaningful here -- callers (per Part 12's own filter semantics) should skip applying a filter
/// entirely when the pattern string is empty, not call this function with one.
pub(crate) fn like_match(pattern: &str, value: &str) -> bool {
    let tokens = parse_pattern(pattern);
    let chars: Vec<char> = value.chars().collect();
    let (m, n) = (tokens.len(), chars.len());

    let mut pi = 0usize;
    let mut vi = 0usize;
    let mut star_pi: Option<usize> = None;
    let mut star_vi = 0usize;

    while vi < n {
        if pi < m && !matches!(tokens[pi], Token::AnyMulti) && token_matches(&tokens[pi], chars[vi])
        {
            pi += 1;
            vi += 1;
        } else if pi < m && matches!(tokens[pi], Token::AnyMulti) {
            star_pi = Some(pi);
            star_vi = vi;
            pi += 1;
        } else if let Some(sp) = star_pi {
            pi = sp + 1;
            star_vi += 1;
            vi = star_vi;
        } else {
            return false;
        }
    }

    while pi < m && matches!(tokens[pi], Token::AnyMulti) {
        pi += 1;
    }
    pi == m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_matches_any_string_of_zero_or_more_chars() {
        assert!(like_match("main%", "maintenance"));
        assert!(like_match("main%", "main"));
        assert!(!like_match("main%", "domain"));
        assert!(like_match("%en%", "entail"));
        assert!(like_match("%en%", "green"));
        assert!(like_match("%en%", "content"));
        assert!(!like_match("%en%", "banana"));
    }

    #[test]
    fn underscore_matches_any_single_char() {
        assert!(like_match("_ould", "would"));
        assert!(like_match("_ould", "could"));
        assert!(!like_match("_ould", "should")); // two chars before "ould"
    }

    #[test]
    fn escape_character_allows_literal_interpretation() {
        assert!(like_match(r"5[%]", "5%"));
        assert!(!like_match(r"5[%]", "5x"));
        assert!(like_match(r"5[_]", "5_"));
        assert!(!like_match(r"5[_]", "5x"));
    }

    #[test]
    fn bracket_list_matches_any_single_char_in_a_range() {
        assert!(like_match("abc[13-68]", "abc1"));
        assert!(like_match("abc[13-68]", "abc3"));
        assert!(like_match("abc[13-68]", "abc4"));
        assert!(like_match("abc[13-68]", "abc5"));
        assert!(like_match("abc[13-68]", "abc6"));
        assert!(like_match("abc[13-68]", "abc8"));
        assert!(!like_match("abc[13-68]", "abc2"));
        assert!(!like_match("abc[13-68]", "abc7"));
        assert!(!like_match("abc[13-68]", "abc9"));

        assert!(like_match("xyz[c-f]", "xyzc"));
        assert!(like_match("xyz[c-f]", "xyzd"));
        assert!(like_match("xyz[c-f]", "xyze"));
        assert!(like_match("xyz[c-f]", "xyzf"));
        assert!(!like_match("xyz[c-f]", "xyzb"));
        assert!(!like_match("xyz[c-f]", "xyzg"));
    }

    #[test]
    fn negated_bracket_list_excludes_a_range() {
        assert!(!like_match("ABC[^13-5]", "ABC1"));
        assert!(!like_match("ABC[^13-5]", "ABC3"));
        assert!(!like_match("ABC[^13-5]", "ABC4"));
        assert!(!like_match("ABC[^13-5]", "ABC5"));
        assert!(like_match("ABC[^13-5]", "ABC2"));
        assert!(like_match("ABC[^13-5]", "ABC9"));

        assert!(!like_match("xyz[^dgh]", "xyzd"));
        assert!(!like_match("xyz[^dgh]", "xyzg"));
        assert!(!like_match("xyz[^dgh]", "xyzh"));
        assert!(like_match("xyz[^dgh]", "xyza"));
    }

    #[test]
    fn wildcard_characters_combine_in_a_single_pattern() {
        // OPC-10000-4 §Table 119's own worked example for combining wildcards.
        assert!(like_match("Th[ia][ts]%", "That is fine"));
        assert!(like_match("Th[ia][ts]%", "This is fine"));
        assert!(like_match("Th[ia][ts]%", "That as one"));
        assert!(like_match("Th[ia][ts]%", "This it is"));
        assert!(!like_match("Th[ia][ts]%", "The one"));
    }

    #[test]
    fn matching_is_case_sensitive() {
        assert!(!like_match("main%", "Maintenance"));
    }

    #[test]
    fn exact_literal_pattern_requires_exact_match() {
        assert!(like_match("urn:example:app", "urn:example:app"));
        assert!(!like_match("urn:example:app", "urn:example:app2"));
        assert!(!like_match("urn:example:app", "urn:example:ap"));
    }
}
