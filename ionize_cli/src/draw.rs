// SPDX-License-Identifier: MPL-2.0

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use ionize::{Flags, Graph, Node, Size};
use ionize_draw::{
    Align, Attr, Block as DrawBlock, Cell, Col, Diagram, Field, Label, Meta, Row, Run, Table, Text,
};

use crate::args::Heat;
use crate::data::{Counts, LirBlock, MirBlock, Pass, Samples, Sel};
use crate::err::{Err, Result};

const USE_PAD: f64 = 14.0 * 0.25;
const STYLE: &str = r".mir.movable text{fill:#1048af}
.mir.guard text{text-decoration:underline}
.mir.recovered text{fill:red;text-decoration-style:wavy;text-decoration-line:line-through}
.mir.worklist text{fill:red}
";

pub fn build<'a>(sel: &'a Sel, samples: Option<&Samples>, heat: Heat) -> Result<Diagram<'a>> {
    let pass = &sel.pass;
    let lir = validate_blocks(pass)?;
    let max = heat_max(samples, heat);
    let mut graph = Graph::new();
    let mut ids = BTreeMap::new();
    let mut node_ids = Vec::with_capacity(pass.mir.blocks.len());
    let mut blocks = Vec::with_capacity(pass.mir.blocks.len());

    for mir in &pass.mir.blocks {
        let mut node = Node::new(Size::default());
        node.depth = mir.depth;
        if has(&mir.attributes, "loopheader") {
            node.flags |= Flags::HEADER;
        }
        if has(&mir.attributes, "backedge") {
            node.flags |= Flags::BACKEDGE;
        }
        if has(&mir.attributes, "osr") {
            node.flags |= Flags::OSR;
        }
        let id = graph.add(node);
        ids.insert(mir.id, id);
        node_ids.push(id);
        blocks.push(block(mir, lir.get(&mir.id).copied(), samples, heat, max));
    }

    for (mir, id) in pass.mir.blocks.iter().zip(node_ids) {
        graph[id].preds = edges(&ids, mir.id, "predecessor", &mir.predecessors)?;
        graph[id].succs = edges(&ids, mir.id, "successor", &mir.successors)?;
    }

    let func_idx = u64::try_from(sel.func_idx)
        .map_err(|_| Err::new("function index does not fit SVG metadata"))?;

    let pass_idx = u64::try_from(sel.pass_idx)
        .map_err(|_| Err::new("pass index does not fit SVG metadata"))?;

    let meta = Meta::new("ionize")
        .attr(Attr::new("version", 1_u64))
        .attr(Attr::new("function-index", func_idx))
        .attr(Attr::new("pass-index", pass_idx))
        .field(Field::new("function", sel.func_name.as_str()))
        .field(Field::new("pass", pass.name.as_str()));

    let title = format!("{} — {}", sel.func_name, pass.name);
    Diagram::new(graph, blocks)
        .map(|diagram| diagram.title(title).meta(meta).css(STYLE))
        .map_err(|err| Err::new(err.to_string()))
}

fn block<'a>(
    mir: &'a MirBlock,
    lir: Option<&'a LirBlock>,
    samples: Option<&Samples>,
    heat: Heat,
    max: f64,
) -> DrawBlock<'a> {
    let table = match lir {
        Some(lir) => lir_table(lir, samples, heat, max),
        None => mir_table(mir),
    };
    let mut block = DrawBlock::new(title(mir), table)
        .attr(Attr::new("data-block-ptr", mir.ptr))
        .attr(Attr::new("data-block-id", mir.id));

    if has(&mir.attributes, "loopheader") {
        block = block.class("loop");
    }

    if has(&mir.attributes, "splitedge") {
        block = block.body_class("split");
    }

    if let Some(lir) = lir {
        block = block.attr(Attr::new("data-lir-block-ptr", lir.ptr));
    }

    if mir.successors.len() == 2 {
        block = block
            .label(Label::new(20.0, "1"))
            .label(Label::new(80.0, "0"));
    }
    block
}

