// SPDX-License-Identifier: MPL-2.0

use alloc::{vec, vec::Vec};
use core::ops::Range;

use crate::{Flags, Graph, Id, LayoutErr, OptErr, OptField, Point, Rect, Result, Size};

/// Controls spacing, routing, and straightening.
///
/// Every floating-point field must be finite and non-negative, and
/// [`Self::track_padding`] must exceed [`Self::bend_radius`].
///
/// Note it's on you to ensure that custom values still leave room for strokes, curves, or arrowheads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Opts {
    /// Empty space around the graph.
    pub padding: f64,
    /// Minimum horizontal gap between neighboring blocks.
    pub block_gap: f64,
    /// Horizontal offset of the first edge port.
    pub first_port: f64,
    /// Horizontal distance between edge ports.
    pub port_gap: f64,
    /// Radius of rounded edge bends.
    pub bend_radius: f64,
    /// Vertical padding above and below edge tracks.
    ///
    /// This **must** exceed [`Self::bend_radius`].
    pub track_padding: f64,
    /// Vertical spacing between horizontal edge routes that would otherwise overlap.
    pub joint_gap: f64,
    /// Vertical offset of loop-header rails.
    pub header_drop: f64,
    /// Number of broad horizontal alignment passes.
    pub layout_iters: usize,
    /// Largest horizontal offset corrected by a comb pass.
    pub near_straight: f64,
    /// Number of up-and-down comb oass iterations.
    pub comb_iters: usize,
    /// Edge stroke width, also used for alignment.
    pub line_width: f64,
    /// Arrowhead radius.
    pub arrow_size: f64,
}

impl Default for Opts {
    fn default() -> Self {
        Self {
            padding: 20.0,
            block_gap: 44.0,
            first_port: 16.0,
            port_gap: 60.0,
            bend_radius: 12.0,
            track_padding: 36.0,
            joint_gap: 16.0,
            header_drop: 16.0,
            layout_iters: 2,
            near_straight: 30.0,
            comb_iters: 8,
            line_width: 1.0,
            arrow_size: 5.0,
        }
    }
}

/// The placed nodes and edges produced by [`layout`].
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    /// Nodes in the same order as the input graph.
    pub nodes: Vec<Placed>,
    /// Pieces of edges, in drawing order.
    pub paths: Vec<Path>,
    /// Commands referenced by [`Self::paths`].
    pub cmds: Vec<Cmd>,
    /// Canvas size, including outer padding.
    ///
    /// With custom options, strokes, arrowheads, and curves may extend past it.
    pub bounds: Size,
}

/// Where an input node ended up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    /// The input node.
    pub id: Id,
    /// Its rectangle in layout coordinates.
    pub rect: Rect,
}

/// One drawable piece of an edge.
#[derive(Clone, Debug, PartialEq)]
pub struct Path {
    /// Range selecting this piece from [`Layout::cmds`].
    pub cmds: Range<usize>,
    /// Arrowhead at the end of this piece, if it has one.
    pub arrow: Option<Arrow>,
    /// Stroke width this piece was laid out for.
    pub width: f64,
}

/// A drawing command for an edge path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cmd {
    /// Moves to a point without drawing.
    Move(Point),
    /// Draws a line to a point.
    Line(Point),
    /// Draws a circular arc to `to`.
    Arc {
        /// Arc radius.
        r: f64,
        /// Direction, using SVG's sweep convention.
        sweep: bool,
        /// Where the arc ends.
        to: Point,
    },
    /// Draws a cubic Bézier curve to `to`.
    Cubic {
        /// First control point.
        a: Point,
        /// Second control point.
        b: Point,
        /// Where the curve ends.
        to: Point,
    },
}

/// Placement and size of an arrowhead.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arrow {
    /// Position of the tip.
    pub tip: Point,
    /// Clockwise rotation in degrees.
    pub rot: f64,
    /// Scale of the triangle.
    pub size: f64,
}

/// Lays out a graph and routes its edges.
///
/// # Errors
///
/// Returns [`LayoutErr`] if the graph is malformed, an option is invalid, or
/// the resulting geometry overflows.
pub fn layout(graph: &Graph, opts: &Opts) -> Result<Layout> {
    validate(graph, opts)?;
    if graph.is_empty() {
        return Ok(Layout {
            nodes: Vec::new(),
            paths: Vec::new(),
            cmds: Vec::new(),
            bounds: Size::default(),
        });
    }

    Engine::new(graph, opts).run()
}

fn validate(graph: &Graph, opts: &Opts) -> Result<()> {
    let nums = [
        (OptField::Padding, opts.padding),
        (OptField::BlockGap, opts.block_gap),
        (OptField::FirstPort, opts.first_port),
        (OptField::PortGap, opts.port_gap),
        (OptField::BendRadius, opts.bend_radius),
        (OptField::TrackPadding, opts.track_padding),
        (OptField::JointGap, opts.joint_gap),
        (OptField::HeaderDrop, opts.header_drop),
        (OptField::NearStraight, opts.near_straight),
        (OptField::LineWidth, opts.line_width),
        (OptField::ArrowSize, opts.arrow_size),
    ];
    for (field, value) in nums {
        let err = if !value.is_finite() {
            Some(OptErr::NonFinite)
        } else if value < 0.0 {
            Some(OptErr::Negative)
        } else {
            None
        };
        if let Some(err) = err {
            return Err(LayoutErr::InvalidOpt { field, err });
        }
    }
    if opts.track_padding <= opts.bend_radius {
        return Err(LayoutErr::InvalidOpt {
            field: OptField::TrackPadding,
            err: OptErr::NotGreaterThan(OptField::BendRadius),
        });
    }

    let len = graph.len();
    for (i, node) in graph.nodes().iter().enumerate() {
        let id = Id::from_index(i);
        if !node.size.w.is_finite()
            || !node.size.h.is_finite()
            || node.size.w < 0.0
            || node.size.h < 0.0
        {
            return Err(LayoutErr::InvalidGeom(id));
        }
        for &edge in node.preds.iter().chain(&node.succs) {
            if edge.index() >= len {
                return Err(LayoutErr::InvalidId { node: id, edge });
            }
        }
        if node.flags.contains(Flags::HEADER) && node.flags.contains(Flags::BACKEDGE) {
            return Err(LayoutErr::MalformedHeader(id));
        }
        if node.flags.contains(Flags::OSR) && node.preds.is_empty() && node.succs.is_empty() {
            return Err(LayoutErr::MalformedOsr(id));
        }
    }

    for (i, node) in graph.nodes().iter().enumerate() {
        let id = Id::from_index(i);
        for &to in &node.succs {
            if !graph[to].preds.contains(&id) {
                return Err(LayoutErr::NonReciprocal { from: id, to });
            }
        }
        for &from in &node.preds {
            if !graph[from].succs.contains(&id) {
                return Err(LayoutErr::NonReciprocal { from, to: id });
            }
        }
    }

    for (i, node) in graph.nodes().iter().enumerate() {
        let id = Id::from_index(i);
        if node.flags.contains(Flags::HEADER) {
            let backs = node
                .preds
                .iter()
                .filter(|&&pred| graph[pred].flags.contains(Flags::BACKEDGE))
                .count();
            if backs != 1 {
                return Err(LayoutErr::MalformedHeader(id));
            }
        }
        if node.flags.contains(Flags::BACKEDGE)
            && (node.succs.len() != 1 || !graph[node.succs[0]].flags.contains(Flags::HEADER))
        {
            return Err(LayoutErr::MalformedBackedge(id));
        }
    }

    detect_cycles(graph)
}

