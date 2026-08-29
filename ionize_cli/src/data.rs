// SPDX-License-Identifier: MPL-2.0

// The implementation mostly used:
// https://github.com/mozilla-spidermonkey/iongraph/blob/main/src/iongraph.ts for reference
//
// We currently have some limitations in our implementation:
// - Cannot migrate to v1 from old format
// - Unclear if we will ever support anything beyond v1

use std::collections::BTreeMap;
use std::fmt;
use std::marker::PhantomData;

use serde::de::{IgnoredAny, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use crate::err::{Err, Result};

#[derive(Debug, Deserialize)]
pub struct Ion {
    pub functions: Vec<Func>,
}

#[derive(Debug, Deserialize)]
pub struct Func {
    pub name: String,
    pub passes: Vec<Pass>,
}

#[derive(Debug, Deserialize)]
pub struct Pass {
    pub name: String,
    pub mir: Mir,
    pub lir: Lir,
}

#[derive(Debug, Deserialize)]
pub struct Mir {
    pub blocks: Vec<MirBlock>,
}

#[derive(Debug, Deserialize)]
pub struct Lir {
    pub blocks: Vec<LirBlock>,
}

#[derive(Debug, Deserialize)]
pub struct MirBlock {
    pub ptr: u64,
    pub id: u64,
    #[serde(rename = "loopDepth")]
    pub depth: usize,
    pub attributes: Vec<String>,
    pub predecessors: Vec<u64>,
    pub successors: Vec<u64>,
    pub instructions: Vec<MirIns>,
}

#[derive(Debug, Deserialize)]
pub struct MirIns {
    pub ptr: u64,
    pub id: u64,
    #[serde(deserialize_with = "xml_text")]
    pub opcode: String,
    pub attributes: Vec<String>,
    #[serde(rename = "inputs")]
    _inputs: Discard<f64>,
    #[serde(rename = "uses")]
    _uses: Discard<f64>,
    #[serde(rename = "memInputs")]
    _mem_inputs: Discard<IgnoredAny>,
    #[serde(rename = "type", deserialize_with = "xml_text")]
    pub ty: String,
}

#[derive(Debug, Deserialize)]
pub struct LirBlock {
    pub ptr: u64,
    pub id: u64,
    pub instructions: Vec<LirIns>,
}

#[derive(Debug, Deserialize)]
pub struct LirIns {
    pub ptr: u64,
    pub id: u64,
    #[serde(rename = "mirPtr")]
    pub mir_ptr: Option<u64>,
    #[serde(deserialize_with = "xml_text")]
    pub opcode: String,
    #[serde(rename = "defs")]
    _defs: Discard<f64>,
}

#[derive(Debug)]
struct Discard<T>(PhantomData<fn() -> T>);

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Discard<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        deserializer.deserialize_seq(Self(PhantomData))
    }
}

impl<'de, T: Deserialize<'de>> Visitor<'de> for Discard<T> {
    type Value = Self;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a sequence")
    }

    fn visit_seq<A: SeqAccess<'de>>(
        self,
        mut seq: A,
    ) -> std::result::Result<Self::Value, A::Error> {
        while seq.next_element::<T>()?.is_some() {}
        Ok(self)
    }
}

pub struct Sel {
    pub func_name: String,
    pub pass: Pass,
    pub func_idx: usize,
    pub pass_idx: usize,
}

