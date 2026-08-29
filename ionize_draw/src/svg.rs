// SPDX-License-Identifier: MPL-2.0

use alloc::{borrow::Cow, format, string::String, vec::Vec};
use core::{
    cell::RefCell,
    fmt::{self, Write as _},
};

use hashbrown::{HashMap, hash_map::Entry};
use ionize::{Cmd, Layout, Opts, Path, Point, Rect, Size, layout};

use crate::err::{Err, Result};
use crate::fmt::num;
use crate::font::{Fonts, Metrics, Weight};
use crate::model::{
    Align, Attr, BODY_PAD, Block, Diagram, FONT_SIZE, LABEL_SIZE, Meta, Row, Table, Text, Value,
    xml_char,
};

/// Renders diagrams as SVG using the bundled fonts.
pub struct Renderer {
    fonts: Fonts,
    text: String,
}

const WIDTH_CACHE_MIN: usize = 128;
const NUM_CACHE_MIN: usize = 128;
const NUMS_PER_BLOCK: usize = 8;

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
enum KeyText<'a> {
    Str(&'a str),
    Int(u64),
    Num(u64),
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
struct WidthKey<'a> {
    text: KeyText<'a>,
    size: u64,
    weight: Weight,
}

struct WidthCache<'a> {
    map: Option<HashMap<WidthKey<'a>, f64>>,
}

struct NumCache {
    state: Option<RefCell<NumState>>,
}

struct NumState {
    map: HashMap<u64, (usize, usize)>,
    text: String,
}

struct CachedNum<'a> {
    cache: &'a NumCache,
    value: f64,
}

impl WidthCache<'_> {
    fn new(calls: usize) -> Self {
        Self {
            map: (calls >= WIDTH_CACHE_MIN).then(|| HashMap::with_capacity(calls)),
        }
    }
}

fn key_text<'a>(text: &Text<'a>) -> Option<KeyText<'a>> {
    match &text.0 {
        Value::Str(Cow::Borrowed(value)) => Some(KeyText::Str(value)),
        Value::Str(Cow::Owned(_)) => None,
        Value::Int(value) => Some(KeyText::Int(*value)),
        Value::Num(value) => Some(KeyText::Num(value.to_bits())),
    }
}

impl NumCache {
    fn new(cmds: usize, blocks: usize) -> Self {
        let size = cmds.saturating_add(blocks.saturating_mul(NUMS_PER_BLOCK));
        Self {
            state: (size >= NUM_CACHE_MIN).then(|| {
                RefCell::new(NumState {
                    map: HashMap::with_capacity(size),
                    text: String::new(),
                })
            }),
        }
    }

    fn num(&self, value: f64) -> CachedNum<'_> {
        CachedNum { cache: self, value }
    }
}

impl fmt::Display for CachedNum<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.value == 0.0 {
            return f.write_str("0");
        }
        let Some(state) = &self.cache.state else {
            return fmt::Display::fmt(&num(self.value), f);
        };

        let mut state = state.borrow_mut();
        let NumState { map, text } = &mut *state;
        let range = match map.entry(self.value.to_bits()) {
            Entry::Occupied(entry) => *entry.get(),
            Entry::Vacant(entry) => {
                let start = text.len();
                write!(text, "{}", num(self.value))?;
                let range = (start, text.len());
                entry.insert(range);
                range
            }
        };
        f.write_str(&text[range.0..range.1])
    }
}

impl Renderer {
    /// Loads the bundled fonts.
    ///
    /// # Errors
    ///
    /// Returns an error if a bundled font cannot be loaded.
    pub fn new() -> Result<Self> {
        Ok(Self {
            fonts: Fonts::new()?,
            text: String::new(),
        })
    }

