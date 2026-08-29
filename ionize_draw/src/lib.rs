// SPDX-License-Identifier: MPL-2.0

//! SVG rendering for [`ionize`] graphs.
//!
//! This crate is `no_std`, but requires `alloc`. A [`Diagram`] pairs each node
//! with the block at the same index.
//!
//! ```
//! use ionize::{Graph, Node, Opts, Size};
//! use ionize_draw::{Block, Cell, Col, Diagram, Renderer, Row, Table};
//!
//! fn table(text: &'static str) -> Table<'static> {
//!     let mut table = Table::new([Col::normal(0.0)]);
//!     table.push(Row::new([Cell::new(text)]));
//!     table
//! }
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut graph = Graph::new();
//! let entry = graph.add(Node::new(Size::new(0.0, 0.0)));
//! let exit = graph.add(Node::new(Size::new(0.0, 0.0)));
//! graph.connect(entry, exit);
//!
//! let blocks = vec![
//!     Block::new("Entry", table("start")),
//!     Block::new("Exit", table("return")),
//! ];
//! let diagram = Diagram::new(graph, blocks)?.title("Two nodes");
//! let mut renderer = Renderer::new()?;
//! let svg = renderer.render(diagram, &Opts::default())?;
//! assert!(svg.starts_with("<?xml"));
//! # Ok(())
//! # }
//! ```
//!
//! Text is measured before it is rendered. Custom CSS must not change its font
//! or geometry!!.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

mod err;
mod fmt;
mod font;
mod model;
mod svg;

pub use err::{Err, Result};
pub use model::{
    Align, Attr, Block, Cell, Col, Diagram, Field, Label, Meta, Row, Run, Table, Text,
};
pub use svg::Renderer;
