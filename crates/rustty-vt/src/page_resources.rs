//! Page resource allocation offsets without native backing memory.
use crate::page_layout::{BitmapLayout, SetLayout};
use crate::screen::Style;

/// The insertion-only phase of native PAGE style decoding.
///
/// Every accepted wire ID retains its reference until all styles and cells
/// have been decoded. No item can die during admission, but reference counts
/// still decide ties when Robin Hood insertion displaces an existing item.
pub(crate) struct StyleAdmission {
    table: Vec<u16>,
    entries: Vec<StyleEntry>,
    capacity: usize,
    max_psl: u8,
}

struct StyleEntry {
    value: Style,
    references: u16,
    psl: u8,
}

impl StyleAdmission {
    pub fn new(layout: SetLayout) -> Self {
        Self {
            table: vec![0; layout.table_cap],
            entries: Vec::new(),
            capacity: layout.cap,
            max_psl: 0,
        }
    }

    /// Default styles need no resource; existing styles take another reference.
    pub fn admit(&mut self, value: Style) -> bool {
        if value == Style::default() {
            return true;
        }
        let hash = value.native_hash() as usize;
        let mask = self.table.len().saturating_sub(1);
        if !self.table.is_empty() {
            for psl in 0..=self.max_psl {
                let id = self.table[hash.wrapping_add(usize::from(psl)) & mask];
                if id == 0 {
                    break;
                }
                let entry = &mut self.entries[usize::from(id) - 1];
                if entry.psl < psl {
                    break;
                }
                if entry.psl == psl && entry.value == value {
                    entry.references += 1;
                    return true;
                }
            }
        }
        // ID zero is reserved. With no releases, the largest PSL never falls,
        // so this is equivalent to native's nonempty psl_stats[31] check.
        if self.max_psl == 31 || self.entries.len() + 1 >= self.capacity {
            return false;
        }

        let new_id = u16::try_from(self.entries.len() + 1).unwrap();
        self.entries.push(StyleEntry {
            value,
            references: 0,
            psl: 0,
        });
        let mut held_id = new_id;
        for distance in 0..self.table.len() - 1 {
            let bucket = hash.wrapping_add(distance) & mask;
            let id = self.table[bucket];
            let held = &self.entries[usize::from(held_id) - 1];
            if id == 0 {
                self.table[bucket] = held_id;
                self.max_psl = self.max_psl.max(held.psl);
                break;
            }
            let resident = &self.entries[usize::from(id) - 1];
            if resident.psl < held.psl
                || (resident.psl == held.psl && resident.references < held.references)
            {
                self.table[bucket] = held_id;
                self.max_psl = self.max_psl.max(held.psl);
                held_id = id;
            }
            self.entries[usize::from(held_id) - 1].psl += 1;
        }
        self.entries[usize::from(new_id) - 1].references = 1;
        true
    }
}

/// Bookkeeping for `terminal/bitmap_allocator.zig`.
///
/// Offsets are relative to this allocator's region, including its bitmap header.
/// The page ledger adds the region's page offset. Callers pass byte lengths;
/// grapheme codepoints occupy four native bytes each.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BitmapAllocator<const CHUNK: usize> {
    bitmaps: Vec<u64>,
    chunks_start: usize,
    search_start: usize,
}

impl<const CHUNK: usize> BitmapAllocator<CHUNK> {
    pub fn new(layout: BitmapLayout) -> Self {
        assert!(CHUNK.is_power_of_two());
        let capacity = layout
            .bitmap_count
            .checked_mul(64)
            .and_then(|chunks| chunks.checked_mul(CHUNK))
            .expect("bitmap capacity overflow");
        assert_eq!(
            layout.total_size,
            layout.chunks_start.checked_add(capacity).unwrap(),
            "bitmap layout must reserve every addressable chunk"
        );
        Self {
            bitmaps: vec![0; layout.bitmap_count],
            chunks_start: layout.chunks_start,
            search_start: 0,
        }
    }

    pub fn bytes_required(byte_len: usize) -> Option<usize> {
        assert!(CHUNK.is_power_of_two());
        byte_len.checked_add(CHUNK - 1).map(|n| n & !(CHUNK - 1))
    }

