// SPDX-License-Identifier: MIT OR Apache-2.0

use std::{cmp::Ordering, fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub prerelease: Option<String>,
}

impl Version {
    #[must_use]
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
            prerelease: None,
        }
    }
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("invalid version {0:?}")]
pub struct VersionParseError(String);

impl FromStr for Version {
    type Err = VersionParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let bytes = value.as_bytes();
        let Some(first_dot) = value.find('.') else {
            return Err(VersionParseError(value.to_owned()));
        };
        let major = parse_component(&value[..first_dot], value, true)?;
        let remainder = &value[first_dot + 1..];
        let minor_digits = remainder.bytes().take_while(u8::is_ascii_digit).count();
        if minor_digits == 0 {
            return Err(VersionParseError(value.to_owned()));
        }
        let minor = parse_component(&remainder[..minor_digits], value, false)?;
        let mut cursor = first_dot + 1 + minor_digits;
        let mut patch = 0;
        if bytes.get(cursor) == Some(&b'.') {
            cursor += 1;
            let patch_digits = bytes[cursor..]
                .iter()
                .take_while(|byte| byte.is_ascii_digit())
                .count();
            if patch_digits == 0 {
                return Err(VersionParseError(value.to_owned()));
            }
            patch = parse_component(&value[cursor..cursor + patch_digits], value, false)?;
            cursor += patch_digits;
        }
        let prerelease = if cursor == value.len() {
            None
        } else {
            let suffix = &value[cursor..];
            if !suffix.as_bytes()[0].is_ascii_alphabetic()
                || !suffix
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
            {
                return Err(VersionParseError(value.to_owned()));
            }
            Some(suffix.to_owned())
        };
        Ok(Self {
            major,
            minor,
            patch,
            prerelease,
        })
    }
}

fn parse_component(
    value: &str,
    original: &str,
    reject_zero: bool,
) -> Result<u32, VersionParseError> {
    if value.is_empty()
        || !value.bytes().all(|byte| byte.is_ascii_digit())
        || (reject_zero && value.starts_with('0'))
    {
        return Err(VersionParseError(original.to_owned()));
    }
    value
        .parse()
        .map_err(|_| VersionParseError(original.to_owned()))
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| match (&self.prerelease, &other.prerelease) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(left), Some(right)) => left.cmp(right),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(prerelease) = &self.prerelease {
            formatter.write_str(prerelease)?;
        }
        Ok(())
    }
}

impl Serialize for Version {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Version {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VersionRange {
    pub minimum: Version,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maximum_exclusive: Option<Version>,
}

impl VersionRange {
    #[must_use]
    pub fn contains(&self, version: &Version) -> bool {
        version >= &self.minimum
            && self
                .maximum_exclusive
                .as_ref()
                .is_none_or(|maximum| version < maximum)
    }

    #[must_use]
    pub fn is_non_empty(&self) -> bool {
        self.maximum_exclusive
            .as_ref()
            .is_none_or(|maximum| self.minimum < *maximum)
    }

    #[must_use]
    pub fn intersection(&self, other: &Self) -> Option<Self> {
        let minimum = self.minimum.clone().max(other.minimum.clone());
        let maximum_exclusive = match (&self.maximum_exclusive, &other.maximum_exclusive) {
            (Some(left), Some(right)) => Some(left.clone().min(right.clone())),
            (Some(value), None) | (None, Some(value)) => Some(value.clone()),
            (None, None) => None,
        };
        let intersection = Self {
            minimum,
            maximum_exclusive,
        };
        intersection.is_non_empty().then_some(intersection)
    }
}

#[cfg(test)]
mod tests {
    use super::{Version, VersionRange};

    #[test]
    fn omitted_patch_is_canonical_and_equal() {
        let short: Version = "7.4".parse().unwrap_or_else(|error| panic!("{error}"));
        let full: Version = "7.4.0".parse().unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(short, full);
        assert_eq!(short.to_string(), "7.4.0");
    }

    #[test]
    fn range_is_lower_inclusive_and_upper_exclusive() {
        let range = VersionRange {
            minimum: "7.4".parse().unwrap_or_else(|error| panic!("{error}")),
            maximum_exclusive: Some("8.0".parse().unwrap_or_else(|error| panic!("{error}"))),
        };
        assert!(range.is_non_empty());
        assert!(range.contains(&"7.4.0".parse().unwrap_or_else(|error| panic!("{error}"))));
        assert!(!range.contains(&"8.0.0".parse().unwrap_or_else(|error| panic!("{error}"))));
    }

    #[test]
    fn ranges_intersect_without_inventing_an_upper_bound() {
        let seven_four_plus = VersionRange {
            minimum: Version::new(7, 4, 0),
            maximum_exclusive: None,
        };
        let bounded = VersionRange {
            minimum: Version::new(7, 4, 16),
            maximum_exclusive: Some(Version::new(8, 0, 0)),
        };
        assert_eq!(seven_four_plus.intersection(&bounded), Some(bounded));
    }
}
