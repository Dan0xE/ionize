// SPDX-License-Identifier: MPL-2.0

use alloc::{borrow::Cow, format, string::String, vec::Vec};

use ionize::Graph;
use smallvec::SmallVec;

use crate::err::{Err, Result};

pub(crate) const FONT_SIZE: f64 = 14.0;
pub(crate) const SMALL_SIZE: f64 = FONT_SIZE * 0.875;
pub(crate) const BODY_PAD: f64 = FONT_SIZE * 0.5;
pub(crate) const CELL_PAD: f64 = FONT_SIZE * 0.5;
pub(crate) const SMALL_PAD: f64 = SMALL_SIZE * 0.5;
pub(crate) const CELL_PAD_Y: f64 = FONT_SIZE * 0.1;
pub(crate) const SMALL_PAD_Y: f64 = SMALL_SIZE * 0.1;
pub(crate) const LABEL_SIZE: f64 = FONT_SIZE * 0.8;

/// Text measured before it is written to SVG.
#[derive(Clone, Debug)]
pub struct Text<'a>(pub(crate) Value<'a>);

#[derive(Clone, Debug)]
pub(crate) enum Value<'a> {
    Str(Cow<'a, str>),
    Int(u64),
    Num(f64),
}

impl<'a> Text<'a> {
    /// Replaces characters XML cannot represent with U+FFFD.
    ///
    /// The input is borrowed if no replacement is needed.
    #[must_use]
    pub fn new(value: impl Into<Cow<'a, str>>) -> Self {
        Self(Value::Str(normalize(value.into())))
    }

    /// Stores an unsigned integer without formatting it up front.
    #[must_use]
    pub const fn int(value: u64) -> Self {
        Self(Value::Int(value))
    }

    /// Stores a floating-point number without formatting it up front.
    #[must_use]
    pub const fn num(value: f64) -> Self {
        Self(Value::Num(value))
    }

    pub(crate) fn is_empty(&self) -> bool {
        matches!(&self.0, Value::Str(value) if value.is_empty())
    }
}

impl<'a> From<&'a str> for Text<'a> {
    fn from(value: &'a str) -> Self {
        Self::new(value)
    }
}

impl From<String> for Text<'_> {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl<'a> From<Cow<'a, str>> for Text<'a> {
    fn from(value: Cow<'a, str>) -> Self {
        Self::new(value)
    }
}

impl From<u64> for Text<'_> {
    fn from(value: u64) -> Self {
        Self::int(value)
    }
}

impl From<f64> for Text<'_> {
    fn from(value: f64) -> Self {
        Self::num(value)
    }
}

/// An SVG attribute name and value.
#[derive(Clone, Debug)]
pub struct Attr<'a> {
    pub(crate) name: Cow<'a, str>,
    pub(crate) value: Text<'a>,
}

impl<'a> Attr<'a> {
    /// Creates an attribute. Its name is checked during rendering.
    #[must_use]
    pub fn new(name: impl Into<Cow<'a, str>>, value: impl Into<Text<'a>>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }
}

/// A named value inside an SVG metadata element.
#[derive(Clone, Debug)]
pub struct Field<'a> {
    pub(crate) name: Cow<'a, str>,
    pub(crate) value: Text<'a>,
}

impl<'a> Field<'a> {
    /// Creates a field. Its name is checked during rendering.
    #[must_use]
    pub fn new(name: impl Into<Cow<'a, str>>, value: impl Into<Text<'a>>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }
}

/// An element written inside SVG metadata.
#[derive(Clone, Debug)]
pub struct Meta<'a> {
    pub(crate) tag: Cow<'a, str>,
    pub(crate) attrs: Vec<Attr<'a>>,
    pub(crate) fields: Vec<Field<'a>>,
}

impl<'a> Meta<'a> {
    /// Creates an empty element. Its tag is checked during rendering.
    #[must_use]
    pub fn new(tag: impl Into<Cow<'a, str>>) -> Self {
        Self {
            tag: tag.into(),
            attrs: Vec::new(),
            fields: Vec::new(),
        }
    }

    /// Adds an attribute at the end of the SVG attribute list.
    #[must_use]
    pub fn attr(mut self, attr: Attr<'a>) -> Self {
        self.attrs.push(attr);
        self
    }

    /// Adds a child at the end of the metadata element.
    #[must_use]
    pub fn field(mut self, field: Field<'a>) -> Self {
        self.fields.push(field);
        self
    }
}

/// Horizontal text alignment within a table column.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Align {
    /// Align to the leading padded column edge.
    #[default]
    Start,
    /// Align to the trailing padded column edge.
    End,
    /// Center within the column.
    Middle,
}

/// A piece of text with its own style and padding.
#[derive(Clone, Debug)]
pub struct Run<'a> {
    pub(crate) text: Text<'a>,
    pub(crate) class: Cow<'a, str>,
    pub(crate) pad: f64,
    pub(crate) width: f64,
}

