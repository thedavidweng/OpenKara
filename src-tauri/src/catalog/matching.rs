//! Ranks tracks from other Streaming Sources as Playable Match candidates.

use super::types::StreamingTrack;
use crate::lyrics::amll_match::{artists_overlap, is_exact_title_artist, title_similar};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use unicode_normalization::UnicodeNormalization;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayableMatchTarget {
    pub source_id: String,
    pub remote_track_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayableMatches {
    pub remembered: Option<PlayableMatchTarget>,
    pub candidates: Vec<StreamingTrack>,
}

const DURATION_TOLERANCE_MS: u64 = 3000;
pub const MAX_CANDIDATES: usize = 10;

const ASCII_MARKS: [(&str, &str); 6] = [
    ("live", "live"),
    ("remix", "remix"),
    ("instrumental", "instrumental"),
    ("inst", "instrumental"),
    ("karaoke", "karaoke"),
    ("acoustic", "acoustic"),
];
const CJK_MARKS: [(&str, &str); 7] = [
    ("伴奏", "instrumental"),
    ("现场", "live"),
    ("現場", "live"),
    ("純音樂", "instrumental"),
    ("纯音乐", "instrumental"),
    ("ライブ", "live"),
    ("カラオケ", "karaoke"),
];

/// Version marks in a raw title, such as live or instrumental.
fn version_marks(title: &str) -> BTreeSet<&'static str> {
    let lower: String = title.nfkc().collect::<String>().to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    let mut marks = BTreeSet::new();
    for (word, mark) in ASCII_MARKS {
        if words.contains(&word) {
            marks.insert(mark);
        }
    }
    if words.contains(&"offvocal") || words.windows(2).any(|pair| pair == ["off", "vocal"]) {
        marks.insert("instrumental");
    }
    for (text, mark) in CJK_MARKS {
        if lower.contains(text) {
            marks.insert(mark);
        }
    }
    marks
}

/// Distance in milliseconds, or `None` when either duration is unknown.
fn duration_delta(original: &StreamingTrack, candidate: &StreamingTrack) -> Option<u64> {
    Some(original.duration_ms?.abs_diff(candidate.duration_ms?))
}

fn tier(original: &StreamingTrack, candidate: &StreamingTrack) -> Option<u8> {
    if original.isrc.is_some() && original.isrc == candidate.isrc {
        return Some(0);
    }
    if duration_delta(original, candidate).is_some_and(|delta| delta > DURATION_TOLERANCE_MS) {
        return None;
    }
    let artists = std::slice::from_ref(&candidate.artist);
    if is_exact_title_artist(
        &original.title,
        &original.artist,
        std::slice::from_ref(&candidate.title),
        artists,
    ) {
        Some(1)
    } else if title_similar(&original.title, &candidate.title)
        && artists_overlap(&original.artist, artists)
    {
        Some(2)
    } else {
        None
    }
}