fn detect_cycles(graph: &Graph) -> Result<()> {
    let nodes = graph.nodes();
    let mut color = vec![0_u8; nodes.len()];
    let mut stack = Vec::with_capacity(nodes.len());
    for root in 0..nodes.len() {
        if color[root] != 0 {
            continue;
        }
        color[root] = 1;
        stack.push((root, 0_usize));
        while let Some((node, next)) = stack.pop() {
            let current = &nodes[node];
            if current.flags.contains(Flags::BACKEDGE) || next == current.succs.len() {
                color[node] = 2;
                continue;
            }
            stack.push((node, next + 1));
            let dst = current.succs[next].index();
            let state = color[dst];
            match state {
                0 => {
                    color[dst] = 1;
                    stack.push((dst, 0));
                }
                1 => return Err(LayoutErr::UnmarkedCycle(Id::from_index(dst))),
                _ => {}
            }
        }
    }
    Ok(())
}

const LEFT_DUMMY: u8 = 1 << 0;
const RIGHT_DUMMY: u8 = 1 << 1;
const NEXT_BACKEDGE: u8 = 1 << 2;
const DUMMY: u8 = 1 << 3;
const NO_DST: usize = usize::MAX;

struct Block<'a> {
    size: Size,
    depth: usize,
    flags: Flags,
    preds: &'a [Id],
    succs: &'a [Id],
    layer: isize,
    loop_id: Option<usize>,
    loop_height: usize,
    parent: Option<usize>,
    outgoing: Vec<usize>,
    pseudo: bool,
    backedge: Option<usize>,
    layout_node: Option<usize>,
}

impl Block<'_> {
    fn is_header(&self) -> bool {
        self.flags.contains(Flags::HEADER)
    }

    fn is_backedge(&self) -> bool {
        self.flags.contains(Flags::BACKEDGE)
    }

    fn is_loop(&self) -> bool {
        self.is_header() || self.pseudo
    }
}

#[derive(Clone, Copy)]
struct Link {
    dst: usize,
    joint: f64,
}

enum Srcs {
    Inline(Option<usize>),
    Heap(Vec<usize>),
}

impl Srcs {
    fn new(cap: usize) -> Self {
        if cap > 1 {
            Self::Heap(Vec::with_capacity(cap))
        } else {
            Self::Inline(None)
        }
    }

    fn as_slice(&self) -> &[usize] {
        match self {
            Self::Inline(None) => &[],
            Self::Inline(Some(id)) => core::slice::from_ref(id),
            Self::Heap(srcs) => srcs,
        }
    }

    fn add(&mut self, id: usize) {
        let srcs = self.as_slice();
        if srcs.last() == Some(&id) || srcs.contains(&id) {
            return;
        }
        match self {
            Self::Inline(slot @ None) => *slot = Some(id),
            Self::Inline(Some(first)) => *self = Self::Heap(vec![*first, id]),
            Self::Heap(srcs) => srcs.push(id),
        }
    }

    fn remove(&mut self, id: usize) -> bool {
        match self {
            Self::Inline(slot) if *slot == Some(id) => {
                *slot = None;
                true
            }
            Self::Inline(_) => false,
            Self::Heap(srcs) => {
                let Some(pos) = srcs.iter().position(|&src| src == id) else {
                    return false;
                };
                srcs.remove(pos);
                true
            }
        }
    }
}

struct LNode {
    pos: Point,
    size: Size,
    block: usize,
    src: Srcs,
    links: Range<usize>,
    flags: u8,
}

impl LNode {
    fn is_dummy(&self) -> bool {
        self.flags & DUMMY != 0
    }
}

#[derive(Clone, Copy)]
struct Edge {
    src: usize,
    port: usize,
    dst_block: usize,
}

struct Engine<'a> {
    opts: &'a Opts,
    blocks: Vec<Block<'a>>,
    nodes: Vec<LNode>,
    links: Vec<Link>,
    layers: Vec<Vec<usize>>,
}

impl<'a> Engine<'a> {
    fn new(graph: &'a Graph, opts: &'a Opts) -> Self {
        let mut blocks = Vec::with_capacity(graph.len());
        for node in graph.nodes() {
            let backedge = if node.flags.contains(Flags::HEADER) {
                node.preds
                    .iter()
                    .copied()
                    .find(|&id| graph[id].flags.contains(Flags::BACKEDGE))
                    .map(Id::index)
            } else {
                None
            };
            blocks.push(Block {
                size: node.size,
                depth: node.depth,
                flags: node.flags,
                preds: node.preds.as_slice(),
                succs: node.succs.as_slice(),
                layer: -1,
                loop_id: None,
                loop_height: 0,
                parent: None,
                outgoing: Vec::new(),
                pseudo: false,
                backedge,
                layout_node: None,
            });
        }
        Self {
            opts,
            blocks,
            nodes: Vec::new(),
            links: Vec::new(),
            layers: Vec::new(),
        }
    }

    fn run(mut self) -> Result<Layout> {
        let (roots, osr) = self.find_roots()?;
        for &root in roots.iter().chain(&osr) {
            self.blocks[root].pseudo = true;
        }
        self.find_loops(&roots)?;
        for (i, block) in self.blocks.iter().enumerate() {
            if block.loop_id.is_none() && !osr.contains(&i) {
                return Err(LayoutErr::Unreachable(Id::from_index(i)));
            }
        }
        self.set_layers(&roots)?;
        for &id in &osr {
            self.blocks[id].layer = 0;
            self.blocks[id].loop_id = Some(id);
            if self.blocks[id]
                .succs
                .iter()
                .any(|succ| self.blocks[succ.index()].layer <= 0)
            {
                return Err(LayoutErr::MalformedOsr(Id::from_index(id)));
            }
        }
        self.make_nodes()?;
        self.straighten();
        let tracks = self.joints();
        let heights = self.vertical(&tracks);
        self.finish(&heights, &tracks)
    }

