use serde::{Deserialize, Serialize};

/// A byte range [start, start + length).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ByteRange {
    pub start: u64,
    pub length: u64,
}

impl ByteRange {
    pub fn new(start: u64, length: u64) -> Self {
        Self { start, length }
    }

    pub fn end(&self) -> u64 {
        self.start + self.length
    }

    #[allow(dead_code)]
    fn overlaps(&self, other: &ByteRange) -> bool {
        self.start < other.end() && other.start < self.end()
    }

    fn is_adjacent_or_overlapping(&self, other: &ByteRange) -> bool {
        self.start <= other.end() && other.start <= self.end()
    }
}

/// Tracks downloaded byte ranges as an ordered list of non-overlapping ranges.
/// Ranges are automatically merged when they overlap or are adjacent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RangeSet {
    ranges: Vec<ByteRange>,
}

impl RangeSet {
    pub fn new() -> Self {
        Self { ranges: Vec::new() }
    }

    /// Number of tracked ranges.
    pub fn len(&self) -> usize {
        self.ranges.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    /// The tracked ranges, guaranteed non-overlapping and sorted by start.
    pub fn ranges(&self) -> &[ByteRange] {
        &self.ranges
    }

    /// Add a byte range. Automatically merges overlapping or adjacent ranges.
    pub fn add_range(&mut self, start: u64, length: u64) {
        if length == 0 {
            return;
        }

        let new_range = ByteRange::new(start, length);

        // Find the insertion position and merge with overlapping/adjacent ranges.
        let mut merged_start = new_range.start;
        let mut merged_end = new_range.end();
        let mut remove_from = None;
        let mut remove_count = 0usize;
        let mut insert_at = self.ranges.len();

        for (i, existing) in self.ranges.iter().enumerate() {
            if new_range.is_adjacent_or_overlapping(existing) {
                merged_start = merged_start.min(existing.start);
                merged_end = merged_end.max(existing.end());
                if remove_from.is_none() {
                    remove_from = Some(i);
                    insert_at = i;
                }
                remove_count += 1;
            } else if existing.start > new_range.end() {
                // Past the new range — no more merging possible.
                if remove_from.is_none() {
                    insert_at = i;
                }
                break;
            }
        }

        if let Some(from) = remove_from {
            self.ranges.drain(from..from + remove_count);
        }

        self.ranges.insert(
            insert_at,
            ByteRange::new(merged_start, merged_end - merged_start),
        );
    }

    /// Check whether a range [start, start+length) is fully contained.
    pub fn contains(&self, start: u64, length: u64) -> bool {
        if length == 0 {
            return true;
        }
        let end = start + length;
        for range in &self.ranges {
            if range.start <= start && range.end() >= end {
                return true;
            }
            if range.start > start {
                break;
            }
        }
        false
    }

    /// How many contiguous bytes starting from `start` (up to `max_length`)
    /// are already covered by this range set.
    pub fn contained_length_from(&self, start: u64, max_length: u64) -> u64 {
        if max_length == 0 {
            return 0;
        }
        let end = start + max_length;

        for range in &self.ranges {
            if range.end() <= start {
                continue;
            }
            if range.start >= end {
                break;
            }
            if range.start > start {
                return 0;
            }
            return range.end().min(end) - start;
        }

        0
    }

    /// Whether the range set covers the entire file [0, file_size).
    pub fn covers_full(&self, file_size: u64) -> bool {
        if file_size == 0 {
            return true;
        }
        self.ranges.len() == 1 && self.ranges[0].start == 0 && self.ranges[0].length >= file_size
    }

    /// Total bytes covered across all ranges.
    pub fn total_bytes(&self) -> u64 {
        self.ranges.iter().map(|r| r.length).sum()
    }
}

impl Default for RangeSet {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_range_set() {
        let rs = RangeSet::new();
        assert!(rs.is_empty());
        assert_eq!(rs.len(), 0);
        assert!(!rs.contains(0, 1));
        assert_eq!(rs.contained_length_from(0, 100), 0);
        assert!(!rs.covers_full(100));
        assert_eq!(rs.total_bytes(), 0);
    }

    #[test]
    fn add_single_range() {
        let mut rs = RangeSet::new();
        rs.add_range(0, 100);
        assert_eq!(rs.len(), 1);
        assert!(rs.contains(0, 100));
        assert!(!rs.contains(0, 101));
        assert!(rs.contains(50, 10));
        assert_eq!(rs.contained_length_from(0, 200), 100);
        assert_eq!(rs.total_bytes(), 100);
    }

