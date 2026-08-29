// SPDX-License-Identifier: MPL-2.0

use core::fmt;

pub(crate) struct Num(f64);

impl fmt::Display for Num {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0 == 0.0 {
            f.write_str("0")
        } else {
            fmt::Display::fmt(&self.0, f)
        }
    }
}

pub(crate) fn num(value: f64) -> Num {
    Num(value)
}
