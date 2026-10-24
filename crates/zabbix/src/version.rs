// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::{Result, ZabbixError};
use std::{fmt, str::FromStr};

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct ZabbixVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl ZabbixVersion {
    pub const fn is_supported_7_4(&self) -> bool {
        self.major == 7 && self.minor == 4
    }
}

impl FromStr for ZabbixVersion {
    type Err = ZabbixError;
    fn from_str(value: &str) -> Result<Self> {
        if value.is_empty()
            || !value
                .chars()
                .all(|character| character.is_ascii_digit() || character == '.')
        {
            return Err(ZabbixError::InvalidVersion(value.to_owned()));
        }
        let mut components = value.split('.');
        let major = take_number(&mut components, value)?;
        let minor = take_number(&mut components, value)?;
        let patch = match components.next() {
            Some(component) if !component.is_empty() => component
                .parse()
                .map_err(|_| ZabbixError::InvalidVersion(value.to_owned()))?,
            Some(_) => return Err(ZabbixError::InvalidVersion(value.to_owned())),
            None => 0,
        };
        if components.next().is_some() {
            return Err(ZabbixError::InvalidVersion(value.to_owned()));
        }
        Ok(Self {
            major,
            minor,
            patch,
        })
    }
}

fn take_number<'a>(components: &mut impl Iterator<Item = &'a str>, original: &str) -> Result<u32> {
    components
        .next()
        .filter(|component| !component.is_empty())
        .ok_or_else(|| ZabbixError::InvalidVersion(original.to_owned()))?
        .parse()
        .map_err(|_| ZabbixError::InvalidVersion(original.to_owned()))
}

impl fmt::Display for ZabbixVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}