    /// Measures and renders `diagram` using `opts`.
    ///
    /// # Errors
    ///
    /// Returns an error if the diagram is invalid or its graph cannot be laid
    /// out.
    pub fn render(&mut self, mut diagram: Diagram<'_>, opts: &Opts) -> Result<String> {
        let cacheable = validate(&diagram)?;
        let header = self.fonts.metrics(FONT_SIZE, Weight::Bold);
        let header_h = header.line + 1.0;
        let mut widths = WidthCache::new(cacheable);
        let ids: Vec<_> = diagram.graph.ids().collect();
        for (id, block) in ids.into_iter().zip(&mut diagram.blocks) {
            let size = measure_block(
                &mut self.fonts,
                &mut self.text,
                &mut widths,
                block,
                header_h,
            );
            diagram.graph[id].size = size;
        }

        let layout = layout(&diagram.graph, opts)?;
        Ok(draw(&diagram, &layout, &self.fonts, header, header_h))
    }
}

fn measure_block<'a>(
    fonts: &mut Fonts,
    text: &mut String,
    widths: &mut WidthCache<'a>,
    block: &mut Block<'a>,
    header_h: f64,
) -> Size {
    let table_h = measure_table(fonts, text, widths, &mut block.table);
    let title_w = text_width(fonts, text, widths, &block.title, FONT_SIZE, Weight::Bold);
    let header_w = title_w + FONT_SIZE * 2.0 + 2.0;
    let body_w = block.table.width + BODY_PAD * 2.0 + 2.0;
    let body_h = table_h + BODY_PAD * 2.0 + 1.0;
    Size::new(
        libm::ceil(header_w.max(body_w)),
        libm::ceil(header_h + body_h),
    )
}

fn measure_table<'a>(
    fonts: &mut Fonts,
    text: &mut String,
    widths: &mut WidthCache<'a>,
    table: &mut Table<'a>,
) -> f64 {
    for col in &mut table.cols {
        col.width = col.min;
    }

    let mut height = 0.0;
    if let Some(head) = &mut table.head {
        measure_row(fonts, text, widths, &mut table.cols, head);
        height += head.height;
    }
    for row in &mut table.rows {
        measure_row(fonts, text, widths, &mut table.cols, row);
        height += row.height;
    }
    for col in &mut table.cols {
        col.width += col.pad * 2.0;
    }
    table.width = table.cols.iter().map(|col| col.width).sum();
    height
}

fn measure_row<'a>(
    fonts: &mut Fonts,
    text: &mut String,
    widths: &mut WidthCache<'a>,
    cols: &mut [crate::model::Col],
    row: &mut Row<'a>,
) {
    for (cell, col) in row.cells.iter().zip(cols.iter()) {
        if cell.runs.iter().any(|run| !run.text.is_empty()) {
            row.size = row.size.max(col.size);
        }
    }
    row.height = fonts.metrics(row.size, Weight::Regular).line + row.pad_y * 2.0;
    for (cell, col) in row.cells.iter_mut().zip(cols) {
        let mut cell_width = 0.0;
        for run in &mut cell.runs {
            run.width = text_width(fonts, text, widths, &run.text, col.size, Weight::Regular);
            cell_width += run.width + run.pad * 2.0;
        }
        cell.width = cell_width;
        col.width = col.width.max(cell_width);
    }
}

fn text_width<'a>(
    fonts: &mut Fonts,
    scratch: &mut String,
    widths: &mut WidthCache<'a>,
    text: &Text<'a>,
    size: f64,
    weight: Weight,
) -> f64 {
    if let Some(map) = &mut widths.map
        && let Some(key_text) = key_text(text)
    {
        let key = WidthKey {
            text: key_text,
            size: size.to_bits(),
            weight,
        };
        return match map.entry(key) {
            Entry::Occupied(entry) => *entry.get(),
            Entry::Vacant(entry) => {
                let width = uncached_width(fonts, scratch, text, size, weight);
                entry.insert(width);
                width
            }
        };
    }
    uncached_width(fonts, scratch, text, size, weight)
}

