// SPDX-License-Identifier: MPL-2.0

use std::fmt;

pub type Result<T> = std::result::Result<T, Err>;

#[derive(Debug)]
pub struct Err(String);

impl Err {
    pub fn new(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }
}

impl fmt::Display for Err {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Err {}