    fn find_roots(&self) -> Result<(Vec<usize>, Vec<usize>)> {
        let roots: Vec<_> = self
            .blocks
            .iter()
            .enumerate()
            .filter_map(|(i, block)| block.preds.is_empty().then_some(i))
            .collect();
        if roots.is_empty() {
            return Err(LayoutErr::NoRoot);
        }
        if !roots
            .iter()
            .any(|&root| self.blocks[root].flags.contains(Flags::OSR))
        {
            return Ok((roots, Vec::new()));
        }

        let mut new_roots = Vec::new();
        let mut osr = Vec::new();
        for root in roots {
            let mut new_root = root;
            if self.blocks[root].flags.contains(Flags::OSR) {
                let post_osr = self.blocks[root].succs[0].index();
                new_root = post_osr;
                loop {
                    let pred = self.blocks[new_root]
                        .preds
                        .iter()
                        .copied()
                        .find(|&id| {
                            !self.blocks[id.index()]
                                .flags
                                .intersects(Flags::OSR | Flags::BACKEDGE)
                        })
                        .map(Id::index);
                    let Some(pred) = pred else { break };
                    new_root = pred;
                }
                if new_root == post_osr {
                    new_root = root;
                } else {
                    osr.push(root);
                }
            }
            if !new_roots.contains(&new_root) {
                new_roots.push(new_root);
            }
        }
        Ok((new_roots, osr))
    }

