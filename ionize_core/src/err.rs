// SPDX-License-Identifier: MPL-2.0

use crate::Id;
use thiserror::Error;

#[allow(missing_docs)] // since this is self-explanatory
pub type Result<T> = core::result::Result<T, LayoutErr>;

/// Layout option named by [`LayoutErr::InvalidOpt`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum OptField {
    /// [`crate::Opts::padding`].
    Padding,
    /// [`crate::Opts::block_gap`].
    BlockGap,
    /// [`crate::Opts::first_port`].
    FirstPort,
    /// [`crate::Opts::port_gap`].
    PortGap,
    /// [`crate::Opts::bend_radius`].
    BendRadius,
    /// [`crate::Opts::track_padding`].
    TrackPadding,
    /// [`crate::Opts::joint_gap`].
    JointGap,
    /// [`crate::Opts::header_drop`].
    HeaderDrop,
    /// [`crate::Opts::near_straight`].
    NearStraight,
    /// [`crate::Opts::line_width`].
    LineWidth,
    /// [`crate::Opts::arrow_size`].
    ArrowSize,
}

impl OptField {
    const fn name(self) -> &'static str {
        match self {
            Self::Padding => "padding",
            Self::BlockGap => "block_gap",
            Self::FirstPort => "first_port",
            Self::PortGap => "port_gap",
            Self::BendRadius => "bend_radius",
            Self::TrackPadding => "track_padding",
            Self::JointGap => "joint_gap",
            Self::HeaderDrop => "header_drop",
            Self::NearStraight => "near_straight",
            Self::LineWidth => "line_width",
            Self::ArrowSize => "arrow_size",
        }
    }
}

/// Why a layout option is invalid.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum OptErr {
    /// The value is NaN or infinite.
    #[error("must be finite")]
    NonFinite,
    /// The finite value is less than zero.
    #[error("must be non-negative")]
    Negative,
    /// The value is less than or equal to the named option.
    #[error("must be greater than `{}`", .0.name())]
    NotGreaterThan(OptField),
}

/// Reason an input graph could not be laid out.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum LayoutErr {
    /// An adjacency entry does not identify a node in this graph.
    #[error("node {} refers to invalid node {}", .node.index(), .edge.index())]
    InvalidId {
        /// Node with the invalid adjacency entry.
        node: Id,
        /// Out-of-range neighbor.
        edge: Id,
    },
    /// A block has non-finite or negative dimensions.
    #[error("node {} has an invalid size", .0.index())]
    InvalidGeom(Id),
    /// A layout option is invalid.
    #[error("layout option `{}` {err}", .field.name())]
    InvalidOpt {
        /// Option that failed validation.
        field: OptField,
        /// Validation failure.
        err: OptErr,
    },
    /// Predecessor and successor lists disagree.
    #[error("edge from node {} to node {} is not reciprocal", .from.index(), .to.index())]
    NonReciprocal {
        /// Source node.
        from: Id,
        /// Destination node.
        to: Id,
    },
    /// A loop header or its reconstructed loop ancestry is malformed.
    #[error("node {} is a malformed loop header", .0.index())]
    MalformedHeader(Id),
    /// A backedge does not uniquely return to a loop header.
    #[error("node {} is a malformed backedge", .0.index())]
    MalformedBackedge(Id),
    /// An OSR entry cannot be used as a layout root.
    #[error("node {} is a malformed OSR entry", .0.index())]
    MalformedOsr(Id),
    /// A non-empty graph has no predecessor-free node.
    #[error("non-empty graph has no layout root")]
    NoRoot,
    /// No layout root can reach this node.
    #[error("node {} is unreachable", .0.index())]
    Unreachable(Id),
    /// A cycle does not terminate at a flgged backedge block.
    #[error("unmarked cycle reaches node {}", .0.index())]
    UnmarkedCycle(Id),
    /// Layout exceeded a coordinate or collection-size limit.
    #[error("layout exceeded a coordinate or collection-size limit")]
    Overflow,
}
