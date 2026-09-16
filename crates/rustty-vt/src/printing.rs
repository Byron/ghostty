//! Pure scans and stores for ordinary packed-cell runs. Stores follow admission.
use crate::packed::Cell;

pub(crate) const DEST_MASK: u64 = 3 | Cell::STYLE_MASK | Cell::WIDE_MASK | Cell::HYPERLINK_MASK;

#[cfg(not(all(
    target_arch = "aarch64",
    target_feature = "neon",
    not(feature = "scalar-kernels")
)))]
pub(crate) use scalar::*;
#[cfg(all(
    target_arch = "aarch64",
    target_feature = "neon",
    not(feature = "scalar-kernels")
))]
pub(crate) use simd::*;

mod scalar {
    use super::*;

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
                pair[0].bits() & DEST_MASK != expected[0]
                    || pair[1].bits() & DEST_MASK != expected[1]
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
}

#[cfg(all(
    target_arch = "aarch64",
    target_feature = "neon",
    not(feature = "scalar-kernels")
))]
mod simd {
    use super::*;
    use bytemuck::cast;
    use wide::{u32x4, u64x2};

    const DESTINATION_MASK: u64x2 = u64x2::new([DEST_MASK; 2]);

    #[inline]
    pub(crate) fn printable_prefix(properties: &[u32], width: u8, graphemes: bool) -> usize {
        let mask = u32x4::splat(if graphemes { 3 | (31 << 3) } else { 3 });
        let expected = u32x4::splat(u32::from(width));
        let (chunks, tail) = properties.as_chunks::<4>();
        for (i, chunk) in chunks.iter().enumerate() {
            let matched = (u32x4::new(*chunk) & mask).simd_eq(expected);
            if !matched.all() {
                return i * 4 + matched.to_bitmask().trailing_ones() as usize;
            }
        }
        chunks.len() * 4 + scalar::printable_prefix(tail, width, graphemes)
    }

    #[inline]
    pub(crate) fn destination_narrow(cells: &[Cell], expected: u64) -> usize {
        // Compare both halves while accepting ordinary Cell alignment.
        let mask: u32x4 = cast(DESTINATION_MASK);
        let wanted: u32x4 = cast(u64x2::splat(expected));
        let (chunks, tail) = cells.as_chunks::<2>();
        for (i, chunk) in chunks.iter().enumerate() {
            // Value casts permit ordinary Cell alignment, including odd offsets.
            let words: u32x4 = cast(*chunk);
            let matched = (words & mask).simd_eq(wanted);
            if !matched.all() {
                return i * 2 + matched.to_bitmask().trailing_ones() as usize / 2;
            }
        }
        chunks.len() * 2 + scalar::destination_narrow(tail, expected)
    }

    #[inline]
    pub(crate) fn destination_wide(cells: &[Cell], expected: [u64; 2]) -> usize {
        let mask: u32x4 = cast(DESTINATION_MASK);
        let wanted: u32x4 = cast(expected);
        let (chunks, tail) = cells.as_chunks::<2>();
        for (i, chunk) in chunks.iter().enumerate() {
            let words: u32x4 = cast(*chunk);
            if !(words & mask).simd_eq(wanted).all() {
                return i * 2;
            }
        }
        chunks.len() * 2 + scalar::destination_wide(tail, expected)
    }

    #[inline]
    pub(crate) fn store_ascii(cells: &mut [Cell], bytes: &[u8], template: Cell) {
        assert_eq!(cells.len(), bytes.len());
        let packed = u64x2::splat(template.bits());
        let (chunks, tail) = cells.as_chunks_mut::<2>();
        let (input, rest) = bytes.as_chunks::<2>();
        for (out, cp) in chunks.iter_mut().zip(input) {
            *out = cast(packed | (u64x2::new(cp.map(u64::from)) << 2u32));
        }
        scalar::store_ascii(tail, rest, template);
    }

    #[inline]
    pub(crate) fn store_narrow(cells: &mut [Cell], codepoints: &[char], template: Cell) {
        assert_eq!(cells.len(), codepoints.len());
        let packed = u64x2::splat(template.bits());
        let (chunks, tail) = cells.as_chunks_mut::<2>();
        let (input, rest) = codepoints.as_chunks::<2>();
        for (out, cp) in chunks.iter_mut().zip(input) {
            *out = cast(packed | (u64x2::new(cp.map(u64::from)) << 2u32));
        }
        scalar::store_narrow(tail, rest, template);
    }