#[derive(Debug, Default)]
pub struct Samples {
    pub counts: BTreeMap<u64, Counts>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Counts {
    pub total: f64,
    pub self_count: f64,
}

impl Samples {
    pub fn get(&self, id: u64) -> Counts {
        self.counts.get(&id).copied().unwrap_or_default()
    }
}

#[derive(Deserialize)]
struct RawSamples {
    #[serde(rename = "totalLineHits")]
    total: Vec<(u64, f64)>,
    #[serde(rename = "selfLineHits")]
    self_count: Vec<(u64, f64)>,
}

pub fn parse(src: &str) -> Result<Ion> {
    let value: Value = serde_json::from_str(src)
        .map_err(|err| Err::new(format!("could not parse ion.json: {err}")))?;

    let Some(version) = value.get("version") else {
        return Err(Err::new("ion.json must explicitly declare version 1"));
    };

    // NOTE: we currently only support the 1.0 ion format.
    // If we will ever support anything beyond 1.0 isn't clear yet.
    if version.as_f64() != Some(1.0) {
        return Err(Err::new(format!(
            "unsupported ion.json version {version}; expected 1"
        )));
    }

    serde_json::from_value(value)
        .map_err(|err| Err::new(format!("invalid ion.json version 1 data: {err}")))
}

pub fn parse_samples(src: &str) -> Result<Samples> {
    let raw: RawSamples = serde_json::from_str(src)
        .map_err(|err| Err::new(format!("could not parse sample counts: {err}")))?;
    let mut samples = Samples::default();
    add_counts("totalLineHits", raw.total, &mut samples.counts, |counts| {
        &mut counts.total
    })?;
    add_counts(
        "selfLineHits",
        raw.self_count,
        &mut samples.counts,
        |counts| &mut counts.self_count,
    )?;
    Ok(samples)
}

pub fn select(ion: Ion, func: &str, pass: &str) -> Result<Sel> {
    let mut functions = ion.functions;
    let func_idx = pick(&functions, func, "function", |item| &item.name)?;
    let Func {
        name: func_name,
        mut passes,
    } = functions.swap_remove(func_idx);
    drop(functions);

    let pass_idx = pick(&passes, pass, "pass", |item| &item.name)?;
    let pass = passes.swap_remove(pass_idx);
    Ok(Sel {
        func_name,
        pass,
        func_idx,
        pass_idx,
    })
}

fn pick<T>(items: &[T], value: &str, kind: &str, name: impl Fn(&T) -> &str) -> Result<usize> {
    if let Ok(index) = value.parse::<usize>() {
        return (index < items.len()).then_some(index).ok_or_else(|| {
            Err::new(format!(
                "{kind} index {index} is out of range; found {} {kind}s",
                items.len()
            ))
        });
    }

    let mut found = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| (name(item) == value).then_some(index));
    let Some(index) = found.next() else {
        return Err(Err::new(format!("no {kind} is named {value:?}")));
    };
    if found.next().is_some() {
        return Err(Err::new(format!(
            "{kind} name {value:?} is ambiguous; use its zero-based index"
        )));
    }
    Ok(index)
}

fn add_counts(
    name: &str,
    pairs: Vec<(u64, f64)>,
    counts: &mut BTreeMap<u64, Counts>,
    field: impl Fn(&mut Counts) -> &mut f64,
) -> Result<()> {
    for (id, count) in pairs {
        if !count.is_finite() || count < 0.0 {
            return Err(Err::new(format!(
                "{name} count for instruction {id} must be finite and non-negative"
            )));
        }
        *field(counts.entry(id).or_default()) = count;
    }
    Ok(())
}

#[inline]
pub(crate) fn is_xml_char(ch: char) -> bool {
    matches!(
        ch,
        '\u{9}' | '\u{A}' | '\u{D}' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}'
    )
}

fn xml_text<'de, D: Deserializer<'de>>(deserializer: D) -> std::result::Result<String, D::Error> {
    let text = String::deserialize(deserializer)?;
    let Some((index, _)) = text.char_indices().find(|(_, ch)| !is_xml_char(*ch)) else {
        return Ok(text);
    };

    let mut normalized = String::with_capacity(text.len());
    normalized.push_str(&text[..index]);
    for ch in text[index..].chars() {
        normalized.push(if is_xml_char(ch) { ch } else { '\u{FFFD}' });
    }
    Ok(normalized)
}
