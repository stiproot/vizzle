//! Text metrics: Arial-like glyph advances, word wrap, and the escaping,
//! rounding and number printing every emitter shares.
//!
//! The picture is deterministic because nothing here measures a font: widths
//! come from a fixed table that leans wide on purpose. The arithmetic is kept
//! in the generator's original order, because the SVG must come out
//! byte-identical to the one it replaced (`docs/plans/impl/hybrid.md` §5.1),
//! and a reordered float sum can round the other way.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Advance widths per 1000 em. Built by replaying the original assignments in
/// order — later ones override earlier ones (`S` ends at 667, `L` at 556), and
/// reading it as a single table would hide that.
fn advances() -> &'static HashMap<char, u32> {
    static TABLE: OnceLock<HashMap<char, u32>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut w = HashMap::new();
        let mut set_w = |chars: &str, width: u32| {
            for c in chars.chars() {
                w.insert(c, width);
            }
        };
        set_w(" !,./:;I[\\]ft", 278);
        set_w("ijl", 222);
        set_w("|", 260);
        set_w("'", 191);
        set_w("\"", 355);
        set_w("()-`r", 333);
        set_w("{}", 334);
        set_w("*", 389);
        set_w("^", 469);
        set_w("#$0123456789?_abdeghnopqu«»–", 556);
        set_w("ckszvxyJ", 500);
        set_w("+<=>~", 584);
        set_w("FTZ", 611);
        set_w("LS", 556);
        set_w("ABEKPSVXY&", 667);
        set_w("CDHNRUw", 722);
        set_w("GOQ", 778);
        set_w("mM", 833);
        set_w("%", 889);
        set_w("W", 944);
        set_w("@", 1015);
        set_w("—→…⇢", 1000);
        set_w("·", 278);
        set_w("S", 667);
        set_w("L", 556);
        w
    })
}

/// An unknown glyph's advance.
const DEFAULT_ADVANCE: u32 = 600;
/// Monospace text (a tile's command line): every glyph is ~0.6 em in Menlo /
/// SF Mono; lean wide.
const MONO_EM: f64 = 0.6;
pub const MONO_FONT: &str = "ui-monospace,SFMono-Regular,Menlo,Consolas,monospace";

pub fn text_width(s: &str, size: f64, bold: bool, mono: bool) -> f64 {
    if mono {
        return s.chars().count() as f64 * MONO_EM * size;
    }
    let table = advances();
    let mut u = 0u32;
    for c in s.chars() {
        u += table.get(&c).copied().unwrap_or(DEFAULT_ADVANCE);
    }
    (u as f64 / 1000.0) * size * if bold { 1.13 } else { 1.0 }
}

/// Greedy word wrap on single spaces. An empty string wraps to no lines.
pub fn wrap(s: &str, max_w: f64, size: f64, bold: bool, mono: bool) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for w in s.split(' ') {
        let t = if cur.is_empty() {
            w.to_owned()
        } else {
            format!("{cur} {w}")
        };
        if !cur.is_empty() && text_width(&t, size, bold, mono) > max_w {
            lines.push(std::mem::replace(&mut cur, w.to_owned()));
        } else {
            cur = t;
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

/// Escape text for an SVG text node: `&`, `<`, `>` (attributes are never
/// built from spec text).
pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// JavaScript's `Math.round`: halves round up (towards +∞), so -2.5 → -2,
/// where Rust's `f64::round` would give -3.
pub fn r(n: f64) -> f64 {
    let f = n.floor();
    if n - f >= 0.5 {
        f + 1.0
    } else {
        f
    }
}

/// A number as JavaScript prints it in a template string: integers without a
/// fraction, `-0` as `0`, everything else as the shortest round-trip decimal.
/// (JavaScript switches to exponent form below 1e-6 and from 1e21; no
/// coordinate gets there.)
pub fn num(n: f64) -> String {
    if n == 0.0 {
        "0".to_owned()
    } else if n.is_nan() {
        "NaN".to_owned()
    } else if n.is_infinite() {
        if n > 0.0 { "Infinity" } else { "-Infinity" }.to_owned()
    } else if n.fract() == 0.0 && n.abs() < 1e21 {
        format!("{n:.0}")
    } else {
        format!("{n}")
    }
}

/// `num(r(n))`: the form nearly every coordinate is printed in.
pub fn rn(n: f64) -> String {
    num(r(n))
}

/// The largest of `values`, or -∞ for none (as `Math.max()` of nothing).
pub fn max_of(values: impl IntoIterator<Item = f64>) -> f64 {
    values.into_iter().fold(f64::NEG_INFINITY, f64::max)
}

/// The smallest of `values`, or +∞ for none.
pub fn min_of(values: impl IntoIterator<Item = f64>) -> f64 {
    values.into_iter().fold(f64::INFINITY, f64::min)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths_follow_the_last_assignment_of_a_glyph() {
        assert_eq!(text_width("S", 1000.0, false, false), 667.0);
        assert_eq!(text_width("L", 1000.0, false, false), 556.0);
        assert_eq!(text_width("\u{1F600}", 1000.0, false, false), 600.0);
    }

    #[test]
    fn wraps_on_spaces_and_an_empty_string_to_nothing() {
        assert_eq!(
            wrap("aa bb cc", 30.0, 17.0, false, false),
            vec!["aa", "bb", "cc"]
        );
        assert_eq!(wrap("aa bb", 1000.0, 17.0, false, false), vec!["aa bb"]);
        assert!(wrap("", 100.0, 17.0, false, false).is_empty());
    }

    #[test]
    fn rounds_and_prints_as_javascript_does() {
        assert_eq!(r(-2.5), -2.0);
        assert_eq!(r(2.5), 3.0);
        assert_eq!(rn(-0.2), "0");
        assert_eq!(num(700.0), "700");
        assert_eq!(num(700.5), "700.5");
        assert_eq!(num(0.1 + 0.2), "0.30000000000000004");
    }
}