    #[inline]
    pub(crate) fn store_wide(cells: &mut [Cell], codepoints: &[char], head: Cell, tail: Cell) {
        assert_eq!(cells.len(), codepoints.len() * 2);
        let heads = u64x2::splat(head.bits());
        let tails = u64x2::splat(tail.bits());
        let (chunks, remainder) = cells.as_chunks_mut::<4>();
        let (input, rest) = codepoints.as_chunks::<2>();
        for (out, cp) in chunks.iter_mut().zip(input) {
            let heads = heads | (u64x2::new(cp.map(u64::from)) << 2u32);
            let pairs = [heads.unpack_lo(tails), heads.unpack_hi(tails)];
            *out = cast(pairs);
        }
        scalar::store_wide(remainder, rest, head, tail);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn printable_scan_matches_scalar_at_every_boundary() {
        for len in 0..=65 {
            for offset in 0..4 {
                for width in [1, 2] {
                    for graphemes in [false, true] {
                        let mut input = vec![0; offset + len + 4];
                        let properties = &mut input[offset..offset + len];
                        for (i, p) in properties.iter_mut().enumerate() {
                            *p = u32::from(width)
                                | if graphemes {
                                    (i as u32 & 1) << 8
                                } else {
                                    (i as u32) << 2
                                };
                        }
                        assert_eq!(printable_prefix(properties, width, graphemes), len);
                        for i in 0..len {
                            for bit in 0..9 {
                                properties[i] ^= 1 << bit;
                                assert_eq!(
                                    printable_prefix(properties, width, graphemes),
                                    scalar::printable_prefix(properties, width, graphemes),
                                    "length={len}, offset={offset}, at={i}, bit={bit}"
                                );
                                properties[i] ^= 1 << bit;
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn destination_scans_match_scalar_for_every_field_and_mismatch() {
        for len in 0..=65 {
            for offset in 0..4 {
                for style in [0, 1, u16::MAX] {
                    let style = u64::from(style) << 26;
                    for expected in [
                        [style; 2],
                        [style | (1 << 42), style | (2 << 42)],
                        [style | 3, style | 3],
                        [style | Cell::HYPERLINK_MASK; 2],
                    ] {
                        let mut input = vec![Cell::default(); offset + len + 4];
                        let cells = &mut input[offset..offset + len];
                        for (i, cell) in cells.iter_mut().enumerate() {
                            // Different codepoints, protection and semantics do not
                            // prevent a homogeneous overwrite of the resource fields.
                            let ignored = ((i as u64) << 2) | ((i as u64 & 7) << 44);
                            *cell = Cell::from_bits(expected[i % 2] | (ignored & !DEST_MASK));
                        }
                        assert_eq!(destination_wide(cells, expected), len / 2 * 2);
                        assert_eq!(
                            destination_narrow(cells, expected[0]),
                            scalar::destination_narrow(cells, expected[0])
                        );
                        for i in 0..len {
                            for bit in 0..48 {
                                cells[i] = Cell::from_bits(cells[i].bits() ^ (1 << bit));
                                assert_eq!(
                                    destination_narrow(cells, expected[0]),
                                    scalar::destination_narrow(cells, expected[0]),
                                    "narrow length={len}, offset={offset}, at={i}, bit={bit}"
                                );
                                assert_eq!(
                                    destination_wide(cells, expected),
                                    scalar::destination_wide(cells, expected),
                                    "wide length={len}, offset={offset}, at={i}, bit={bit}"
                                );
                                cells[i] = Cell::from_bits(cells[i].bits() ^ (1 << bit));
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn stores_match_scalar_and_preserve_output_sentinels() {
        let alphabet = ['\0', 'a', 'é', '界', '\u{fe0f}', '😀', '\u{10ffff}'];
        let sentinel = Cell::from_bits(0xa55a_ffff_1234);
        for len in (0..=65).chain([127, 128, 129, 255, 256, 257, 513]) {
            for offset in 0..8 {
                let chars: Vec<_> = (0..offset + len + 8)
                    .map(|i| alphabet[i % alphabet.len()])
                    .collect();
                let bytes: Vec<_> = (0..chars.len()).map(|i| 32 + (i % 95) as u8).collect();
                let codepoints = &chars[offset..offset + len];
                let bytes = &bytes[offset..offset + len];
                for style in [0, 1, u16::MAX] {
                    for flags in 0..8u64 {
                        let template = Cell::from_bits(
                            (u64::from(style) << 26) | ((flags & 1) << 44) | ((flags >> 1) << 46),
                        );
                        let mut actual = vec![sentinel; offset + len + 8];
                        let mut expected = actual.clone();
                        scalar::store_ascii(&mut expected[offset..offset + len], bytes, template);
                        store_ascii(&mut actual[offset..offset + len], bytes, template);
                        assert_eq!(actual, expected, "ASCII length={len}, offset={offset}");
                        scalar::store_narrow(
                            &mut expected[offset..offset + len],
                            codepoints,
                            template,
                        );
                        store_narrow(&mut actual[offset..offset + len], codepoints, template);
                        assert_eq!(actual, expected, "narrow length={len}, offset={offset}");

                        let head = Cell::from_bits(template.bits() | (1 << 42));
                        let tail = Cell::from_bits(template.bits() | (2 << 42));
                        let mut actual = vec![sentinel; offset + len * 2 + 8];
                        let mut expected = actual.clone();
                        scalar::store_wide(
                            &mut expected[offset..offset + len * 2],
                            codepoints,
                            head,
                            tail,
                        );
                        store_wide(
                            &mut actual[offset..offset + len * 2],
                            codepoints,
                            head,
                            tail,
                        );
                        assert_eq!(actual, expected, "wide length={len}, offset={offset}");
                    }
                }
            }
        }
    }
}