impl<'a> Run<'a> {
    /// Creates an unstyled run with no extra padding.
    #[must_use]
    pub fn new(text: impl Into<Text<'a>>) -> Self {
        Self {
            text: text.into(),
            class: Cow::Borrowed(""),
            pad: 0.0,
            width: 0.0,
        }
    }

    /// Sets the SVG class value.
    #[must_use]
    pub fn class(mut self, class: impl Into<Cow<'a, str>>) -> Self {
        self.class = class.into();
        self
    }

    /// Adds equal horizontal space before and after the run.
    #[must_use]
    pub fn pad(mut self, pad: f64) -> Self {
        self.pad = pad;
        self
    }
}

/// A table cell made from text runs.
#[derive(Clone, Debug)]
pub struct Cell<'a> {
    pub(crate) runs: SmallVec<[Run<'a>; 1]>,
    pub(crate) class: Cow<'a, str>,
    pub(crate) align: Align,
    pub(crate) width: f64,
}

impl<'a> Cell<'a> {
    /// Creates a cell containing one run.
    #[must_use]
    pub fn new(text: impl Into<Text<'a>>) -> Self {
        Self::runs([Run::new(text)])
    }

    /// Creates an empty cell.
    #[must_use]
    pub fn empty() -> Self {
        Self::runs([])
    }

    /// Creates a cell from ordered runs.
    #[must_use]
    pub fn runs(runs: impl IntoIterator<Item = Run<'a>>) -> Self {
        Self {
            runs: runs.into_iter().collect(),
            class: Cow::Borrowed(""),
            align: Align::Start,
            width: 0.0,
        }
    }

    /// Appends a text run.
    pub fn push(&mut self, run: Run<'a>) {
        self.runs.push(run);
    }

    /// Sets the fallback SVG class for runs without their own class.
    #[must_use]
    pub fn class(mut self, class: impl Into<Cow<'a, str>>) -> Self {
        self.class = class.into();
        self
    }

    /// Sets the cell alignment.
    #[must_use]
    pub const fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }
}

/// Controls how a table column is measured.
#[derive(Clone, Copy, Debug)]
pub struct Col {
    pub(crate) size: f64,
    pub(crate) pad: f64,
    pub(crate) min: f64,
    pub(crate) width: f64,
}

impl Col {
    /// Creates a normal-size column with a minimum content width.
    #[must_use]
    pub const fn normal(min: f64) -> Self {
        Self {
            size: FONT_SIZE,
            pad: CELL_PAD,
            min,
            width: 0.0,
        }
    }

    /// Creates a small-text column with a minimum content width.
    #[must_use]
    pub const fn small(min: f64) -> Self {
        Self {
            size: SMALL_SIZE,
            pad: SMALL_PAD,
            min,
            width: 0.0,
        }
    }
}

/// A row of table cells.
#[derive(Clone, Debug)]
pub struct Row<'a> {
    pub(crate) cells: Vec<Cell<'a>>,
    pub(crate) class: Cow<'a, str>,
    pub(crate) attrs: Vec<Attr<'a>>,
    pub(crate) heat: Option<f64>,
    pub(crate) size: f64,
    pub(crate) pad_y: f64,
    pub(crate) height: f64,
}

impl<'a> Row<'a> {
    /// Creates a row with the normal minimum line height.
    #[must_use]
    pub fn new(cells: impl IntoIterator<Item = Cell<'a>>) -> Self {
        Self::with_size(cells, FONT_SIZE, CELL_PAD_Y)
    }

    /// Creates a row with the compact minimum line height.
    ///
    /// A non-empty cell in a larger-font column expands the row to fit it.
    #[must_use]
    pub fn small(cells: impl IntoIterator<Item = Cell<'a>>) -> Self {
        Self::with_size(cells, SMALL_SIZE, SMALL_PAD_Y)
    }

    fn with_size(cells: impl IntoIterator<Item = Cell<'a>>, size: f64, pad_y: f64) -> Self {
        Self {
            cells: cells.into_iter().collect(),
            class: Cow::Borrowed(""),
            attrs: Vec::new(),
            heat: None,
            size,
            pad_y,
            height: 0.0,
        }
    }

    /// Sets the row SVG class value.
    #[must_use]
    pub fn class(mut self, class: impl Into<Cow<'a, str>>) -> Self {
        self.class = class.into();
        self
    }

    /// Appends an ordered row attribute.
    #[must_use]
    pub fn attr(mut self, attr: Attr<'a>) -> Self {
        self.attrs.push(attr);
        self
    }

    /// Sets the heatmap intensity from zero to one.
    #[must_use]
    pub const fn heat(mut self, heat: f64) -> Self {
        self.heat = Some(heat);
        self
    }
}

/// A fixed-column table rendered inside a block.
#[derive(Clone, Debug)]
pub struct Table<'a> {
    pub(crate) cols: Vec<Col>,
    pub(crate) head: Option<Row<'a>>,
    pub(crate) rows: Vec<Row<'a>>,
    pub(crate) width: f64,
}

impl<'a> Table<'a> {
    /// Creates an empty table with fixed column settings.
    #[must_use]
    pub fn new(cols: impl IntoIterator<Item = Col>) -> Self {
        Self {
            cols: cols.into_iter().collect(),
            head: None,
            rows: Vec::new(),
            width: 0.0,
        }
    }