fn mir_table(block: &MirBlock) -> Table<'_> {
    let mut table = Table::new([Col::normal(1.0), Col::normal(0.0), Col::normal(1.0)]);
    for ins in &block.instructions {
        let ty = if ins.ty == "None" {
            Cell::empty()
        } else {
            Cell::new(ins.ty.as_str()).class("type").align(Align::End)
        };
        let row = Row::new([
            Cell::new(Text::int(ins.id)).class("num").align(Align::End),
            opcode_cell(&ins.opcode),
            ty,
        ])
        .class(mir_class(&ins.attributes))
        .attr(Attr::new("data-ins-ptr", ins.ptr))
        .attr(Attr::new("data-ins-id", ins.id));
        table.push(row);
    }
    table
}

fn lir_table<'a>(
    block: &'a LirBlock,
    samples: Option<&Samples>,
    heat: Heat,
    max: f64,
) -> Table<'a> {
    let mut cols = vec![Col::normal(1.0), Col::normal(0.0)];
    if samples.is_some() {
        cols.extend([Col::small(0.0), Col::small(0.0)]);
    }
    let mut table = Table::new(cols);
    if samples.is_some() {
        table.set_head(Row::small([
            Cell::empty(),
            Cell::empty(),
            Cell::new("Total").class("sample-head").align(Align::Middle),
            Cell::new("Self").class("sample-head").align(Align::Middle),
        ]));
    }

    for ins in &block.instructions {
        let counts = samples.map(|samples| samples.get(ins.id));
        let mut cells = vec![
            Cell::new(Text::int(ins.id)).class("num").align(Align::End),
            Cell::new(pretty(&ins.opcode)).class("opcode"),
        ];
        if let Some(counts) = counts {
            cells.push(sample_cell(counts.total));
            cells.push(sample_cell(counts.self_count));
        }
        let mut row = Row::new(cells)
            .class("lir")
            .attr(Attr::new("data-ins-ptr", ins.ptr))
            .attr(Attr::new("data-ins-id", ins.id));
        if let Some(ptr) = ins.mir_ptr {
            row = row.attr(Attr::new("data-mir-ptr", ptr));
        }
        if let Some(counts) = counts {
            let value = sample(counts, heat);
            row = row.heat(if max == 0.0 { 0.0 } else { value / max });
        }
        table.push(row);
    }
    table
}

#[inline]
fn sample_cell<'a>(count: f64) -> Cell<'a> {
    Cell::new(Text::num(count))
        .class(if count == 0.0 { "sample dim" } else { "sample" })
        .align(Align::End)
}

fn opcode_cell(opcode: &str) -> Cell<'_> {
    match pretty(opcode) {
        Cow::Borrowed(text) => {
            let mut cell = Cell::empty();
            each_piece(text, |piece| cell.push(piece.run()));
            cell
        }
        Cow::Owned(text) => {
            let mut cell = Cell::empty();
            each_piece(&text, |piece| cell.push(piece.owned_run()));
            cell
        }
    }
}

fn title(block: &MirBlock) -> String {
    let desc = if has(&block.attributes, "loopheader") {
        " (loop header)"
    } else if has(&block.attributes, "backedge") {
        " (backedge)"
    } else if has(&block.attributes, "splitedge") {
        " (split edge)"
    } else {
        ""
    };
    format!("Block {}{desc}", block.id)
}

fn mir_class(attrs: &[String]) -> Cow<'static, str> {
    let mut class = String::new();
    for (attr, name) in [
        ("Movable", "movable"),
        ("Guard", "guard"),
        ("RecoveredOnBailout", "recovered"),
        ("InWorklist", "worklist"),
    ] {
        if has(attrs, attr) {
            if class.is_empty() {
                class.push_str("mir");
            }
            class.push(' ');
            class.push_str(name);
        }
    }
    if class.is_empty() {
        Cow::Borrowed("mir")
    } else {
        Cow::Owned(class)
    }
}

fn pretty(opcode: &str) -> Cow<'_, str> {
    let right = opcode.find("->");
    if right.is_none() && !opcode.contains("<-") {
        return Cow::Borrowed(opcode);
    }
    let mut pretty = String::with_capacity(opcode.len() + 2);
    pretty.push_str(opcode);
    if let Some(index) = right {
        pretty.replace_range(index..index + 2, "→");
    }
    if let Some(index) = pretty.find("<-") {
        pretty.replace_range(index..index + 2, "←");
    }
    Cow::Owned(pretty)
}