fn uncached_width(
    fonts: &mut Fonts,
    scratch: &mut String,
    text: &Text<'_>,
    size: f64,
    weight: Weight,
) -> f64 {
    match &text.0 {
        Value::Str(value) => fonts.width(value, size, weight),
        Value::Int(value) => {
            scratch.clear();
            write!(scratch, "{value}").unwrap();
            fonts.width(scratch, size, weight)
        }
        Value::Num(value) => {
            scratch.clear();
            write!(scratch, "{}", num(*value)).unwrap();
            fonts.width(scratch, size, weight)
        }
    }
}

fn validate(diagram: &Diagram<'_>) -> Result<usize> {
    let mut cacheable = 0_usize;
    if let Some(meta) = &diagram.meta {
        validate_name(&meta.tag, "metadata tag")?;
        validate_attrs(&meta.attrs, &[])?;
        for field in &meta.fields {
            validate_name(&field.name, "metadata field")?;
            validate_text(&field.value)?;
        }
    }
    validate_text(&diagram.title)?;

    for block in &diagram.blocks {
        validate_text(&block.title)?;
        cacheable = cacheable.saturating_add(usize::from(key_text(&block.title).is_some()));
        validate_attrs(&block.attrs, &["class", "transform"])?;
        for label in &block.labels {
            nonnegative(label.x, "label position")?;
            validate_text(&label.text)?;
        }
        for col in &block.table.cols {
            nonnegative(col.min, "column minimum width")?;
        }
        let cols = block.table.cols.len();
        if let Some(row) = &block.table.head {
            cacheable = cacheable.saturating_add(validate_row(row, cols)?);
        }
        for row in &block.table.rows {
            cacheable = cacheable.saturating_add(validate_row(row, cols)?);
        }
    }
    Ok(cacheable)
}

fn validate_row(row: &Row<'_>, cols: usize) -> Result<usize> {
    if row.cells.len() != cols {
        return Err(Err::new(format!(
            "row cell count {} does not match column count {cols}",
            row.cells.len(),
        )));
    }
    if let Some(heat) = row.heat
        && (!heat.is_finite() || !(0.0..=1.0).contains(&heat))
    {
        return Err(Err::new("row heat must be finite and between zero and one"));
    }
    validate_attrs(&row.attrs, &["class"])?;
    let mut cacheable = 0_usize;
    for cell in &row.cells {
        for run in &cell.runs {
            nonnegative(run.pad, "run padding")?;
            validate_text(&run.text)?;
            cacheable = cacheable.saturating_add(usize::from(key_text(&run.text).is_some()));
        }
    }
    Ok(cacheable)
}

