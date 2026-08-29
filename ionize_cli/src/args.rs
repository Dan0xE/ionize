// SPDX-License-Identifier: MPL-2.0

use clap::{Parser, ValueEnum};

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum Heat {
    Total,
    #[value(name = "self")]
    SelfCount,
}

#[derive(Debug, Parser)]
#[command(name = "ionize", version, about = "Render an Ion graph as SVG")]
pub struct Args {
    /// Input file, or use - for stdin
    #[arg(value_name = "INPUT")]
    pub input: String,

    /// Output file, or use - for stdout
    #[arg(short, long, value_name = "OUTPUT")]
    pub output: Option<String>,

    /// Function name or index
    #[arg(long, default_value = "0", value_name = "INDEX|NAME")]
    pub func: String,

    /// Pass name or index
    #[arg(long, default_value = "0", value_name = "INDEX|NAME")]
    pub pass: String,

    /// Samples file with `totalLineHits` and `selfLineHits`
    #[arg(long, value_name = "FILE")]
    pub samples: Option<String>,

    /// Sample count used for the LIR heatmap
    #[arg(long, value_enum, default_value_t = Heat::Total)]
    pub heatmap: Heat,
}
