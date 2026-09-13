//! Page resource allocation offsets without native backing memory.
use crate::page_layout::BitmapLayout;

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