fn validate_attrs(attrs: &[Attr<'_>], reserved: &[&str]) -> Result<()> {
    for (index, attr) in attrs.iter().enumerate() {
        validate_name(&attr.name, "attribute")?;
        if attr.name == "xmlns" || reserved.contains(&attr.name.as_ref()) {
            return Err(Err::new(format!(
                "attribute name {:?} is reserved",
                attr.name
            )));
        }
        if attrs[..index].iter().any(|other| other.name == attr.name) {
            return Err(Err::new(format!(
                "duplicate attribute name {:?}",
                attr.name
            )));
        }
        validate_text(&attr.value)?;
    }
    Ok(())
}

fn validate_text(text: &Text<'_>) -> Result<()> {
    if matches!(text.0, Value::Num(value) if !value.is_finite()) {
        return Err(Err::new("text number must be finite"));
    }
    Ok(())
}

fn nonnegative(value: f64, what: &str) -> Result<()> {
    if value.is_finite() && value >= 0.0 {
        Ok(())
    } else {
        Err(Err::new(format!("{what} must be finite and non-negative")))
    }
}

fn validate_name(name: &str, what: &str) -> Result<()> {
    let mut chars = name.chars();
    if !chars.next().is_some_and(name_start) || !chars.all(name_char) {
        return Err(Err::new(format!("{what} {name:?} is not a valid XML name")));
    }
    Ok(())
}

fn name_start(ch: char) -> bool {
    matches!(
        ch,
        'A'..='Z' | '_' | 'a'..='z'
            | '\u{C0}'..='\u{D6}'
            | '\u{D8}'..='\u{F6}'
            | '\u{F8}'..='\u{2FF}'
            | '\u{370}'..='\u{37D}'
            | '\u{37F}'..='\u{1FFF}'
            | '\u{200C}'..='\u{200D}'
            | '\u{2070}'..='\u{218F}'
            | '\u{2C00}'..='\u{2FEF}'
            | '\u{3001}'..='\u{D7FF}'
            | '\u{F900}'..='\u{FDCF}'
            | '\u{FDF0}'..='\u{FFFD}'
            | '\u{10000}'..='\u{EFFFF}'
    )
}

fn name_char(ch: char) -> bool {
    name_start(ch)
        || matches!(
            ch,
            '-' | '.' | '0'..='9' | '\u{B7}' | '\u{300}'..='\u{36F}' | '\u{203F}'..='\u{2040}'
        )
}

fn draw(
    diagram: &Diagram<'_>,
    layout: &Layout,
    fonts: &Fonts,
    header: Metrics,
    header_h: f64,
) -> String {
    let nums = NumCache::new(layout.cmds.len(), diagram.blocks.len());
    let width = nums.num(layout.bounds.w);
    let height = nums.num(layout.bounds.h);
    let mut out = String::with_capacity(Fonts::css_len() * 2);
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    writeln!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\">"
    )
    .unwrap();
    if !diagram.title.is_empty() {
        out.push_str("<title>");
        write_text(&mut out, &diagram.title, &nums);
        out.push_str("</title>\n");
    }
    if let Some(meta) = &diagram.meta {
        draw_meta(&mut out, meta, &nums);
    }
    out.push_str("<defs><style>\n");
    Fonts::css(&mut out);
    out.push_str(STYLE_START);
    write!(out, "{}", xml(&diagram.css)).unwrap();
    out.push_str(STYLE_END);
    out.push_str("</style></defs>\n");
    writeln!(
        out,
        "<rect class=\"canvas\" width=\"{width}\" height=\"{height}\"/>"
    )
    .unwrap();

    out.push_str("<g class=\"edges\">\n");
    for path in &layout.paths {
        draw_path(&mut out, path, &layout.cmds[path.cmds.clone()], &nums);
    }
    out.push_str("</g>\n<g class=\"blocks\">\n");
    for placed in &layout.nodes {
        draw_block(
            &mut out,
            &diagram.blocks[placed.id.index()],
            placed.rect,
            fonts,
            header,
            header_h,
            &nums,
        );
    }
    out.push_str("</g>\n</svg>\n");
    out
}

fn draw_meta(out: &mut String, meta: &Meta<'_>, nums: &NumCache) {
    write!(out, "<metadata><{}", meta.tag).unwrap();
    draw_attrs(out, &meta.attrs, nums);
    out.push('>');
    for field in &meta.fields {
        write!(out, "<{}>", field.name).unwrap();
        write_text(out, &field.value, nums);
        write!(out, "</{}>", field.name).unwrap();
    }
    writeln!(out, "</{}></metadata>", meta.tag).unwrap();
}

fn draw_path(out: &mut String, path: &Path, cmds: &[Cmd], nums: &NumCache) {
    out.push_str("<g class=\"edge\"><path d=\"");
    write_path_data(out, cmds, nums);
    write!(out, "\" stroke-width=\"{}\"/>", nums.num(path.width)).unwrap();
    if let Some(arrow) = path.arrow {
        write!(
            out,
            "<path class=\"arrow\" d=\"M 0 0 L {} {} L {} {} Z\" transform=\"translate({} {}) rotate({})\"/>",
            nums.num(-arrow.size),
            nums.num(arrow.size * 1.5),
            nums.num(arrow.size),
            nums.num(arrow.size * 1.5),
            nums.num(arrow.tip.x),
            nums.num(arrow.tip.y),
            nums.num(arrow.rot)
        )
        .unwrap();
    }
    out.push_str("</g>\n");
}