    #[test]
    fn add_non_overlapping_ranges() {
        let mut rs = RangeSet::new();
        rs.add_range(0, 50);
        rs.add_range(100, 50);
        assert_eq!(rs.len(), 2);
        assert!(rs.contains(0, 50));
        assert!(rs.contains(100, 50));
        assert!(!rs.contains(0, 101));
        assert!(!rs.contains(40, 20));
        assert_eq!(rs.total_bytes(), 100);
    }

    #[test]
    fn merge_overlapping_ranges() {
        let mut rs = RangeSet::new();
        rs.add_range(0, 100);
        rs.add_range(50, 100); // overlaps [50, 150)
        assert_eq!(rs.len(), 1);
        assert_eq!(rs.ranges()[0].start, 0);
        assert_eq!(rs.ranges()[0].length, 150);
        assert!(rs.contains(0, 150));
        assert_eq!(rs.total_bytes(), 150);
    }

    #[test]
    fn merge_adjacent_ranges() {
        let mut rs = RangeSet::new();
        rs.add_range(0, 50);
        rs.add_range(50, 50); // adjacent
        assert_eq!(rs.len(), 1);
        assert_eq!(rs.ranges()[0].start, 0);
        assert_eq!(rs.ranges()[0].length, 100);
        assert!(rs.contains(0, 100));
    }

    #[test]
    fn merge_range_enclosed_by_existing() {
        let mut rs = RangeSet::new();
        rs.add_range(0, 200);
        rs.add_range(50, 50); // fully inside
        assert_eq!(rs.len(), 1);
        assert_eq!(rs.ranges()[0].length, 200);
    }

    #[test]
    fn merge_range_enclosing_existing() {
        let mut rs = RangeSet::new();
        rs.add_range(50, 50);
        rs.add_range(0, 200); // encloses the first
        assert_eq!(rs.len(), 1);
        assert_eq!(rs.ranges()[0].start, 0);
        assert_eq!(rs.ranges()[0].length, 200);
    }

    #[test]
    fn merge_three_ranges_into_one() {
        let mut rs = RangeSet::new();
        rs.add_range(0, 50);
        rs.add_range(100, 50);
        rs.add_range(40, 70); // bridges the gap
        assert_eq!(rs.len(), 1);
        assert_eq!(rs.ranges()[0].start, 0);
        assert_eq!(rs.ranges()[0].length, 150);
    }

    #[test]
    fn zero_length_range_is_ignored() {
        let mut rs = RangeSet::new();
        rs.add_range(50, 0);
        assert!(rs.is_empty());
    }

    #[test]
    fn contained_length_from_with_gap() {
        let mut rs = RangeSet::new();
        rs.add_range(0, 30);
        rs.add_range(50, 30);
        // Starting from 0, max 100: only [0,30) is contiguous.
        assert_eq!(rs.contained_length_from(0, 100), 30);
        // Starting from 20, max 100: only [20,30) is contiguous.
        assert_eq!(rs.contained_length_from(20, 100), 10);
        // Starting from 60, max 10: 10 bytes from [50,80)
        assert_eq!(rs.contained_length_from(60, 10), 10);
        // Window entirely in the gap: 0
        assert_eq!(rs.contained_length_from(35, 10), 0);
    }

    #[test]
    fn covers_full_file() {
        let mut rs = RangeSet::new();
        assert!(rs.covers_full(0));
        assert!(!rs.covers_full(100));

        rs.add_range(0, 100);
        assert!(rs.covers_full(100));
        assert!(!rs.covers_full(101));
        assert!(rs.covers_full(50)); // covers more than needed
    }

    #[test]
    fn add_range_extends_end() {
        let mut rs = RangeSet::new();
        rs.add_range(0, 50);
        rs.add_range(30, 50); // extends beyond [0, 50) → [0, 80)
        assert_eq!(rs.len(), 1);
        assert_eq!(rs.ranges()[0].end(), 80);
    }

    #[test]
    fn add_range_extends_start() {
        let mut rs = RangeSet::new();
        rs.add_range(50, 50); // [50, 100)
        rs.add_range(0, 60); // [0, 60) overlaps → [0, 100)
        assert_eq!(rs.len(), 1);
        assert_eq!(rs.ranges()[0].start, 0);
        assert_eq!(rs.ranges()[0].end(), 100);
    }

    #[test]
    fn serialization_roundtrip() {
        let mut rs = RangeSet::new();
        rs.add_range(0, 100);
        rs.add_range(200, 50);

        let json = serde_json::to_string(&rs).unwrap();
        let deserialized: RangeSet = serde_json::from_str(&json).unwrap();
        assert_eq!(rs, deserialized);
    }
}