/// Keeps playable candidates from other sources that plausibly match the
/// original, best first, at most [`MAX_CANDIDATES`].
pub fn rank_candidates(
    original: &StreamingTrack,
    candidates: Vec<StreamingTrack>,
) -> Vec<StreamingTrack> {
    let original_marks = version_marks(&original.title);
    let mut ranked: Vec<_> = candidates
        .into_iter()
        .enumerate()
        .filter(|(_, candidate)| {
            candidate.refusal.is_none() && candidate.source_id != original.source_id
        })
        .filter_map(|(index, candidate)| {
            let tier = tier(original, &candidate)?;
            let marks_differ = version_marks(&candidate.title) != original_marks;
            let delta = duration_delta(original, &candidate).unwrap_or(u64::MAX);
            Some(((marks_differ, tier, delta, index), candidate))
        })
        .collect();
    ranked.sort_by_key(|(key, _)| *key);
    ranked
        .into_iter()
        .take(MAX_CANDIDATES)
        .map(|(_, candidate)| candidate)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::types::{ImportRefusal, ImportRefusalReason};

    fn track(source: &str, id: &str, title: &str, duration_ms: Option<u64>) -> StreamingTrack {
        StreamingTrack {
            source_id: source.into(),
            remote_track_id: id.into(),
            title: title.into(),
            artist: "Jay Chou".into(),
            album: None,
            duration_ms,
            refusal: None,
            library_song_id: None,
            cover_url: None,
            isrc: None,
        }
    }

    fn ids(ranked: &[StreamingTrack]) -> Vec<&str> {
        ranked
            .iter()
            .map(|track| track.remote_track_id.as_str())
            .collect()
    }

    #[test]
    fn same_isrc_ranks_first_even_when_titles_and_durations_differ() {
        let mut original = track("a", "o", "Sunny Day", Some(180_000));
        original.isrc = Some("TWA010000001".into());
        let exact = track("b", "exact", "Sunny Day", Some(180_000));
        let mut isrc = track("b", "isrc", "晴天", Some(200_000));
        isrc.isrc = Some("TWA010000001".into());
        let ranked = rank_candidates(&original, vec![exact, isrc]);
        assert_eq!(ids(&ranked), ["isrc", "exact"]);
    }

    #[test]
    fn exact_beats_similar_then_closer_duration_then_input_order() {
        let original = track("a", "o", "Sunny Day", Some(180_000));
        let similar = track("b", "similar", "Sunny Days", Some(180_000));
        let far = track("b", "far", "Sunny Day", Some(182_000));
        let near = track("b", "near", "Sunny Day", Some(180_500));
        let tied = track("b", "tied", "Sunny Day", Some(180_500));
        let ranked = rank_candidates(&original, vec![similar, far, near, tied]);
        assert_eq!(ids(&ranked), ["near", "tied", "far", "similar"]);
    }

    #[test]
    fn version_mark_differences_rank_below_all_unmarked_tiers() {
        let original = track("a", "o", "Sunny Day", Some(180_000));
        let live = track("b", "live", "Sunny Day (Live)", Some(180_000));
        let similar = track("b", "similar", "Sunny Days", Some(180_000));
        let ranked = rank_candidates(&original, vec![live, similar]);
        assert_eq!(ids(&ranked), ["similar", "live"]);

        let live_original = track("a", "o", "Sunny Day (Live)", Some(180_000));
        let plain = track("b", "plain", "Sunny Day", Some(180_000));
        let live = track("b", "live", "Sunny Day [Live]", Some(180_000));
        let ranked = rank_candidates(&live_original, vec![plain, live]);
        assert_eq!(ids(&ranked), ["live", "plain"]);
    }

    #[test]
    fn detects_off_vocal_and_cjk_marks_but_not_words_that_contain_them() {
        assert!(version_marks("Song (Off Vocal)").contains("instrumental"));
        assert!(version_marks("歌 伴奏").contains("instrumental"));
        assert!(version_marks("Song (Inst.)").contains("instrumental"));
        assert!(version_marks("Delivery").is_empty());
    }

    #[test]
    fn durations_more_than_three_seconds_apart_are_dropped() {
        let original = track("a", "o", "Sunny Day", Some(180_000));
        let edge = track("b", "edge", "Sunny Day", Some(183_000));
        let off = track("b", "off", "Sunny Day", Some(183_001));
        let unknown = track("b", "unknown", "Sunny Day", None);
        let ranked = rank_candidates(&original, vec![off, unknown, edge]);
        assert_eq!(ids(&ranked), ["edge", "unknown"]);
    }

    #[test]
    fn drops_refused_same_source_and_unrelated_candidates() {
        let original = track("a", "o", "Sunny Day", Some(180_000));
        let mut refused = track("b", "refused", "Sunny Day", Some(180_000));
        refused.refusal = Some(ImportRefusal {
            reason: ImportRefusalReason::EmptyUrl,
            title: "Sunny Day".into(),
            artist: "Jay Chou".into(),
        });
        let same_source = track("a", "same", "Sunny Day", Some(180_000));
        let unrelated = track("b", "unrelated", "Rainy Night", Some(180_000));
        let ok = track("c", "ok", "Sunny Day", Some(180_000));
        let ranked = rank_candidates(&original, vec![refused, same_source, unrelated, ok]);
        assert_eq!(ids(&ranked), ["ok"]);
    }

    #[test]
    fn caps_the_list() {
        let original = track("a", "o", "Sunny Day", Some(180_000));
        let many = (0..15)
            .map(|index| track("b", &index.to_string(), "Sunny Day", Some(180_000)))
            .collect();
        assert_eq!(rank_candidates(&original, many).len(), MAX_CANDIDATES);
    }
}
