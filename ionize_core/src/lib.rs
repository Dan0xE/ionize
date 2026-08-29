// SPDX-License-Identifier: MPL-2.0

//! Layout and edge routing for cfg's.

#![no_std]

extern crate alloc;

mod err;
mod geom;
mod graph;
mod layout;

pub use err::{LayoutErr, OptErr, OptField, Result};
pub use geom::{Point, Rect, Size};
pub use graph::{Flags, Graph, Id, Node};
pub use layout::{Arrow, Cmd, Layout, Opts, Path, Placed, layout};
