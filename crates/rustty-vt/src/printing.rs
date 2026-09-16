//! Pure scans and stores for ordinary packed-cell runs. Stores follow admission.
use crate::packed::Cell;

pub(crate) const DEST_MASK: u64 = 3 | Cell::STYLE_MASK | Cell::WIDE_MASK | Cell::HYPERLINK_MASK;

#[inline]
pub(crate) fn printable_prefix(properties: &[u32], width: u8, graphemes: bool) -> usize {
    let mask = if graphemes { 3 | (31 << 3) } else { 3 };
    properties
        .iter()
        .position(|&p| p & mask != u32::from(width))
        .unwrap_or(properties.len())
}

#[inline]
pub(crate) fn destination_narrow(cells: &[Cell], expected: u64) -> usize {
    cells
        .iter()
        .position(|cell| cell.bits() & DEST_MASK != expected)
        .unwrap_or(cells.len())
}

#[inline]
pub(crate) fn destination_wide(cells: &[Cell], expected: [u64; 2]) -> usize {
    let pairs = cells.as_chunks::<2>().0;
    2 * pairs
        .iter()
        .position(|pair| {
            pair[0].bits() & DEST_MASK != expected[0] || pair[1].bits() & DEST_MASK != expected[1]
        })
        .unwrap_or(pairs.len())
}

#[inline]
pub(crate) fn store_ascii(cells: &mut [Cell], bytes: &[u8], template: Cell) {
    assert_eq!(cells.len(), bytes.len());
    for (cell, &byte) in cells.iter_mut().zip(bytes) {
        *cell = Cell::from_bits(template.bits() | (u64::from(byte) << 2));
    }
}

#[inline]
pub(crate) fn store_narrow(cells: &mut [Cell], codepoints: &[char], template: Cell) {
    assert_eq!(cells.len(), codepoints.len());
    for (cell, &cp) in cells.iter_mut().zip(codepoints) {
        *cell = Cell::from_bits(template.bits() | (u64::from(cp) << 2));
    }
}

#[inline]
pub(crate) fn store_wide(cells: &mut [Cell], codepoints: &[char], head: Cell, tail: Cell) {
    assert_eq!(cells.len(), codepoints.len() * 2);
    for (pair, &cp) in cells.as_chunks_mut::<2>().0.iter_mut().zip(codepoints) {
        pair[0] = Cell::from_bits(head.bits() | (u64::from(cp) << 2));
        pair[1] = tail;
    }
}