    /// Sets the optional table header row.
    pub fn set_head(&mut self, row: Row<'a>) {
        self.head = Some(row);
    }

    /// Appends a body row.
    pub fn push(&mut self, row: Row<'a>) {
        self.rows.push(row);
    }
}

/// Text placed below a block at an offset from its left edge.
///
/// Labels are overlays and do not affect the layout or SVG bounds. Leave enough
/// padding in [`ionize::Opts`] for their position and text.
#[derive(Clone, Debug)]
pub struct Label<'a> {
    pub(crate) x: f64,
    pub(crate) text: Text<'a>,
}

impl<'a> Label<'a> {
    /// Creates a label at the given block-relative horizontal offset.
    #[must_use]
    pub fn new(x: f64, text: impl Into<Text<'a>>) -> Self {
        Self {
            x,
            text: text.into(),
        }
    }
}

/// Content rendered for one graph node.
#[derive(Clone, Debug)]
pub struct Block<'a> {
    pub(crate) title: Text<'a>,
    pub(crate) class: Cow<'a, str>,
    pub(crate) body_class: Cow<'a, str>,
    pub(crate) attrs: Vec<Attr<'a>>,
    pub(crate) table: Table<'a>,
    pub(crate) labels: Vec<Label<'a>>,
}

impl<'a> Block<'a> {
    /// Creates a block with the default header and body style.
    #[must_use]
    pub fn new(title: impl Into<Text<'a>>, table: Table<'a>) -> Self {
        Self {
            title: title.into(),
            class: Cow::Borrowed(""),
            body_class: Cow::Borrowed(""),
            attrs: Vec::new(),
            table,
            labels: Vec::new(),
        }
    }

    /// Sets additional SVG classes on the block.
    ///
    /// The built-in `loop` class colors the header green.
    #[must_use]
    pub fn class(mut self, class: impl Into<Cow<'a, str>>) -> Self {
        self.class = class.into();
        self
    }

    /// Sets additional SVG classes on the body border.
    ///
    /// The built-in `split` class makes the border dashed.
    #[must_use]
    pub fn body_class(mut self, class: impl Into<Cow<'a, str>>) -> Self {
        self.body_class = class.into();
        self
    }

    /// Adds an attribute at the end of the block's SVG attribute list.
    #[must_use]
    pub fn attr(mut self, attr: Attr<'a>) -> Self {
        self.attrs.push(attr);
        self
    }

    /// Appends an overlay label below the block.
    #[must_use]
    pub fn label(mut self, label: Label<'a>) -> Self {
        self.labels.push(label);
        self
    }
}

/// A graph and the content drawn for each node.
#[derive(Debug)]
pub struct Diagram<'a> {
    pub(crate) graph: Graph,
    pub(crate) blocks: Vec<Block<'a>>,
    pub(crate) title: Text<'a>,
    pub(crate) meta: Option<Meta<'a>>,
    pub(crate) css: Cow<'a, str>,
}

impl<'a> Diagram<'a> {
    /// Pairs each graph node with the block at the same index.
    ///
    /// Rendering replaces node sizes with the measured block sizes.
    ///
    /// # Errors
    ///
    /// Returns an error when the graph and block counts differ.
    pub fn new(graph: Graph, blocks: Vec<Block<'a>>) -> Result<Self> {
        if graph.len() != blocks.len() {
            return Err(Err::new(format!(
                "graph has {} nodes but {} blocks were supplied",
                graph.len(),
                blocks.len()
            )));
        }
        Ok(Self {
            graph,
            blocks,
            title: Text::new(""),
            meta: None,
            css: Cow::Borrowed(""),
        })
    }

    /// Sets the SVG document title.
    #[must_use]
    pub fn title(mut self, title: impl Into<Text<'a>>) -> Self {
        self.title = title.into();
        self
    }

    /// Sets the SVG metadata element.
    #[must_use]
    pub fn meta(mut self, meta: Meta<'a>) -> Self {
        self.meta = Some(meta);
        self
    }

    /// Adds CSS between the built-in structural and state rules.
    ///
    /// Text is measured before CSS is applied, so these rules must not change
    /// fonts, spacing, or other geometry.    
    #[must_use]
    pub fn css(mut self, css: impl Into<Cow<'a, str>>) -> Self {
        self.css = css.into();
        self
    }
}

fn normalize(value: Cow<'_, str>) -> Cow<'_, str> {
    let Some((index, _)) = value.char_indices().find(|(_, ch)| !xml_char(*ch)) else {
        return value;
    };
    let mut clean = String::with_capacity(value.len());
    clean.push_str(&value[..index]);
    for ch in value[index..].chars() {
        clean.push(if xml_char(ch) { ch } else { '\u{FFFD}' });
    }
    Cow::Owned(clean)
}

pub(crate) fn xml_char(ch: char) -> bool {
    matches!(
        ch,
        '\u{9}' | '\u{A}' | '\u{D}' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}'
    )
}