#[derive(Clone, Copy)]
struct Piece<'a> {
    text: &'a str,
    use_ref: bool,
}

impl<'a> Piece<'a> {
    fn run(self) -> Run<'a> {
        let run = Run::new(self.text).class(if self.use_ref { "use" } else { "opcode" });
        if self.use_ref { run.pad(USE_PAD) } else { run }
    }

    fn owned_run(self) -> Run<'static> {
        let run = Run::new(self.text.to_owned()).class(if self.use_ref { "use" } else { "opcode" });
        if self.use_ref { run.pad(USE_PAD) } else { run }
    }
}

fn each_piece<'a>(text: &'a str, mut f: impl FnMut(Piece<'a>)) {
    let bytes = text.as_bytes();
    let mut plain = 0;
    let mut start = 0;
    while start < bytes.len() {
        if !word(bytes[start]) {
            start += char_len(text, start);
            continue;
        }
        let mut hash = start;
        while hash < bytes.len() && word(bytes[hash]) {
            hash += 1;
        }
        if hash == bytes.len() || bytes[hash] != b'#' {
            start = hash;
            continue;
        }
        let mut end = hash + 1;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
        if end == hash + 1 {
            start = end;
            continue;
        }

        if plain < start {
            f(Piece {
                text: &text[plain..start],
                use_ref: false,
            });
        }
        f(Piece {
            text: &text[start..end],
            use_ref: true,
        });
        plain = end;
        start = end;
    }
    if plain < text.len() {
        f(Piece {
            text: &text[plain..],
            use_ref: false,
        });
    }
}

fn validate_blocks(pass: &Pass) -> Result<BTreeMap<u64, &LirBlock>> {
    let mut mir_ids = BTreeSet::new();
    let mut mir_ptrs = BTreeSet::new();
    for block in &pass.mir.blocks {
        if block.ptr == 0 {
            return Err(Err::new(format!("MIR block {} has a null ptr", block.id)));
        }
        if !mir_ids.insert(block.id) {
            return Err(Err::new(format!("duplicate MIR block id {}", block.id)));
        }
        if !mir_ptrs.insert(block.ptr) {
            return Err(Err::new(format!("duplicate MIR block ptr {}", block.ptr)));
        }
    }
    let mut lir_blocks = BTreeMap::new();
    let mut lir_ptrs = BTreeSet::new();
    for block in &pass.lir.blocks {
        if lir_blocks.insert(block.id, block).is_some() {
            return Err(Err::new(format!("duplicate LIR block id {}", block.id)));
        }
        if !lir_ptrs.insert(block.ptr) {
            return Err(Err::new(format!("duplicate LIR block ptr {}", block.ptr)));
        }
    }
    Ok(lir_blocks)
}

fn edges(
    ids: &BTreeMap<u64, ionize::Id>,
    block: u64,
    kind: &str,
    edges: &[u64],
) -> Result<Vec<ionize::Id>> {
    edges
        .iter()
        .map(|edge| {
            ids.get(edge).copied().ok_or_else(|| {
                Err::new(format!(
                    "MIR block {block} refers to unknown {kind} block {edge}"
                ))
            })
        })
        .collect()
}

fn heat_max(samples: Option<&Samples>, heat: Heat) -> f64 {
    let Some(samples) = samples else { return 0.0 };
    samples
        .counts
        .values()
        .copied()
        .map(|counts| sample(counts, heat))
        .fold(0.0, f64::max)
}

#[inline]
fn sample(counts: Counts, heat: Heat) -> f64 {
    match heat {
        Heat::Total => counts.total,
        Heat::SelfCount => counts.self_count,
    }
}

#[inline]
fn has(attrs: &[String], name: &str) -> bool {
    attrs.iter().any(|attr| attr == name)
}

#[inline]
fn word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

#[inline]
// TODO: remove
fn char_len(text: &str, index: usize) -> usize {
    text[index..].chars().next().map_or(1, char::len_utf8)
}