fn draw_block(
    out: &mut String,
    block: &Block<'_>,
    rect: Rect,
    fonts: &Fonts,
    header: Metrics,
    header_h: f64,
    nums: &NumCache,
) {
    write!(out, "<g class=\"block").unwrap();
    if !block.class.is_empty() {
        write!(out, " {}", xml(&block.class)).unwrap();
    }
    write!(
        out,
        "\" transform=\"translate({} {})\"",
        nums.num(rect.pos.x),
        nums.num(rect.pos.y)
    )
    .unwrap();
    draw_attrs(out, &block.attrs, nums);
    out.push_str(">\n");

    let width = rect.size.w;
    let height = rect.size.h;
    writeln!(
        out,
        "<rect class=\"header\" width=\"{}\" height=\"{}\"/><path class=\"header-border\" d=\"M 0 {} V 0 H {} V {}\"/>",
        nums.num(width),
        nums.num(header_h),
        nums.num(header_h),
        nums.num(width),
        nums.num(header_h)
    )
    .unwrap();
    write!(
        out,
        "<rect class=\"body\" y=\"{}\" width=\"{}\" height=\"{}\"/>",
        nums.num(header_h),
        nums.num(width),
        nums.num(height - header_h)
    )
    .unwrap();
    write!(out, "<path class=\"body-border").unwrap();
    if !block.body_class.is_empty() {
        write!(out, " {}", xml(&block.body_class)).unwrap();
    }
    writeln!(
        out,
        "\" d=\"M 0 {} V {} H {} V {}\"/>",
        nums.num(header_h),
        nums.num(height),
        nums.num(width),
        nums.num(header_h)
    )
    .unwrap();

    let baseline = (header_h - header.line) * 0.5 + header.ascent;
    write!(
        out,
        "<text class=\"header-text\" x=\"{}\" y=\"{}\">",
        nums.num(width * 0.5),
        nums.num(baseline)
    )
    .unwrap();
    write_text(out, &block.title, nums);
    out.push_str("</text>\n");

    draw_table(
        out,
        &block.table,
        Point::new(BODY_PAD + 1.0, header_h + BODY_PAD),
        fonts,
        nums,
    );
    for label in &block.labels {
        let baseline = height + fonts.metrics(LABEL_SIZE, Weight::Regular).ascent;
        draw_text(
            out,
            Point::new(label.x, baseline),
            &label.text,
            "edge-label",
            Align::Start,
            LABEL_SIZE,
            nums,
        );
    }
    out.push_str("</g>\n");
}

fn draw_table(out: &mut String, table: &Table<'_>, pos: Point, fonts: &Fonts, nums: &NumCache) {
    let mut top = pos.y;
    if let Some(head) = &table.head {
        draw_row(out, table, Point::new(pos.x, top), head, fonts, true, nums);
        top += head.height;
    }
    for row in &table.rows {
        draw_row(out, table, Point::new(pos.x, top), row, fonts, false, nums);
        top += row.height;
    }
}

