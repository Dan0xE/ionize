// SPDX-License-Identifier: MPL-2.0

//! Pre-encodes the bundled webfonts for direct SVG embedding.

use std::env;
use std::fs;
use std::path::Path;

use base64::{Engine as _, engine::general_purpose::STANDARD};

const FONTS: [(&str, &str); 3] = [
    ("assets/NotoSans-Regular.woff2", "regular.b64"),
    ("assets/NotoSans-Bold.woff2", "bold.b64"),
    ("assets/NotoSansSymbols-Regular.woff2", "symbols.b64"),
];

fn main() {
    let root = env::var_os("CARGO_MANIFEST_DIR").expect("Cargo sets CARGO_MANIFEST_DIR");
    let out = env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR");
    for (src, dst) in FONTS {
        println!("cargo:rerun-if-changed={src}");
        let data = fs::read(Path::new(&root).join(src))
            .unwrap_or_else(|err| panic!("could not read {src}: {err}"));
        assert!(valid_woff2(&data), "bundled webfont {src} is invalid");
        let encoded = STANDARD.encode(data);
        fs::write(Path::new(&out).join(dst), encoded)
            .unwrap_or_else(|err| panic!("could not write {dst}: {err}"));
    }
}

fn valid_woff2(data: &[u8]) -> bool {
    if data.len() < 48 || !data.starts_with(b"wOF2") {
        return false;
    }
    let len = u32::from_be_bytes(data[8..12].try_into().expect("four-byte WOFF2 length"));
    u32::try_from(data.len()) == Ok(len)
}
