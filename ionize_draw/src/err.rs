// SPDX-License-Identifier: MPL-2.0

use alloc::string::String;

use thiserror::Error;

#[allow(missing_docs)] // since this is self-explanatory
pub type Result<T> = core::result::Result<T, Err>;

/// An error raised while drawing a diagram.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Err {
    /// The graph could not be laid out.
    #[error(transparent)]
    Layout(#[from] ionize::LayoutErr),
    /// The diagram contains invalid data.
    #[error("{0}")]
    Invalid(String),
}

impl Err {
    pub(crate) fn new(msg: impl Into<String>) -> Self {
        Self::Invalid(msg.into())
    }
}