fn draw_row(
    out: &mut String,
    table: &Table<'_>,
    pos: Point,
    row: &Row<'_>,
    fonts: &Fonts,
    head: bool,
    nums: &NumCache,
) {
    let x = pos.x;
    let top = pos.y;
    let group = !row.class.is_empty() || !row.attrs.is_empty() || row.heat.is_some();
    if group {
        out.push_str("<g");
        if !row.class.is_empty() {
            write!(out, " class=\"{}\"", xml(&row.class)).unwrap();
        }
        draw_attrs(out, &row.attrs, nums);
        out.push('>');
    }
    if let Some(heat) = row.heat {
        write!(
            out,
            "<rect class=\"hot\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" style=\"--hotness:{}\"/>",
            nums.num(x),
            nums.num(top),
            nums.num(table.width),
            nums.num(row.height),
            nums.num(heat)
        )
        .unwrap();
    }
    let mut left = x;
    let row_metrics = fonts.metrics(row.size, Weight::Regular);
    let row_baseline = if head {
        top + row.pad_y + row_metrics.ascent
    } else {
        top + (row.height - row_metrics.line) * 0.5 + row_metrics.ascent
    };
    for (cell, col) in row.cells.iter().zip(&table.cols) {
        let width = col.width;
        let metrics = fonts.metrics(col.size, Weight::Regular);
        let baseline = if col.size.to_bits() == row.size.to_bits() {
            row_baseline
        } else {
            row_baseline + (row_metrics.line - metrics.line) * 0.5 + metrics.ascent
                - row_metrics.ascent
        };
        if let [run] = cell.runs.as_slice()
            && run.pad == 0.0
        {
            let x = match cell.align {
                Align::Start => left + col.pad,
                Align::End => left + width - col.pad,
                Align::Middle => left + width * 0.5,
            };
            draw_text(
                out,
                Point::new(x, baseline),
                &run.text,
                if run.class.is_empty() {
                    &cell.class
                } else {
                    &run.class
                },
                cell.align,
                col.size,
                nums,
            );
            left += width;
            continue;
        }
        let mut pos = match cell.align {
            Align::Start => left + col.pad,
            Align::End => left + width - col.pad - cell.width,
            Align::Middle => left + (width - cell.width) * 0.5,
        };
        for run in &cell.runs {
            pos += run.pad;
            let anchor = match cell.align {
                Align::Start => pos,
                Align::End => pos + run.width,
                Align::Middle => pos + run.width * 0.5,
            };
            draw_text(
                out,
                Point::new(anchor, baseline),
                &run.text,
                if run.class.is_empty() {
                    &cell.class
                } else {
                    &run.class
                },
                cell.align,
                col.size,
                nums,
            );
            pos += run.width;
            pos += run.pad;
        }
        left += width;
    }
    if group {
        out.push_str("</g>\n");
    }
}

fn draw_text(
    out: &mut String,
    pos: Point,
    value: &Text<'_>,
    class: &str,
    align: Align,
    size: f64,
    nums: &NumCache,
) {
    out.push_str("<text");
    if !class.is_empty() || align != Align::Start {
        out.push_str(" class=\"");
        if !class.is_empty() {
            write!(out, "{}", xml(class)).unwrap();
        }
        let align = match align {
            Align::Start => "",
            Align::End => "end",
            Align::Middle => "middle",
        };
        if !align.is_empty() {
            if !class.is_empty() {
                out.push(' ');
            }
            out.push_str(align);
        }
        out.push('"');
    }
    write!(
        out,
        " x=\"{}\" y=\"{}\" font-size=\"{}\" xml:space=\"preserve\">",
        nums.num(pos.x),
        nums.num(pos.y),
        nums.num(size)
    )
    .unwrap();
    write_text(out, value, nums);
    out.push_str("</text>");
}

fn draw_attrs(out: &mut String, attrs: &[Attr<'_>], nums: &NumCache) {
    for item in attrs {
        write!(out, " {}=\"", item.name).unwrap();
        write_text(out, &item.value, nums);
        out.push('"');
    }
}

fn write_text(out: &mut String, text: &Text<'_>, nums: &NumCache) {
    match &text.0 {
        Value::Str(value) => write_normalized(out, value),
        Value::Int(value) => write!(out, "{value}").unwrap(),
        Value::Num(value) => write!(out, "{}", nums.num(*value)).unwrap(),
    }
}

fn write_normalized(out: &mut String, value: &str) {
    let mut plain = 0;
    for (index, byte) in value.bytes().enumerate() {
        let escaped = match byte {
            b'&' => "&amp;",
            b'<' => "&lt;",
            b'>' => "&gt;",
            b'\"' => "&quot;",
            b'\'' => "&apos;",
            _ => continue,
        };
        if plain < index {
            out.push_str(&value[plain..index]);
        }
        out.push_str(escaped);
        plain = index + 1;
    }
    if plain < value.len() {
        out.push_str(&value[plain..]);
    }
}

struct Xml<'a>(&'a str);

