// SPDX-License-Identifier: MPL-2.0

use alloc::{format, string::String, vec::Vec};

use harfrust::{
    Direction, FontRef, GlyphPosition, Script, ShapeOptions, ShapePlan, ShaperData, UnicodeBuffer,
    script,
};
use skrifa::{
    MetadataProvider as _,
    charmap::Charmap,
    instance::{LocationRef, Size as FontSize},
    metrics::Metrics as FontMetrics,
};

use crate::err::{Err, Result};

const REGULAR: &[u8] = include_bytes!("../assets/NotoSans-Regular.ttf");
const BOLD: &[u8] = include_bytes!("../assets/NotoSans-Bold.ttf");
const SYMBOLS: &[u8] = include_bytes!("../assets/NotoSansSymbols-Regular.ttf");
const REGULAR_WEB: &str = include_str!(concat!(env!("OUT_DIR"), "/regular.b64"));
const BOLD_WEB: &str = include_str!(concat!(env!("OUT_DIR"), "/bold.b64"));
const SYMBOLS_WEB: &str = include_str!(concat!(env!("OUT_DIR"), "/symbols.b64"));
const REGULAR_START: &str = "@font-face{font-family:'Ion Noto Sans';font-style:normal;font-weight:400;src:url(data:font/woff2;base64,";
const BOLD_START: &str = "@font-face{font-family:'Ion Noto Sans';font-style:normal;font-weight:700;src:url(data:font/woff2;base64,";
const SYMBOLS_START: &str = "@font-face{font-family:'Ion Noto Symbols';font-style:normal;font-weight:400;src:url(data:font/woff2;base64,";
const END: &str = ") format('woff2')}\n";

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum Weight {
    Regular,
    Bold,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Metrics {
    pub(crate) ascent: f64,
    pub(crate) line: f64,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Kind {
    Regular,
    Bold,
    Symbols,
}

struct Plan {
    dir: Direction,
    script: Option<Script>,
    shape: ShapePlan,
}

struct Font {
    face: FontRef<'static>,
    data: ShaperData,
    cmap: Charmap<'static>,
    metrics: FontMetrics,
    plans: Vec<Plan>,
    buf: UnicodeBuffer,
}

pub(crate) struct Fonts {
    regular: Font,
    bold: Font,
    symbols: Font,
}

impl Fonts {
    pub(crate) fn new() -> Result<Self> {
        let regular = Font::new(REGULAR, "Noto Sans Regular")?;
        let bold = Font::new(BOLD, "Noto Sans Bold")?;
        let symbols = Font::new(SYMBOLS, "Noto Sans Symbols")?;
        Ok(Self {
            regular,
            bold,
            symbols,
        })
    }

    pub(crate) fn metrics(&self, size: f64, weight: Weight) -> Metrics {
        let metrics = match weight {
            Weight::Regular => &self.regular.metrics,
            Weight::Bold => &self.bold.metrics,
        };
        let scale = size / f64::from(metrics.units_per_em);
        let ascent = f64::from(metrics.ascent) * scale;
        let line = f64::from(metrics.ascent - metrics.descent + metrics.leading) * scale;
        Metrics {
            ascent,
            line: line.max(size),
        }
    }

    pub(crate) fn width(&mut self, text: &str, size: f64, weight: Weight) -> f64 {
        let (preferred_kind, preferred) = match weight {
            Weight::Regular => (Kind::Regular, &mut self.regular),
            Weight::Bold => (Kind::Bold, &mut self.bold),
        };
        let symbols = &mut self.symbols;
        let mut width = 0.0;
        let mut kind = None;
        let mut start = 0;

        for (index, ch) in text.char_indices() {
            let next = if !ch.is_ascii()
                && preferred.cmap.map(ch).is_none()
                && symbols.cmap.map(ch).is_some()
            {
                Kind::Symbols
            } else {
                preferred_kind
            };
            if let Some(current) = kind
                && current != next
            {
                width += match current {
                    Kind::Symbols => symbols.run_width(&text[start..index], size),
                    Kind::Regular | Kind::Bold => preferred.run_width(&text[start..index], size),
                };
                start = index;
            }
            kind = Some(next);
        }
        if let Some(kind) = kind {
            width += match kind {
                Kind::Symbols => symbols.run_width(&text[start..], size),
                Kind::Regular | Kind::Bold => preferred.run_width(&text[start..], size),
            };
        }
        width
    }

    pub(crate) fn css_len() -> usize {
        REGULAR_WEB.len()
            + BOLD_WEB.len()
            + SYMBOLS_WEB.len()
            + REGULAR_START.len()
            + BOLD_START.len()
            + SYMBOLS_START.len()
            + END.len() * 3
    }

    pub(crate) fn css(out: &mut String) {
        out.reserve(Self::css_len());

        out.push_str(REGULAR_START);
        out.push_str(REGULAR_WEB);
        out.push_str(END);
        out.push_str(BOLD_START);
        out.push_str(BOLD_WEB);
        out.push_str(END);
        out.push_str(SYMBOLS_START);
        out.push_str(SYMBOLS_WEB);
        out.push_str(END);
    }
}

impl Font {
    fn new(data: &'static [u8], name: &str) -> Result<Self> {
        let face =
            FontRef::new(data).map_err(|_| Err::new(format!("bundled font {name} is invalid")))?;
        let shaper = ShaperData::new(&face);
        let cmap = face.charmap();
        let metrics = face.metrics(FontSize::unscaled(), LocationRef::default());
        Ok(Self {
            face,
            data: shaper,
            cmap,
            metrics,
            plans: Vec::new(),
            buf: UnicodeBuffer::new(),
        })
    }

    fn run_width(&mut self, text: &str, size: f64) -> f64 {
        let mut buf = core::mem::take(&mut self.buf);
        buf.push_str(text);
        buf.guess_segment_properties();
        let dir = buf.direction();
        let guessed = buf.script();
        // UNKNOWN means the segment has no inferred script.
        let script = (guessed != script::UNKNOWN).then_some(guessed);
        let shaper = self.data.shaper(&self.face).build();
        let index = if let Some(index) = self
            .plans
            .iter()
            .position(|plan| plan.dir == dir && plan.script == script)
        {
            index
        } else {
            self.plans.push(Plan {
                dir,
                script,
                shape: ShapePlan::new(&shaper, dir, script, None, &[]),
            });
            self.plans.len() - 1
        };
        let glyphs = shaper.shape(
            buf,
            ShapeOptions::new().plan(Some(&self.plans[index].shape)),
        );
        let units = sum_units(glyphs.glyph_positions());
        self.buf = glyphs.clear();
        #[allow(clippy::cast_precision_loss)]
        let units = units as f64;
        units * size / f64::from(self.metrics.units_per_em)
    }
}

fn sum_units(pos: &[GlyphPosition]) -> i64 {
    pos.iter().map(|pos| i64::from(pos.x_advance)).sum()
}
