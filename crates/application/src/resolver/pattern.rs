// SPDX-License-Identifier: MIT OR Apache-2.0

//! Name patterns of `svggraph` as the Zabbix 7.4 frontend evaluates them
//! (P1-IN-02): compared with the visible name, case-insensitive, `*` is the
//! only wildcard and a pattern without `*` matches the whole name, not a
//! substring.

/// `true` when the pattern contains the wildcard `*`.
#[must_use]
pub fn is_mask(pattern: &str) -> bool {
    pattern.contains('*')
}

/// Whether `name` matches `pattern` under the 7.4 frontend rules. An empty
/// pattern matches nothing.
#[must_use]
pub fn name_pattern_matches(pattern: &str, name: &str) -> bool {
    if pattern.is_empty() {
        return false;
    }
    let pattern = pattern.to_lowercase();
    let name = name.to_lowercase();
    let mut segments = pattern.split('*');
    let first = segments.next().unwrap_or_default();
    let Some(mut rest) = name.strip_prefix(first) else {
        return false;
    };
    let tail: Vec<&str> = segments.collect();
    let Some((last, middle)) = tail.split_last() else {
        // No `*`: the whole name must match.
        return rest.is_empty();
    };
    for segment in middle {
        match rest.find(segment) {
            Some(position) => rest = &rest[position + segment.len()..],
            None => return false,
        }
    }
    rest.ends_with(last)
}

/// Case-insensitive equality used for exact host patterns.
#[must_use]
pub fn same_visible_name(pattern: &str, name: &str) -> bool {
    !is_mask(pattern) && name_pattern_matches(pattern, name)
}

#[cfg(test)]
mod tests {
    use super::{is_mask, name_pattern_matches, same_visible_name};

    #[test]
    fn patterns_follow_frontend_rules() {
        assert!(name_pattern_matches("pg-a-01", "PG-A-01"));
        assert!(
            !name_pattern_matches("pg-a", "pg-a-01"),
            "no substring match"
        );
        assert!(name_pattern_matches("pg-a-*", "pg-a-01"));
        assert!(name_pattern_matches("*", "anything"));
        assert!(name_pattern_matches("*-01", "pg-a-01"));
        assert!(name_pattern_matches("pg*a*01", "pg-a-01"));
        assert!(!name_pattern_matches("pg*b*01", "pg-a-01"));
        assert!(!name_pattern_matches("pg-a-*-x", "pg-a-01"));
        assert!(name_pattern_matches("CPU*", "cpu utilization"));
        assert!(!name_pattern_matches("", "pg-a-01"));
        assert!(
            !name_pattern_matches("pg_a_01", "pg-a-01"),
            "only * is a wildcard"
        );
        assert!(is_mask("pg-*"));
        assert!(same_visible_name("Synthetic DB 1", "synthetic db 1"));
        assert!(!same_visible_name("synthetic*", "synthetic db 1"));
    }
}
