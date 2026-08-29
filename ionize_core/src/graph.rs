// SPDX-License-Identifier: MPL-2.0

use alloc::vec::Vec;
use core::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, Index, IndexMut};

use crate::Size;

/// Node index assigned by [`Graph::add`].
///
/// Note that an `Id` identifies a position, not a particular graph. Using it with a
/// different [`Graph`] accesses the node at the same index.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Id(usize);

impl Id {
    pub(crate) const fn from_index(index: usize) -> Self {
        Self(index)
    }

    /// Returns the node index.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0
    }
}

/// Properties that alter a node's layout role.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Flags(u8);

impl Flags {
    /// You're not special.
    pub const NONE: Self = Self(0);
    /// A loop header.
    pub const HEADER: Self = Self(1 << 0);
    /// A backedge block that returns to a loop header.
    pub const BACKEDGE: Self = Self(1 << 1);
    /// An on-stack-replacement entry.
    pub const OSR: Self = Self(1 << 2);

    /// Returns whether all `other` flags are present.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Returns whether any `other` flag is present.
    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

impl BitOr for Flags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for Flags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl BitAnd for Flags {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        Self(self.0 & rhs.0)
    }
}

impl BitAndAssign for Flags {
    fn bitand_assign(&mut self, rhs: Self) {
        self.0 &= rhs.0;
    }
}

/// A layout node and its graph connections.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    /// Width and height used by the layout.
    pub size: Size,
    /// Declared loop nesting depth.
    pub depth: usize,
    /// Roles that affect layout.
    pub flags: Flags,
    /// Predecessor blocks in input order.
    ///
    /// Each predecessor appears once, even when it has parallel edges to this node.
    /// [`Graph::connect`] maintains this convention.
    pub preds: Vec<Id>,
    /// Successor edges in input port order.
    ///
    /// An ID may appear more than once when there are parallel edges.
    pub succs: Vec<Id>,
}

impl Node {
    /// Creates a node without edges.
    #[must_use]
    pub const fn new(size: Size) -> Self {
        Self {
            size,
            depth: 0,
            flags: Flags::NONE,
            preds: Vec::new(),
            succs: Vec::new(),
        }
    }
}

/// A collection of layout nodes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Graph {
    nodes: Vec<Node>,
}

impl Graph {
    /// Creates an empty graph.
    #[must_use]
    pub const fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    /// Appends a node and returns its index.
    pub fn add(&mut self, node: Node) -> Id {
        let id = Id(self.nodes.len());
        self.nodes.push(node);
        id
    }

    /// Adds an edge from `from` to `to`.
    ///
    /// Successors preserve every edge in insertion order. Predecessors keep each
    /// source only once.
    ///
    /// # Panics
    ///
    /// Panics if either index is out of range. IDs are not tied to a graph, so an
    /// ID from another graph is accepted when the same index exists here.
    #[track_caller]
    pub fn connect(&mut self, from: Id, to: Id) {
        let len = self.nodes.len();
        assert!(from.0 < len, "source ID is out of range");
        assert!(to.0 < len, "destination ID is out of range");

        self.nodes[from.0].succs.push(to);
        let preds = &mut self.nodes[to.0].preds;
        if preds.last() != Some(&from) && !preds.contains(&from) {
            preds.push(from);
        }
    }

    /// Returns the node at `id`, if it is in range.
    #[must_use]
    pub fn get(&self, id: Id) -> Option<&Node> {
        self.nodes.get(id.0)
    }

    /// Returns the node at `id` mutably, if it is in range.
    pub fn get_mut(&mut self, id: Id) -> Option<&mut Node> {
        self.nodes.get_mut(id.0)
    }

    /// Returns the number of nodes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Returns whether the graph contains no nodes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Iterates over identifiers in input order.
    pub fn ids(&self) -> impl ExactSizeIterator<Item = Id> + DoubleEndedIterator {
        (0..self.nodes.len()).map(Id)
    }

    pub(crate) fn nodes(&self) -> &[Node] {
        &self.nodes
    }
}

impl Index<Id> for Graph {
    type Output = Node;

    fn index(&self, id: Id) -> &Self::Output {
        &self.nodes[id.0]
    }
}

impl IndexMut<Id> for Graph {
    fn index_mut(&mut self, id: Id) -> &mut Self::Output {
        &mut self.nodes[id.0]
    }
}