    fn find_loops(&mut self, roots: &[usize]) -> Result<()> {
        #[derive(Clone, Copy)]
        struct LoopState {
            id: usize,
            depth: usize,
        }

        let mut stack = Vec::new();
        for &root in roots {
            stack.push((root, LoopState { id: root, depth: 0 }));
            while let Some((id, mut state)) = stack.pop() {
                if self.blocks[id].loop_id.is_some() {
                    continue;
                }
                if self.blocks[id].is_header() {
                    let parent = state.id;
                    if parent == id {
                        return Err(LayoutErr::MalformedHeader(Id::from_index(id)));
                    }
                    self.blocks[id].parent = Some(parent);
                    state = LoopState {
                        id,
                        depth: state.depth + 1,
                    };
                }
                if self.blocks[id].depth > state.depth {
                    self.blocks[id].depth = state.depth;
                }
                while self.blocks[id].depth < state.depth {
                    state.id = self.blocks[state.id]
                        .parent
                        .expect("nested loop has no parent");
                    state.depth -= 1;
                }
                self.blocks[id].loop_id = Some(state.id);
                if !self.blocks[id].is_backedge() {
                    for &succ in self.blocks[id].succs.iter().rev() {
                        stack.push((succ.index(), state));
                    }
                }
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn set_layers(&mut self, roots: &[usize]) -> Result<()> {
        enum Phase {
            Enter,
            Succ(usize),
            Out(usize),
        }
        struct Frame {
            id: usize,
            layer: isize,
            phase: Phase,
        }

        let mut stack = Vec::with_capacity(self.blocks.len());
        for &root in roots {
            stack.push(Frame {
                id: root,
                layer: 0,
                phase: Phase::Enter,
            });
            while let Some(frame) = stack.pop() {
                match frame.phase {
                    Phase::Enter => {
                        if self.blocks[frame.id].is_backedge() {
                            let header = self.blocks[frame.id].succs[0].index();
                            self.blocks[frame.id].layer = self.blocks[header].layer;
                            continue;
                        }
                        if frame.layer <= self.blocks[frame.id].layer {
                            continue;
                        }
                        self.blocks[frame.id].layer = frame.layer;
                        let mut loop_id = self.blocks[frame.id].loop_id;
                        let mut seen = 0;
                        while let Some(header) = loop_id {
                            assert!(
                                self.blocks[header].is_loop() && seen <= self.blocks.len(),
                                "invalid loop parent chain"
                            );
                            let height = frame.layer - self.blocks[header].layer + 1;
                            assert!(height >= 0, "loop member precedes its header");
                            let height =
                                usize::try_from(height).map_err(|_| LayoutErr::Overflow)?;
                            self.blocks[header].loop_height =
                                self.blocks[header].loop_height.max(height);
                            loop_id = self.blocks[header].parent;
                            seen += 1;
                        }
                        stack.push(Frame {
                            id: frame.id,
                            layer: frame.layer,
                            phase: Phase::Succ(0),
                        });
                    }
                    Phase::Succ(next) => {
                        let succ = self.blocks[frame.id]
                            .succs
                            .get(next)
                            .copied()
                            .map(Id::index);
                        if let Some(succ) = succ {
                            stack.push(Frame {
                                id: frame.id,
                                layer: frame.layer,
                                phase: Phase::Succ(next + 1),
                            });
                            if self.blocks[succ].depth < self.blocks[frame.id].depth {
                                let header = self.blocks[frame.id]
                                    .loop_id
                                    .expect("reachable block has no loop");
                                assert!(self.blocks[header].is_loop(), "loop ID is not a loop");
                                self.blocks[header].outgoing.push(succ);
                            } else {
                                stack.push(Frame {
                                    id: succ,
                                    layer: frame.layer + 1,
                                    phase: Phase::Enter,
                                });
                            }
                        } else if self.blocks[frame.id].is_header() {
                            stack.push(Frame {
                                id: frame.id,
                                layer: frame.layer,
                                phase: Phase::Out(0),
                            });
                        }
                    }
                    Phase::Out(next) => {
                        if let Some(&succ) = self.blocks[frame.id].outgoing.get(next) {
                            let height = isize::try_from(self.blocks[frame.id].loop_height)
                                .map_err(|_| LayoutErr::Overflow)?;
                            let layer =
                                frame.layer.checked_add(height).ok_or(LayoutErr::Overflow)?;
                            stack.push(Frame {
                                id: frame.id,
                                layer: frame.layer,
                                phase: Phase::Out(next + 1),
                            });
                            stack.push(Frame {
                                id: succ,
                                layer,
                                phase: Phase::Enter,
                            });
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn add_node(&mut self, block: usize, size: Size, flags: u8) -> usize {
        let id = self.nodes.len();
        let (src_cap, dst_len) = if flags & DUMMY != 0 {
            (1, 1)
        } else {
            (
                self.blocks[block].preds.len(),
                self.blocks[block].succs.len(),
            )
        };
        let start = self.links.len();
        self.links.resize(
            start + dst_len,
            Link {
                dst: NO_DST,
                joint: 0.0,
            },
        );
        self.nodes.push(LNode {
            pos: Point::new(self.opts.padding, self.opts.padding),
            size,
            block,
            src: Srcs::new(src_cap),
            links: start..self.links.len(),
            flags,
        });
        id
    }

    fn connect(&mut self, from: usize, port: usize, to: usize) {
        let range = self.nodes[from].links.clone();
        self.links[range][port].dst = to;
        self.nodes[to].src.add(from);
    }

    #[allow(clippy::too_many_lines)]
    fn make_nodes(&mut self) -> Result<()> {
        self.nodes.reserve(self.blocks.len());
        let link_cap = self
            .blocks
            .iter()
            .try_fold(0usize, |count, block| count.checked_add(block.succs.len()));
        self.links.reserve(link_cap.ok_or(LayoutErr::Overflow)?);

        let max_layer = self
            .blocks
            .iter()
            .map(|block| block.layer)
            .max()
            .expect("non-empty graph has no blocks");
        assert!(max_layer >= 0, "graph has no populated layer");
        let count = usize::try_from(max_layer + 1).map_err(|_| LayoutErr::Overflow)?;
        let mut blocks_by_layer = vec![Vec::new(); count];
        for (id, block) in self.blocks.iter().enumerate() {
            let layer = usize::try_from(block.layer)
                .map_err(|_| LayoutErr::Unreachable(Id::from_index(id)))?;
            blocks_by_layer[layer].push(id);
        }
        if blocks_by_layer.iter().any(Vec::is_empty) {
            blocks_by_layer.retain(|layer| !layer.is_empty());
        }

        self.layers = vec![Vec::new(); blocks_by_layer.len()];
        let mut active: Vec<Edge> = Vec::with_capacity(self.blocks.len());
        let mut latest: Vec<(usize, usize)> = Vec::new();
        let mut terminating: Vec<Edge> = Vec::new();
        let mut dummies: Vec<(usize, usize)> = Vec::new();
        let mut pending: Vec<(usize, usize)> = Vec::new();
        let mut back_edges = Vec::new();
        for (layer, blocks) in blocks_by_layer.into_iter().enumerate() {
            terminating.clear();
            dummies.clear();
            pending.clear();
            for &block in &blocks {
                let mut i = active.len();
                while i > 0 {
                    i -= 1;
                    if active[i].dst_block == block {
                        let edge = active.remove(i);
                        terminating.push(edge);
                    }
                }
            }

            for edge in &mut active {
                let dummy = if let Some((_, id)) = dummies
                    .iter()
                    .find(|(dst, _)| *dst == edge.dst_block)
                    .copied()
                {
                    self.connect(edge.src, edge.port, id);
                    id
                } else {
                    let id = self.add_node(edge.dst_block, Size::default(), DUMMY);
                    self.connect(edge.src, edge.port, id);
                    self.layers[layer].push(id);
                    dummies.push((edge.dst_block, id));
                    id
                };
                edge.src = dummy;
                edge.port = 0;
            }

            for &block in &blocks {
                let mut header = self.blocks[block]
                    .loop_id
                    .expect("reachable block has no loop");
                while self.blocks[header].is_header() {
                    if let Some((_, rightmost)) =
                        pending.iter_mut().find(|(loop_id, _)| *loop_id == header)
                    {
                        *rightmost = block;
                    } else {
                        pending.push((header, block));
                    }
                    let Some(parent) = self.blocks[header].parent else {
                        break;
                    };
                    header = parent;
                }
            }

            let more = blocks
                .len()
                .checked_add(pending.len())
                .ok_or(LayoutErr::Overflow)?;
            self.layers[layer].reserve(more);

            for &block in &blocks {
                let node = self.add_node(block, self.blocks[block].size, 0);
                for edge in terminating.iter().rev() {
                    if edge.dst_block == block {
                        self.connect(edge.src, edge.port, node);
                    }
                }
                self.layers[layer].push(node);
                self.blocks[block].layout_node = Some(node);

                for &(loop_id, rightmost) in &pending {
                    if rightmost != block {
                        continue;
                    }
                    let backedge = self.blocks[loop_id]
                        .backedge
                        .expect("loop header has no backedge");
                    let dummy = self.add_node(backedge, Size::default(), DUMMY);
                    let latest_index = latest.iter().position(|(id, _)| *id == backedge);
                    if let Some(index) = latest_index {
                        let prior = latest[index].1;
                        self.connect(dummy, 0, prior);
                    } else {
                        let dst = self.blocks[backedge]
                            .layout_node
                            .ok_or(LayoutErr::MalformedBackedge(Id::from_index(backedge)))?;
                        self.nodes[dummy].flags |= NEXT_BACKEDGE;
                        self.connect(dummy, 0, dst);
                    }
                    self.layers[layer].push(dummy);
                    if let Some(index) = latest_index {
                        latest[index].1 = dummy;
                    } else {
                        latest.push((backedge, dummy));
                    }
                }

                if self.blocks[block].is_backedge() {
                    let header = self.blocks[block].succs[0].index();
                    let dst = self.blocks[header]
                        .layout_node
                        .ok_or(LayoutErr::MalformedBackedge(Id::from_index(block)))?;
                    self.connect(node, 0, dst);
                } else {
                    for (port, &succ) in self.blocks[block].succs.iter().enumerate() {
                        let succ = succ.index();
                        let edge = Edge {
                            src: node,
                            port,
                            dst_block: succ,
                        };
                        if self.blocks[succ].is_backedge() {
                            back_edges.push(edge);
                        } else {
                            active.push(edge);
                        }
                    }
                }
            }
            for edge in back_edges.drain(..) {
                let dummy = latest
                    .iter()
                    .find(|(id, _)| *id == edge.dst_block)
                    .map(|(_, node)| *node)
                    .ok_or(LayoutErr::MalformedBackedge(Id::from_index(edge.dst_block)))?;
                self.connect(edge.src, edge.port, dummy);
            }
        }

        if let Some(edge) = active.first() {
            return Err(self.unterminated(edge.dst_block));
        }
        self.prune();
        self.mark_dummies();
        #[cfg(debug_assertions)]
        self.check_nodes();
        Ok(())
    }

    #[cold]
    fn unterminated(&self, dst: usize) -> LayoutErr {
        let header = self.blocks[dst]
            .preds
            .iter()
            .copied()
            .map(Id::index)
            .find(|&pred| {
                !self.blocks[pred].is_backedge()
                    && self.blocks[pred].layer >= self.blocks[dst].layer
            })
            .and_then(|pred| self.blocks[pred].loop_id)
            .or(self.blocks[dst].loop_id)
            .unwrap_or(dst);
        LayoutErr::MalformedHeader(Id::from_index(header))
    }

    fn prune(&mut self) {
        let mut roots = Vec::new();
        for layer in &self.layers {
            for &id in layer {
                let node = &self.nodes[id];
                if node.is_dummy()
                    && self.blocks[node.block].is_backedge()
                    && node.src.as_slice().is_empty()
                {
                    roots.push(id);
                }
            }
        }
        if roots.is_empty() {
            return;
        }
        let mut removed = vec![false; self.nodes.len()];
        for orphan in roots {
            let mut current = orphan;
            while self.nodes[current].is_dummy() && self.nodes[current].src.as_slice().is_empty() {
                let range = self.nodes[current].links.clone();
                assert_eq!(range.len(), 1, "dummy does not have one link");
                let dst = self.links[range.start].dst;
                assert_ne!(dst, NO_DST, "dummy link is not connected");
                assert!(
                    self.nodes[dst].src.remove(current),
                    "dummy link has no reciprocal source"
                );
                removed[current] = true;
                current = dst;
            }
        }
        for layer in &mut self.layers {
            layer.retain(|&id| !removed[id]);
        }
    }

    fn mark_dummies(&mut self) {
        for layer in &self.layers {
            for &id in layer {
                if self.nodes[id].is_dummy() {
                    self.nodes[id].flags |= LEFT_DUMMY;
                } else {
                    break;
                }
            }
            for &id in layer.iter().rev() {
                if self.nodes[id].is_dummy() {
                    self.nodes[id].flags |= RIGHT_DUMMY;
                } else {
                    break;
                }
            }
        }
    }

    #[cfg(debug_assertions)]
    fn check_nodes(&self) {
        for layer in &self.layers {
            for &id in layer {
                let node = &self.nodes[id];
                let expected = if node.is_dummy() {
                    1
                } else {
                    self.blocks[node.block].succs.len()
                };
                let links = &self.links[node.links.clone()];
                assert_eq!(links.len(), expected, "node has the wrong link count");
                assert!(
                    links.iter().all(|link| link.dst < self.nodes.len()),
                    "node link has no destination"
                );
            }
        }
    }

    fn dst(&self, node: usize, port: usize) -> usize {
        let node = &self.nodes[node];
        self.links[node.links.clone()][port].dst
    }

    fn push_neighbors(&mut self, layer: usize) {
        let ids = &self.layers[layer];
        let Some((&first, rest)) = ids.split_first() else {
            return;
        };
        if rest.is_empty() {
            return;
        }

        let first_port = self.opts.first_port;
        let block_gap = self.opts.block_gap;
        let nodes = &mut self.nodes;
        let (mut left_dummy, mut left_x, mut left_w) = {
            let left = &nodes[first];
            (left.is_dummy(), left.pos.x, left.size.w)
        };

        for &id in rest {
            let right = &mut nodes[id];
            let padding = if left_dummy && !right.is_dummy() {
                first_port
            } else {
                0.0
            };
            let x = left_x + left_w + padding + block_gap;
            right.pos.x = right.pos.x.max(x);

            left_dummy = right.is_dummy();
            left_x = right.pos.x;
            left_w = right.size.w;
        }
    }

    fn push_into_loops(&mut self) {
        for layer in 0..self.layers.len() {
            for index in 0..self.layers[layer].len() {
                let id = self.layers[layer][index];
                if self.nodes[id].is_dummy() {
                    continue;
                }
                let block = self.nodes[id].block;
                let header = self.blocks[block]
                    .loop_id
                    .expect("reachable block has no loop");
                let header_node = self.blocks[header]
                    .layout_node
                    .expect("loop has no layout node");
                self.nodes[id].pos.x = self.nodes[id].pos.x.max(self.nodes[header_node].pos.x);
            }
        }
    }

    fn straighten_dummies(&mut self, slots: &mut Vec<Option<f64>>, used: &mut Vec<usize>) {
        for layer in 0..self.layers.len() {
            for index in 0..self.layers[layer].len() {
                let id = self.layers[layer][index];
                if !self.nodes[id].is_dummy() {
                    continue;
                }
                let dst = self.nodes[id].block;
                let x = self.nodes[id].pos.x;
                if slots.is_empty() {
                    slots.resize(self.blocks.len(), None);
                }
                let slot = &mut slots[dst];
                if let Some(current) = slot {
                    *current = (*current).max(x);
                } else {
                    *slot = Some(x);
                    used.push(dst);
                }
            }
        }
        for layer in 0..self.layers.len() {
            for index in 0..self.layers[layer].len() {
                let id = self.layers[layer][index];
                if !self.nodes[id].is_dummy() {
                    continue;
                }
                let dst = self.nodes[id].block;
                self.nodes[id].pos.x = slots[dst].expect("dummy has no shared position");
            }
        }
        for layer in 0..self.layers.len() {
            self.push_neighbors(layer);
        }
        for &dst in used.iter() {
            slots[dst] = None;
        }
        used.clear();
    }

    fn suck_left_dummies(&mut self, slots: &mut Vec<Option<f64>>, used: &mut Vec<usize>) {
        for layer in 0..self.layers.len() {
            let mut split = 0;
            let mut next_x = 0.0;
            while split < self.layers[layer].len() {
                let id = self.layers[layer][split];
                if self.nodes[id].flags & LEFT_DUMMY == 0 {
                    next_x = self.nodes[id].pos.x;
                    break;
                }
                split += 1;
            }
            next_x -= self.opts.block_gap + self.opts.first_port;
            for index in (0..split).rev() {
                let id = self.layers[layer][index];
                let mut safe = next_x;
                for &src in self.nodes[id].src.as_slice() {
                    let range = self.nodes[src].links.clone();
                    let port = self.links[range]
                        .iter()
                        .position(|link| link.dst == id)
                        .expect("dummy source has no reciprocal link");
                    let x = self.nodes[src].pos.x + num(port) * self.opts.port_gap;
                    if x < safe {
                        safe = x;
                    }
                }
                self.nodes[id].pos.x = safe;
                next_x = safe - self.opts.block_gap;
                let dst = self.nodes[id].block;
                if slots.is_empty() {
                    slots.resize(self.blocks.len(), None);
                }
                let slot = &mut slots[dst];
                if let Some(current) = slot {
                    *current = (*current).min(safe);
                } else {
                    *slot = Some(safe);
                    used.push(dst);
                }
            }
        }
        for layer in 0..self.layers.len() {
            for index in 0..self.layers[layer].len() {
                let id = self.layers[layer][index];
                if !self.nodes[id].is_dummy() {
                    continue;
                }
                if self.nodes[id].flags & LEFT_DUMMY == 0 {
                    continue;
                }
                let dst = self.nodes[id].block;
                self.nodes[id].pos.x = slots[dst].expect("dummy has no shared position");
            }
        }
        for &dst in used.iter() {
            slots[dst] = None;
        }
        used.clear();
    }

    fn straighten_children(&mut self) {
        for layer in 0..self.layers.len().saturating_sub(1) {
            self.push_neighbors(layer);
            let mut last_shifted = None;
            for index in 0..self.layers[layer].len() {
                let node = self.layers[layer][index];
                for port in 0..self.nodes[node].links.len() {
                    let dst = self.dst(node, port);
                    let index = self.layers[layer + 1].iter().position(|&id| id == dst);
                    if index.is_some_and(|index| last_shifted.is_none_or(|last| index > last))
                        && self.nodes[dst].src.as_slice().first() == Some(&node)
                    {
                        let src_offset = self.opts.first_port + self.opts.port_gap * num(port);
                        let before = self.nodes[dst].pos.x;
                        let x = self.nodes[node].pos.x + src_offset - self.opts.first_port;
                        if x > before {
                            self.nodes[dst].pos.x = x;
                            last_shifted = index;
                        }
                    }
                }
            }
        }
    }

    fn straighten_conservative(&mut self) {
        let mut deltas = Vec::new();
        for layer in 0..self.layers.len() {
            for i in (0..self.layers[layer].len()).rev() {
                let node = self.layers[layer][i];
                if self.nodes[node].is_dummy() {
                    continue;
                }
                let block = self.nodes[node].block;
                if self.blocks[block].is_backedge() {
                    continue;
                }

                deltas.clear();
                for &parent in self.nodes[node].src.as_slice() {
                    let range = self.nodes[parent].links.clone();
                    let port = self.links[range]
                        .iter()
                        .position(|link| link.dst == node)
                        .expect("node source has no reciprocal link");
                    let src = self.opts.first_port + num(port) * self.opts.port_gap;
                    deltas.push(
                        self.nodes[parent].pos.x + src
                            - (self.nodes[node].pos.x + self.opts.first_port),
                    );
                }
                for port in 0..self.nodes[node].links.len() {
                    let dst = self.dst(node, port);
                    if self.nodes[dst].is_dummy()
                        && self.blocks[self.nodes[dst].block].is_backedge()
                    {
                        continue;
                    }
                    let src = self.opts.first_port + num(port) * self.opts.port_gap;
                    deltas.push(
                        self.nodes[dst].pos.x + self.opts.first_port
                            - (self.nodes[node].pos.x + src),
                    );
                }
                if deltas.contains(&0.0) {
                    continue;
                }
                deltas.retain(|delta| *delta > 0.0);
                deltas.sort_by(f64::total_cmp);
                for &delta in &deltas {
                    let mut overlaps = false;
                    for index in i + 1..self.layers[layer].len() {
                        let other = self.layers[layer][index];
                        if self.nodes[other].flags & RIGHT_DUMMY != 0 {
                            continue;
                        }
                        let a1 = self.nodes[node].pos.x + delta;
                        let a2 = a1 + self.nodes[node].size.w;
                        let b1 = self.nodes[other].pos.x - self.opts.block_gap;
                        let b2 = self.nodes[other].pos.x
                            + self.nodes[other].size.w
                            + self.opts.block_gap;
                        if a2 >= b1 && a1 <= b2 {
                            overlaps = true;
                        }
                    }
                    if !overlaps {
                        self.nodes[node].pos.x += delta;
                        break;
                    }
                }
            }
            self.push_neighbors(layer);
        }
    }

    fn comb_up(&mut self) {
        for layer in (0..self.layers.len()).rev() {
            self.push_neighbors(layer);
            for index in 0..self.layers[layer].len() {
                let node = self.layers[layer][index];
                let src_len = self.nodes[node].src.as_slice().len();
                for src_index in 0..src_len {
                    let src = self.nodes[node].src.as_slice()[src_index];
                    if !self.nodes[src].is_dummy() {
                        continue;
                    }
                    let wiggle = (self.nodes[src].pos.x - self.nodes[node].pos.x).abs();
                    if wiggle <= self.opts.near_straight {
                        let x = self.nodes[src].pos.x.max(self.nodes[node].pos.x);
                        self.nodes[src].pos.x = x;
                        self.nodes[node].pos.x = x;
                    }
                }
            }
        }
    }

    fn comb_down(&mut self) {
        for layer in 0..self.layers.len() {
            self.push_neighbors(layer);
            for index in 0..self.layers[layer].len() {
                let node = self.layers[layer][index];
                if self.nodes[node].links.is_empty() {
                    continue;
                }
                let dst = self.dst(node, 0);
                if !self.nodes[dst].is_dummy() {
                    continue;
                }
                let wiggle = (self.nodes[dst].pos.x - self.nodes[node].pos.x).abs();
                if wiggle <= self.opts.near_straight {
                    let x = self.nodes[dst].pos.x.max(self.nodes[node].pos.x);
                    self.nodes[dst].pos.x = x;
                    self.nodes[node].pos.x = x;
                }
            }
        }
    }

    fn straighten(&mut self) {
        let mut slots = Vec::new();
        let mut used = Vec::new();
        for _ in 0..self.opts.layout_iters {
            self.straighten_children();
            self.push_into_loops();
            self.straighten_dummies(&mut slots, &mut used);
        }
        self.straighten_dummies(&mut slots, &mut used);
        for _ in 0..self.opts.comb_iters {
            self.comb_up();
            self.comb_down();
        }
        self.straighten_conservative();
        self.straighten_dummies(&mut slots, &mut used);
        self.suck_left_dummies(&mut slots, &mut used);
    }

    fn joints(&mut self) -> Vec<f64> {
        #[derive(Clone, Copy)]
        struct Joint {
            x1: f64,
            x2: f64,
            link: usize,
            dst: usize,
        }

        let mut heights = Vec::with_capacity(self.layers.len());
        let mut joints = Vec::new();
        let mut right: Vec<Vec<Joint>> = Vec::new();
        let mut left: Vec<Vec<Joint>> = Vec::new();
        for layer in 0..self.layers.len() {
            let mut right_len = 0;
            let mut left_len = 0;
            for &node in &self.layers[layer] {
                if !self.nodes[node].is_dummy() && self.blocks[self.nodes[node].block].is_backedge()
                {
                    continue;
                }
                let link_start = self.nodes[node].links.start;
                for port in 0..self.nodes[node].links.len() {
                    let dst = self.dst(node, port);
                    let x1 = self.nodes[node].pos.x
                        + self.opts.first_port
                        + self.opts.port_gap * num(port);
                    let x2 = self.nodes[dst].pos.x + self.opts.first_port;
                    if (x2 - x1).abs() < 2.0 * self.opts.bend_radius {
                        continue;
                    }
                    joints.push(Joint {
                        x1,
                        x2,
                        link: link_start + port,
                        dst,
                    });
                }
            }
            joints.sort_by(|a, b| a.x1.total_cmp(&b.x1));

            for joint in joints.drain(..) {
                let (tracks, len) = if joint.x2 - joint.x1 >= 0.0 {
                    (&mut right, &mut right_len)
                } else {
                    (&mut left, &mut left_len)
                };
                let al = joint.x1.min(joint.x2);
                let ar = joint.x1.max(joint.x2);
                let mut merge = None;
                let mut last_valid = None;
                for i in (0..*len).rev() {
                    let mut overlaps = false;
                    for other in &tracks[i] {
                        if joint.dst == other.dst {
                            merge = Some(i);
                            break;
                        }
                        let bl = other.x1.min(other.x2);
                        let br = other.x1.max(other.x2);
                        if ar >= bl && al <= br {
                            overlaps = true;
                            break;
                        }
                    }
                    if merge.is_some() || overlaps {
                        break;
                    }
                    last_valid = Some(i);
                }
                if let Some(i) = merge.or(last_valid) {
                    tracks[i].push(joint);
                } else {
                    if *len == tracks.len() {
                        tracks.push(Vec::new());
                    }
                    tracks[*len].push(joint);
                    *len += 1;
                }
            }

            let track_count = right_len + left_len;
            let height = num(track_count.saturating_sub(1)) * self.opts.joint_gap;
            let mut offset = -height / 2.0;
            for track in right[..right_len].iter().rev().chain(&left[..left_len]) {
                for &joint in track {
                    self.links[joint.link].joint = offset;
                }
                offset += self.opts.joint_gap;
            }
            for track in right[..right_len].iter_mut().chain(&mut left[..left_len]) {
                track.clear();
            }
            heights.push(height);
        }
        heights
    }

    fn vertical(&mut self, tracks: &[f64]) -> Vec<f64> {
        assert_eq!(tracks.len(), self.layers.len(), "track count changed");
        let mut heights = Vec::with_capacity(self.layers.len());
        let mut next_y = self.opts.padding;
        for (layer, &track_height) in tracks.iter().enumerate() {
            let mut height: f64 = 0.0;
            for &node in &self.layers[layer] {
                self.nodes[node].pos.y = next_y;
                height = height.max(self.nodes[node].size.h);
            }
            heights.push(height);
            next_y += height + self.opts.track_padding + track_height + self.opts.track_padding;
        }
        heights
    }

    #[allow(clippy::too_many_lines)]
    fn finish(self, heights: &[f64], tracks: &[f64]) -> Result<Layout> {
        let mut max_x: f64 = 0.0;
        let mut max_y: f64 = 0.0;
        let mut path_count = 0usize;
        for layer in &self.layers {
            for &id in layer {
                let node = &self.nodes[id];
                max_x = max_x.max(node.pos.x + node.size.w + self.opts.padding);
                max_y = max_y.max(node.pos.y + node.size.h + self.opts.padding);
                path_count = path_count
                    .checked_add(node.links.len())
                    .ok_or(LayoutErr::Overflow)?;
            }
        }

        let cmd_cap = path_count.checked_mul(6).ok_or(LayoutErr::Overflow)?;
        let mut paths = Vec::with_capacity(path_count);
        let mut cmds = Vec::with_capacity(cmd_cap);
        let odd = odd_stroke(self.opts.line_width);
        for (layer, nodes) in self.layers.iter().enumerate() {
            for &id in nodes {
                let node = &self.nodes[id];
                let links = &self.links[node.links.clone()];
                for (port, &link) in links.iter().enumerate() {
                    let dst_id = link.dst;
                    let dst = &self.nodes[dst_id];
                    let x1 = node.pos.x + self.opts.first_port + self.opts.port_gap * num(port);
                    let y1 = node.pos.y + node.size.h;

                    let (path, x1, x2, y1, y2, mid) = match () {
                        () if !node.is_dummy() && self.blocks[node.block].is_backedge() => {
                            let block = node.block;
                            let header = self.blocks[block].succs[0].index();
                            let header_node = self.blocks[header]
                                .layout_node
                                .expect("header has no layout node");
                            let x1 = node.pos.x;
                            let y1 = node.pos.y + self.opts.header_drop;
                            let x2 = self.nodes[header_node].pos.x + self.blocks[header].size.w;
                            let y2 = self.nodes[header_node].pos.y + self.opts.header_drop;
                            (
                                loop_arrow(&mut cmds, x1, y1, x2, y2, odd, self.opts),
                                x1,
                                x2,
                                y1,
                                y2,
                                None,
                            )
                        }
                        () if node.flags & NEXT_BACKEDGE != 0 => {
                            assert!(
                                node.is_dummy() && !dst.is_dummy(),
                                "backedge marker is on the wrong nodes"
                            );
                            let x1 = node.pos.x + self.opts.first_port;
                            let y1 = node.pos.y + self.opts.header_drop + self.opts.bend_radius;
                            let x2 = dst.pos.x + dst.size.w;
                            let y2 = dst.pos.y + self.opts.header_drop;
                            (
                                to_backedge(&mut cmds, x1, y1, x2, y2, odd, self.opts),
                                x1,
                                x2,
                                y1,
                                y2,
                                None,
                            )
                        }
                        () if dst.is_dummy() && self.blocks[dst.block].is_backedge() => {
                            let x2 = dst.pos.x + self.opts.first_port;
                            let y2 = dst.pos.y
                                + if dst.flags & NEXT_BACKEDGE != 0 {
                                    self.opts.header_drop + self.opts.bend_radius
                                } else {
                                    0.0
                                };
                            if node.is_dummy() {
                                let mid = y1 - self.opts.track_padding;
                                (
                                    up_arrow(&mut cmds, x1, y1, x2, y2, mid, odd, self.opts),
                                    x1,
                                    x2,
                                    y1,
                                    y2,
                                    Some(mid),
                                )
                            } else {
                                let mid = (y1 - node.size.h)
                                    + heights[layer]
                                    + self.opts.track_padding
                                    + tracks[layer] / 2.0
                                    + link.joint;
                                (
                                    block_to_backedge(
                                        &mut cmds, x1, y1, x2, y2, mid, odd, self.opts,
                                    ),
                                    x1,
                                    x2,
                                    y1,
                                    y2,
                                    Some(mid),
                                )
                            }
                        }
                        () => {
                            let x2 = dst.pos.x + self.opts.first_port;
                            let y2 = dst.pos.y;
                            let mid = (y1 - node.size.h)
                                + heights[layer]
                                + self.opts.track_padding
                                + tracks[layer] / 2.0
                                + link.joint;
                            (
                                down_arrow(
                                    &mut cmds,
                                    x1,
                                    y1,
                                    x2,
                                    y2,
                                    mid,
                                    !dst.is_dummy(),
                                    odd,
                                    self.opts,
                                ),
                                x1,
                                x2,
                                y1,
                                y2,
                                Some(mid),
                            )
                        }
                    };
                    max_x = max_x.max(x1 + self.opts.padding);
                    max_x = max_x.max(x2 + self.opts.padding);
                    max_y = max_y.max(y1 + self.opts.padding);
                    max_y = max_y.max(y2 + self.opts.padding);
                    if let Some(mid) = mid {
                        max_y = max_y.max(mid + self.opts.padding);
                    }
                    paths.push(path);
                }
            }
        }

        let mut placed = Vec::with_capacity(self.blocks.len());
        for id in 0..self.blocks.len() {
            let node = self.blocks[id]
                .layout_node
                .expect("block has no layout node");
            placed.push(Placed {
                id: Id::from_index(id),
                rect: Rect::new(self.nodes[node].pos, self.blocks[id].size),
            });
        }
        let point_finite = |p: Point| p.x.is_finite() && p.y.is_finite();
        if !max_x.is_finite()
            || !max_y.is_finite()
            || placed
                .iter()
                .any(|node| !node.rect.pos.x.is_finite() || !node.rect.pos.y.is_finite())
            || !cmds.iter().all(|cmd| match *cmd {
                Cmd::Move(p) | Cmd::Line(p) | Cmd::Arc { to: p, .. } => point_finite(p),
                Cmd::Cubic { a, b, to } => point_finite(a) && point_finite(b) && point_finite(to),
            })
        {
            return Err(LayoutErr::Overflow);
        }
        Ok(Layout {
            nodes: placed,
            paths,
            cmds,
            bounds: Size::new(max_x, max_y),
        })
    }
}

fn odd_stroke(width: f64) -> bool {
    (width % 2.0).total_cmp(&1.0).is_eq()
}

#[allow(clippy::cast_precision_loss)]
fn num(value: usize) -> f64 {
    value as f64
}

fn arrow(tip: Point, rot: f64, opts: &Opts) -> Arrow {
    Arrow {
        tip,
        rot,
        size: opts.arrow_size,
    }
}

#[allow(clippy::too_many_arguments)]
fn down_arrow(
    cmds: &mut Vec<Cmd>,
    mut x1: f64,
    y1: f64,
    mut x2: f64,
    y2: f64,
    mut mid: f64,
    head: bool,
    odd: bool,
    opts: &Opts,
) -> Path {
    if odd {
        x1 += 0.5;
        x2 += 0.5;
        mid += 0.5;
    }
    let r = opts.bend_radius;
    let curve = (x2 - x1).abs() < 2.0 * r;
    let start = cmds.len();
    cmds.push(Cmd::Move(Point::new(x1, y1)));
    if curve {
        let h = y2 - y1;
        cmds.push(Cmd::Cubic {
            a: Point::new(x1, y1 + h / 3.0),
            b: Point::new(x2, y1 + 2.0 * h / 3.0),
            to: Point::new(x2, y2),
        });
    } else {
        let dir = if x2 > x1 { 1.0 } else { -1.0 };
        cmds.push(Cmd::Line(Point::new(x1, mid - r)));
        cmds.push(Cmd::Arc {
            r,
            sweep: dir <= 0.0,
            to: Point::new(x1 + r * dir, mid),
        });
        cmds.push(Cmd::Line(Point::new(x2 - r * dir, mid)));
        cmds.push(Cmd::Arc {
            r,
            sweep: dir > 0.0,
            to: Point::new(x2, mid + r),
        });
        cmds.push(Cmd::Line(Point::new(x2, y2)));
    }
    Path {
        cmds: start..cmds.len(),
        arrow: head.then_some(arrow(Point::new(x2, y2), 180.0, opts)),
        width: opts.line_width,
    }
}

#[allow(clippy::too_many_arguments)]
fn up_arrow(
    cmds: &mut Vec<Cmd>,
    mut x1: f64,
    y1: f64,
    mut x2: f64,
    y2: f64,
    mut mid: f64,
    odd: bool,
    opts: &Opts,
) -> Path {
    if odd {
        x1 += 0.5;
        x2 += 0.5;
        mid += 0.5;
    }
    let r = opts.bend_radius;
    let curve = (x2 - x1).abs() < 2.0 * r;
    let start = cmds.len();
    cmds.push(Cmd::Move(Point::new(x1, y1)));
    if curve {
        let h = y2 - y1;
        cmds.push(Cmd::Cubic {
            a: Point::new(x1, y1 + h / 3.0),
            b: Point::new(x2, y1 + 2.0 * h / 3.0),
            to: Point::new(x2, y2),
        });
    } else {
        let dir = if x2 > x1 { 1.0 } else { -1.0 };
        cmds.push(Cmd::Line(Point::new(x1, mid + r)));
        cmds.push(Cmd::Arc {
            r,
            sweep: dir > 0.0,
            to: Point::new(x1 + r * dir, mid),
        });
        cmds.push(Cmd::Line(Point::new(x2 - r * dir, mid)));
        cmds.push(Cmd::Arc {
            r,
            sweep: dir <= 0.0,
            to: Point::new(x2, mid - r),
        });
        cmds.push(Cmd::Line(Point::new(x2, y2)));
    }
    Path {
        cmds: start..cmds.len(),
        arrow: None,
        width: opts.line_width,
    }
}

fn to_backedge(
    cmds: &mut Vec<Cmd>,
    mut x1: f64,
    y1: f64,
    x2: f64,
    mut y2: f64,
    odd: bool,
    opts: &Opts,
) -> Path {
    if odd {
        x1 += 0.5;
        y2 += 0.5;
    }
    let r = opts.bend_radius;
    let start = cmds.len();
    cmds.extend([
        Cmd::Move(Point::new(x1, y1)),
        Cmd::Arc {
            r,
            sweep: false,
            to: Point::new(x1 - r, y2),
        },
        Cmd::Line(Point::new(x2, y2)),
    ]);
    Path {
        cmds: start..cmds.len(),
        arrow: Some(arrow(Point::new(x2, y2), 270.0, opts)),
        width: opts.line_width,
    }
}

#[allow(clippy::too_many_arguments)]
fn block_to_backedge(
    cmds: &mut Vec<Cmd>,
    mut x1: f64,
    y1: f64,
    mut x2: f64,
    y2: f64,
    mut mid: f64,
    odd: bool,
    opts: &Opts,
) -> Path {
    if odd {
        x1 += 0.5;
        x2 += 0.5;
        mid += 0.5;
    }
    let r = opts.bend_radius;
    let start = cmds.len();
    cmds.extend([
        Cmd::Move(Point::new(x1, y1)),
        Cmd::Line(Point::new(x1, mid - r)),
        Cmd::Arc {
            r,
            sweep: false,
            to: Point::new(x1 + r, mid),
        },
        Cmd::Line(Point::new(x2 - r, mid)),
        Cmd::Arc {
            r,
            sweep: false,
            to: Point::new(x2, mid - r),
        },
        Cmd::Line(Point::new(x2, y2)),
    ]);
    Path {
        cmds: start..cmds.len(),
        arrow: None,
        width: opts.line_width,
    }
}

fn loop_arrow(
    cmds: &mut Vec<Cmd>,
    x1: f64,
    mut y1: f64,
    x2: f64,
    mut y2: f64,
    odd: bool,
    opts: &Opts,
) -> Path {
    if odd {
        y1 += 0.5;
        y2 += 0.5;
    }
    let start = cmds.len();
    cmds.extend([Cmd::Move(Point::new(x1, y1)), Cmd::Line(Point::new(x2, y2))]);
    Path {
        cmds: start..cmds.len(),
        arrow: Some(arrow(Point::new(x2, y2), 270.0, opts)),
        width: opts.line_width,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Node;

    fn add(graph: &mut Graph, w: f64, h: f64) -> Id {
        graph.add(Node::new(Size::new(w, h)))
    }

    #[test]
    fn srcs_promote_and_remove() {
        let mut srcs = Srcs::new(1);
        assert!(matches!(&srcs, Srcs::Inline(None)));
        srcs.add(2);
        srcs.add(2);
        assert_eq!(srcs.as_slice(), &[2]);

        srcs.add(3);
        assert!(matches!(&srcs, Srcs::Heap(_)));
        srcs.add(3);
        srcs.add(2);
        srcs.add(4);
        assert_eq!(srcs.as_slice(), &[2, 3, 4]);
        assert!(srcs.remove(3));
        assert_eq!(srcs.as_slice(), &[2, 4]);
        assert!(!srcs.remove(3));

        let mut inline = Srcs::new(0);
        inline.add(5);
        assert!(inline.remove(5));
        assert!(matches!(&inline, Srcs::Inline(None)));
        assert!(matches!(Srcs::new(2), Srcs::Heap(_)));
    }

    #[test]
    fn rejects_unmarked_cycle() {
        let mut graph = Graph::new();
        let a = add(&mut graph, 1.0, 1.0);
        let b = add(&mut graph, 1.0, 1.0);
        graph.connect(a, b);
        graph.connect(b, a);

        assert_eq!(
            layout(&graph, &Opts::default()),
            Err(LayoutErr::UnmarkedCycle(a))
        );
    }

    #[test]
    fn rejects_rootless_loop() {
        let mut graph = Graph::new();
        let header = add(&mut graph, 1.0, 1.0);
        let backedge = add(&mut graph, 1.0, 1.0);
        graph[header].flags = Flags::HEADER;
        graph[backedge].flags = Flags::BACKEDGE;
        graph.connect(header, backedge);
        graph.connect(backedge, header);

        assert_eq!(layout(&graph, &Opts::default()), Err(LayoutErr::NoRoot));
    }
}