impl fmt::Display for Xml<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut plain = 0;
        for (index, ch) in self.0.char_indices() {
            let escaped = match ch {
                '&' => "&amp;",
                '<' => "&lt;",
                '>' => "&gt;",
                '"' => "&quot;",
                '\'' => "&apos;",
                _ if !xml_char(ch) => "\u{FFFD}",
                _ => continue,
            };
            if plain < index {
                f.write_str(&self.0[plain..index])?;
            }
            f.write_str(escaped)?;
            plain = index + ch.len_utf8();
        }
        if plain < self.0.len() {
            f.write_str(&self.0[plain..])?;
        }
        Ok(())
    }
}

fn xml(value: &str) -> Xml<'_> {
    Xml(value)
}

fn write_path_data(out: &mut String, cmds: &[Cmd], nums: &NumCache) {
    for (index, cmd) in cmds.iter().enumerate() {
        if index != 0 {
            out.push(' ');
        }
        match *cmd {
            Cmd::Move(point) => {
                write!(out, "M {} {}", nums.num(point.x), nums.num(point.y)).unwrap();
            }
            Cmd::Line(point) => {
                write!(out, "L {} {}", nums.num(point.x), nums.num(point.y)).unwrap();
            }
            Cmd::Arc { r, sweep, to } => {
                write!(
                    out,
                    "A {} {} 0 0 {} {} {}",
                    nums.num(r),
                    nums.num(r),
                    u8::from(sweep),
                    nums.num(to.x),
                    nums.num(to.y)
                )
                .unwrap();
            }
            Cmd::Cubic { a, b, to } => {
                write!(
                    out,
                    "C {} {} {} {} {} {}",
                    nums.num(a.x),
                    nums.num(a.y),
                    nums.num(b.x),
                    nums.num(b.y),
                    nums.num(to.x),
                    nums.num(to.y)
                )
                .unwrap();
            }
        }
    }
}

const STYLE_START: &str = r"
svg{background:#e5e8ea}
text{font-family:'Ion Noto Sans','Ion Noto Symbols';font-size:14px;fill:#000}
.canvas{fill:#e5e8ea}
.edge>path:first-child{fill:none;stroke:#000}
.arrow{fill:#000}
.header{fill:#0c0c0d}
.loop>.header{fill:#1fa411}
.header-border,.body-border{fill:none;stroke:#0c0c0d;stroke-width:1}
.body{fill:#fff}
.body-border.split{stroke-width:2;stroke-dasharray:2 2}
.header-text{fill:#fff;font-weight:700;text-anchor:middle}
.end{text-anchor:end}
.middle{text-anchor:middle}
";

const STYLE_END: &str = r".dim{fill:#777}
.hot{
  --cool:#ffe546;
  --hot:#ff849e;
  --threshold:.2;
  --cold:color-mix(in srgb,var(--cool) 20%,transparent);
  fill:color-mix(in oklab,
    color-mix(in oklab,
      color-mix(in srgb,transparent,var(--cold) clamp(0%,calc(var(--hotness) * 100000000%),100%)),
      var(--cool) clamp(0%,calc((var(--hotness) / var(--threshold)) * 100%),100%)),
    var(--hot) clamp(0%,calc(((var(--hotness) - var(--threshold)) / (1 - var(--threshold))) * 100%),100%));
}
";
