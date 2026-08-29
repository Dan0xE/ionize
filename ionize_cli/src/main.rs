// SPDX-License-Identifier: MPL-2.0

//! Turns Ion graphs into SVG.

mod args;
mod data;
mod draw;
mod err;

use std::fs;
use std::io::{self, Read as _, Write as _};
use std::process::ExitCode;

use clap::Parser as _;
use ionize::Opts;
use ionize_draw::Renderer;
use mimalloc::MiMalloc;

use crate::args::Args;
use crate::err::{Err, Result};

#[global_allocator]
static ALLOC: MiMalloc = MiMalloc;

fn main() -> ExitCode {
    let args = Args::parse();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("ionize: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &Args) -> Result<()> {
    let input = read_input(&args.input)?;
    let ion = data::parse(&input)?;
    drop(input);
    let sel = data::select(ion, &args.func, &args.pass)?;
    let samples = args
        .samples
        .as_deref()
        .map(|path| {
            fs::read_to_string(path)
                .map_err(|err| Err::new(format!("could not read samples file {path:?}: {err}")))
                .and_then(|src| data::parse_samples(&src))
        })
        .transpose()?;
    let diagram = draw::build(&sel, samples.as_ref(), args.heatmap)?;
    let mut renderer = Renderer::new().map_err(draw_err)?;
    let output = renderer
        .render(diagram, &Opts::default())
        .map_err(draw_err)?;
    write_output(args.output.as_deref(), output.as_bytes())
}

fn draw_err(err: ionize_draw::Err) -> Err {
    match err {
        ionize_draw::Err::Layout(err) => Err::new(format!("could not lay out graph: {err}")),
        err => Err::new(err.to_string()),
    }
}

fn read_input(path: &str) -> Result<String> {
    if path == "-" {
        let mut input = String::new();
        io::stdin()
            .read_to_string(&mut input)
            .map_err(|err| Err::new(format!("could not read standard input: {err}")))?;
        Ok(input)
    } else {
        fs::read_to_string(path)
            .map_err(|err| Err::new(format!("could not read input file {path:?}: {err}")))
    }
}

fn write_output(path: Option<&str>, bytes: &[u8]) -> Result<()> {
    match path {
        None | Some("-") => io::stdout()
            .lock()
            .write_all(bytes)
            .map_err(|err| Err::new(format!("could not write standard output: {err}"))),
        Some(path) => fs::write(path, bytes)
            .map_err(|err| Err::new(format!("could not write output file {path:?}: {err}"))),
    }
}