    /// Reserve a native-sized run, returning None for zero length or exhaustion.
    pub fn alloc(&mut self, byte_len: usize) -> Option<usize> {
        let chunks = Self::bytes_required(byte_len)? / CHUNK;
        if chunks == 0 {
            return None;
        }
        let start = self.search_start.min(self.bitmaps.len());
        let chunk = start * 64 + find_free_chunks(&self.bitmaps[start..], chunks)?;
        self.set_bits(chunk, chunks, true);
        while self.search_start < self.bitmaps.len() && self.bitmaps[self.search_start] == u64::MAX
        {
            self.search_start += 1;
        }
        Some(self.chunks_start + chunk * CHUNK)
    }

    /// Release a live allocation using its original offset and byte length.
    pub fn free(&mut self, offset: usize, byte_len: usize) {
        let start = offset
            .checked_sub(self.chunks_start)
            .expect("resource offset precedes the chunks region");
        assert_eq!(start % CHUNK, 0, "resource offset must start a chunk");
        let len = Self::bytes_required(byte_len).expect("resource length overflow");
        assert!(len > 0, "resource allocation must not be empty");
        assert!(
            start
                .checked_add(len)
                .is_some_and(|end| end <= self.capacity_bytes()),
            "resource allocation extends beyond the chunks region"
        );
        let chunk = start / CHUNK;
        self.set_bits(chunk, len / CHUNK, false);
        self.search_start = self.search_start.min(chunk / 64);
    }

    pub fn capacity_bytes(&self) -> usize {
        self.bitmaps.len() * 64 * CHUNK
    }

    #[cfg(test)]
    fn used_bytes(&self) -> usize {
        self.bitmaps
            .iter()
            .map(|bits| bits.count_ones() as usize)
            .sum::<usize>()
            * CHUNK
    }

    fn set_bits(&mut self, mut chunk: usize, mut count: usize, used: bool) {
        while count > 0 {
            let bit = chunk % 64;
            let bits = count.min(64 - bit);
            let mask = (u64::MAX >> (64 - bits)) << bit;
            if used {
                self.bitmaps[chunk / 64] |= mask;
            } else {
                self.bitmaps[chunk / 64] &= !mask;
            }
            chunk += bits;
            count -= bits;
        }
    }
}

fn find_free_chunks(bitmaps: &[u64], count: usize) -> Option<usize> {
    if count <= 64 {
        // Native small allocations stay within one bitmap word, even when
        // neighboring partial words contain a sufficiently large free span.
        for (index, &bitmap) in bitmaps.iter().enumerate() {
            let free = !bitmap;
            let mut starts = free;
            for shift in 1..count {
                starts &= free >> shift;
            }
            if starts != 0 {
                return Some(index * 64 + starts.trailing_zeros() as usize);
            }
        }
        return None;
    }

    let mut index = 0;
    'search: while index < bitmaps.len() {
        let prefix = bitmaps[index].leading_zeros() as usize;
        if prefix == 0 {
            index += 1;
            continue;
        }
        let first = index * 64 + 64 - prefix;
        let mut remaining = count - prefix;
        index += 1;
        while remaining > 64 {
            if *bitmaps.get(index)? != 0 {
                continue 'search;
            }
            remaining -= 64;
            index += 1;
        }
        // Native assumes the final word exists; exhaustion must remain safe
        // here even when the only available prefix is in the last word.
        if bitmaps.get(index)?.trailing_zeros() as usize >= remaining {
            return Some(first);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page_layout::PageCapacity;
    use crate::screen::Color;

    fn style_set(capacity: u16) -> StyleAdmission {
        StyleAdmission::new(
            PageCapacity {
                styles: capacity,
                ..PageCapacity::STANDARD
            }
            .metadata()
            .unwrap()
            .styles_layout,
        )
    }

    fn numbered_style(number: u32) -> Style {
        Style {
            foreground: Color::Rgb(number as u8, (number >> 8) as u8, (number >> 16) as u8),
            ..Style::default()
        }
    }

    fn styles_in_bucket(mask: u64, bucket: u64, count: usize) -> Vec<Style> {
        (0..0x1000000)
            .map(numbered_style)
            .filter(|style| style.native_hash() & mask == bucket)
            .take(count)
            .collect()
    }

    #[test]
    fn style_admission_reserves_zero_and_reuses_live_values_when_full() {
        for capacity in [0, 1, 2, 3, 4, 8, 16, 64, 128] {
            let mut styles = style_set(capacity);
            assert!(styles.admit(Style::default()));
            assert!(styles.entries.is_empty());
            let live_capacity = styles.capacity.saturating_sub(1);
            // Use distinct home buckets to make capacity the only limit.
            for bucket in 0..live_capacity {
                let value = styles_in_bucket((styles.table.len() - 1) as u64, bucket as u64, 1)[0];
                assert!(styles.admit(value));
                assert!(styles.admit(value));
                assert_eq!(styles.entries[bucket].references, 2);
            }
            let rejected = if styles.table.is_empty() {
                numbered_style(0)
            } else {
                styles_in_bucket((styles.table.len() - 1) as u64, live_capacity as u64, 1)[0]
            };
            assert!(!styles.admit(rejected));
            assert_eq!(styles.entries.len(), live_capacity);
            if let Some(entry) = styles.entries.first() {
                let value = entry.value;
                assert!(styles.admit(value));
                assert_eq!(styles.entries[0].references, 3);
            }
        }
    }

    #[test]
    fn style_admission_collision_limit_rejects_even_unrelated_new_values() {
        for bucket in [0, 250, 255] {
            let mut styles = style_set(256);
            let values = styles_in_bucket(255, bucket, 33);
            for value in &values[..32] {
                assert!(styles.admit(*value));
            }
            assert_eq!(styles.max_psl, 31);
            assert!(styles.entries.len() + 1 < styles.capacity);
            assert!(!styles.admit(values[32]));
            assert!(!styles.admit(styles_in_bucket(255, (bucket + 80) & 255, 1)[0]));
            // Lookup precedes both the PSL and capacity checks.
            for value in values[..32].iter().rev() {
                assert!(styles.admit(*value));
            }
            assert!(styles.entries.iter().all(|entry| entry.references == 2));
        }
    }

    #[test]
    fn style_admission_refcounts_break_displacement_ties() {
        let mut styles = style_set(16);
        let home_zero = styles_in_bucket(15, 0, 2);
        let home_one = styles_in_bucket(15, 1, 2);
        assert!(styles.admit(home_zero[0]));
        for _ in 0..5 {
            assert!(styles.admit(home_one[0]));
        }
        assert!(styles.admit(home_one[1]));
        assert!(styles.admit(home_zero[1]));
        // The displaced, frequently referenced style wins the equal-PSL tie.
        assert_eq!(&styles.table[..4], &[1, 4, 2, 3]);
        assert_eq!(styles.entries[1].psl, 1);
        assert_eq!(styles.entries[2].psl, 2);
        assert_eq!(styles.max_psl, 2);
    }

    fn allocator<const CHUNK: usize>(words: usize) -> BitmapAllocator<CHUNK> {
        BitmapAllocator::new(BitmapLayout {
            total_size: words * (8 + 64 * CHUNK),
            bitmap_count: words,
            bitmap_start: 0,
            chunks_start: words * 8,
        })
    }

    #[test]
    fn native_chunk_rounding_and_small_capacity() {
        for (bytes, rounded) in [(0, 0), (1, 16), (4, 16), (16, 16), (17, 32), (24, 32)] {
            assert_eq!(BitmapAllocator::<16>::bytes_required(bytes), Some(rounded));
        }
        for (bytes, rounded) in [(1, 32), (32, 32), (33, 64)] {
            assert_eq!(BitmapAllocator::<32>::bytes_required(bytes), Some(rounded));
        }
        assert_eq!(BitmapAllocator::<16>::bytes_required(usize::MAX), None);
        let capacity = PageCapacity {
            grapheme_bytes: 48,
            ..PageCapacity::STANDARD
        };
        let layout = capacity.metadata().unwrap().grapheme_alloc_layout;
        let mut arena = BitmapAllocator::<16>::new(layout);
        assert_eq!(arena.capacity_bytes(), 1024);
        for chunk in 0..64 {
            assert_eq!(arena.alloc(1), Some(layout.chunks_start + chunk * 16));
        }
        assert_eq!(arena.alloc(1), None);
        assert_eq!(arena.used_bytes(), arena.capacity_bytes());
        let mut empty = allocator::<32>(0);
        assert_eq!(empty.alloc(1), None);
        assert_eq!((empty.capacity_bytes(), empty.used_bytes()), (0, 0));
    }

    #[test]
    fn native_first_fit_and_search_hint_preserve_partial_words() {
        let mut arena = allocator::<1>(3);
        let first = arena.alloc(1).unwrap();
        for chunk in 1..64 {
            assert_eq!(arena.alloc(1), Some(first + chunk));
        }
        assert_eq!(arena.search_start, 1);
        assert_eq!(arena.alloc(1), Some(first + 64));
        arena.free(first, 1);
        assert_eq!(arena.search_start, 0);
        assert_eq!(arena.alloc(1), Some(first));

        let mut arena = allocator::<1>(2);
        let first = arena.alloc(60).unwrap();
        assert_eq!(arena.alloc(8), Some(first + 64));
        assert_eq!(arena.search_start, 0);
        assert_eq!(arena.alloc(4), Some(first + 60));
        assert_eq!(arena.search_start, 1);
    }

    #[test]
    fn native_multi_chunk_graphemes_free_the_rounded_allocation() {
        let mut arena = allocator::<16>(1);
        let first = arena.alloc(6 * 4).unwrap();
        assert_eq!(arena.alloc(4), Some(first + 32));
        assert_eq!(arena.used_bytes(), 48);
        arena.free(first, 6 * 4);
        assert_eq!(arena.used_bytes(), 16);
        assert_eq!(arena.alloc(4), Some(first));
    }

    #[test]
    fn native_large_spans_cross_words_and_preserve_neighbors_when_freed() {
        let mut arena = allocator::<1>(3);
        let first = arena.alloc(96).unwrap();
        let second = arena.alloc(96).unwrap();
        assert_eq!(second, first + 96);
        assert_eq!(arena.bitmaps, [u64::MAX; 3]);
        arena.free(first, 96);
        assert_eq!(arena.bitmaps, [0, u64::MAX << 32, u64::MAX]);
        assert_eq!(arena.used_bytes(), 96);
        assert_eq!(arena.alloc(96), Some(first));
        arena.free(second, 96);
        assert_eq!(arena.bitmaps, [u64::MAX, u32::MAX as u64, 0]);
        arena.free(first, 96);
        assert_eq!(arena.bitmaps, [0; 3]);

        let first = arena.alloc(56).unwrap();
        assert_eq!(arena.alloc(65), Some(first + 56));
        assert_eq!(arena.bitmaps, [u64::MAX, (1 << 57) - 1, 0]);
    }

    #[test]
    fn native_large_spans_restart_at_the_next_partial_word() {
        let mut arena = allocator::<1>(4);
        arena.bitmaps = vec![u32::MAX as u64, 1 << 10, 0, 0];
        let before = arena.clone();
        // The first free suffix is interrupted by bit 10 of the second word.
        let offset = arena.alloc(100).unwrap();
        assert_eq!(offset, arena.chunks_start + 75);
        assert_eq!(
            arena.bitmaps,
            [u32::MAX as u64, u64::MAX << 10, (1 << 47) - 1, 0]
        );
        arena.free(offset, 100);
        assert_eq!(arena, before);
    }

    #[test]
    fn native_small_spans_do_not_join_holes_across_word_boundaries() {
        let mut arena = allocator::<1>(2);
        let first = arena.alloc(60).unwrap();
        let second = arena.alloc(64).unwrap();
        arena.free(second, 4);
        assert_eq!(arena.capacity_bytes() - arena.used_bytes(), 8);
        let before = arena.clone();
        assert_eq!(arena.alloc(8), None);
        assert_eq!(arena, before);
        assert_eq!(arena.alloc(4), Some(first + 60));
        assert_eq!(arena.alloc(4), Some(second));
    }

    #[test]
    fn exhausted_large_spans_leave_the_bitmap_unchanged() {
        let mut arena = allocator::<1>(1);
        let before = arena.clone();
        for bytes in [0, 65, 128, usize::MAX] {
            assert_eq!(arena.alloc(bytes), None);
            assert_eq!(arena, before);
        }
        let mut arena = allocator::<32>(2);
        arena.alloc(63 * 32).unwrap();
        let before = arena.clone();
        assert_eq!(arena.alloc(66 * 32), None);
        assert_eq!(arena, before);
        assert!(arena.alloc(65 * 32).is_some());
    }
}
