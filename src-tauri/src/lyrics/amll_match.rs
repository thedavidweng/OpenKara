use regex_lite::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;
use unicode_normalization::UnicodeNormalization;

static FEAT_PARENS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\((?:feat\.?|ft\.?|featuring|with)\b[^)]*\)").unwrap());
static FEAT_BRACKETS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[(?:feat\.?|ft\.?|featuring|with)\b[^\]]*\]").unwrap());
static FEAT_TRAILING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s+(?:feat\.?|ft\.?|featuring)\b.*$").unwrap());
static VERSION_NOISE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?x)
        [\(\[]\s*
        (?:
            official(?:\s+(?:music\s+)?video)?
          | lyric\s+video
          | audio
          | mv
          | karaoke
          | instrumental
          | off[\s-]?vocal
          | live
          | radio\s+edit
          | remaster(?:ed)?(?:\s+\d{4})?
          | deluxe
          | explicit
          | clean
          | bonus(?:\s+track)?
          | version
          | edit
          | mono
          | stereo
          | re-?record(?:ed)?
        )
        \s*[\)\]]",
    )
    .unwrap()
});
static ARTIST_SPLIT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\s*[,/&;+]\s*|\s+[x×]\s+|\s+(?:feat\.?|ft\.?|featuring|with)\s+").unwrap()
});

#[derive(Debug, Clone, Copy)]
pub struct AmllMatchCandidate<'a> {
    pub music_names: &'a [String],
    pub artist_names: &'a [String],
}

pub fn normalize_name(input: &str) -> String {
    let nfkc: String = input.nfkc().collect();
    strip_feat_and_version(&nfkc.to_lowercase())
}

pub fn split_artists(input: &str) -> Vec<String> {
    let nfkc: String = input.nfkc().collect();
    let lower = nfkc.to_lowercase();
    ARTIST_SPLIT
        .split(&lower)
        .map(strip_feat_and_version)
        .filter(|token| !token.is_empty())
        .collect()
}

pub fn title_similar(song_title: &str, candidate_title: &str) -> bool {
    let song = normalize_name(song_title);
    let candidate = normalize_name(candidate_title);
    if song.is_empty() || candidate.is_empty() {
        return false;
    }
    if song == candidate {
        return true;
    }
    let (shorter, longer) = if song.len() <= candidate.len() {
        (&song, &candidate)
    } else {
        (&candidate, &song)
    };
    if longer.contains(shorter.as_str()) && (shorter.len() as f64 / longer.len() as f64) >= 0.8 {
        return true;
    }
    token_jaccard(&song, &candidate) >= 0.8
}

pub fn artists_overlap(song_artist: &str, candidate_artists: &[String]) -> bool {
    let song_tokens = split_artists(song_artist);
    let candidate_tokens: Vec<String> = candidate_artists
        .iter()
        .flat_map(|name| split_artists(name))
        .collect();
    song_tokens.iter().any(|song_token| {
        candidate_tokens.iter().any(|candidate_token| {
            if song_token == candidate_token {
                return true;
            }
            let (contained, container) = if song_token.len() <= candidate_token.len() {
                (song_token, candidate_token)
            } else {
                (candidate_token, song_token)
            };
            contained.len() >= 2 && container.contains(contained.as_str())
        })
    })
}

pub fn is_exact_title_artist(
    track_name: &str,
    artist_name: &str,
    music_names: &[String],
    artist_names: &[String],
) -> bool {
    let title = normalize_name(track_name);
    if title.is_empty() {
        return false;
    }
    let title_exact = music_names
        .iter()
        .any(|candidate| normalize_name(candidate) == title);
    if !title_exact {
        return false;
    }
    let song_tokens: HashSet<String> = split_artists(artist_name).into_iter().collect();
    let candidate_tokens: HashSet<String> = artist_names
        .iter()
        .flat_map(|name| split_artists(name))
        .collect();
    song_tokens == candidate_tokens
}

pub fn select_confident_index(
    track_name: &str,
    artist_name: &str,
    items: &[AmllMatchCandidate<'_>],
) -> Option<usize> {
    let filtered: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            item.music_names
                .iter()
                .any(|name| title_similar(track_name, name))
                && artists_overlap(artist_name, item.artist_names)
        })
        .map(|(index, _)| index)
        .collect();

    if filtered.len() == 1 {
        return Some(filtered[0]);
    }
    if filtered.is_empty() {
        return None;
    }

    let exact: Vec<usize> = filtered
        .iter()
        .copied()
        .filter(|&index| {
            is_exact_title_artist(
                track_name,
                artist_name,
                items[index].music_names,
                items[index].artist_names,
            )
        })
        .collect();
    if exact.len() == 1 {
        Some(exact[0])
    } else {
        None
    }
}

fn strip_feat_and_version(input: &str) -> String {
    let without_parens = FEAT_PARENS.replace_all(input, "");
    let without_brackets = FEAT_BRACKETS.replace_all(&without_parens, "");
    let without_trailing = FEAT_TRAILING.replace_all(&without_brackets, "");
    let without_version = VERSION_NOISE.replace_all(&without_trailing, "");
    collapse_ascii_whitespace(&without_version)
}

fn collapse_ascii_whitespace(input: &str) -> String {
    input.split_ascii_whitespace().collect::<Vec<_>>().join(" ")
}

fn token_jaccard(left: &str, right: &str) -> f64 {
    let left_tokens: HashSet<&str> = left.split_ascii_whitespace().collect();
    let right_tokens: HashSet<&str> = right.split_ascii_whitespace().collect();
    if left_tokens.is_empty() || right_tokens.is_empty() {
        return 0.0;
    }
    let intersection = left_tokens.intersection(&right_tokens).count();
    let union = left_tokens.union(&right_tokens).count();
    intersection as f64 / union as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn candidate<'a>(music: &'a [String], artists: &'a [String]) -> AmllMatchCandidate<'a> {
        AmllMatchCandidate {
            music_names: music,
            artist_names: artists,
        }
    }

    #[test]
    fn exact_unique_title_and_artist() {
        let music = names(&["Yellow"]);
        let artists = names(&["Coldplay"]);
        let items = [candidate(&music, &artists)];
        assert_eq!(
            select_confident_index("Yellow", "Coldplay", &items),
            Some(0)
        );
    }

    #[test]
    fn unique_after_filter_when_second_fails_artist_overlap() {
        let yellow = names(&["Yellow"]);
        let coldplay = names(&["Coldplay"]);
        let other = names(&["Someone Else"]);
        let items = [candidate(&yellow, &coldplay), candidate(&yellow, &other)];
        assert_eq!(
            select_confident_index("Yellow", "Coldplay", &items),
            Some(0)
        );
    }

    #[test]
    fn two_similar_titles_same_artist_are_ambiguous() {
        let yellows = names(&["Yellows"]);
        let yellow_bang = names(&["Yellow!"]);
        let coldplay = names(&["Coldplay"]);
        let items = [
            candidate(&yellows, &coldplay),
            candidate(&yellow_bang, &coldplay),
        ];
        assert_eq!(select_confident_index("Yellow", "Coldplay", &items), None);
    }

    #[test]
    fn one_exact_and_one_similar_is_confident_exact() {
        let exact = names(&["Yellow"]);
        let similar = names(&["Yellows"]);
        let coldplay = names(&["Coldplay"]);
        let items = [candidate(&similar, &coldplay), candidate(&exact, &coldplay)];
        assert_eq!(
            select_confident_index("Yellow", "Coldplay", &items),
            Some(1)
        );
    }

    #[test]
    fn feat_ft_and_fullwidth_parentheticals_normalize() {
        assert_eq!(
            normalize_name("ME! (feat. Brendon Urie of Panic! At The Disco)"),
            normalize_name("ME!")
        );
        assert_eq!(
            normalize_name("ME! (ft. Brendon Urie)"),
            normalize_name("ME!")
        );
        assert_eq!(
            normalize_name("ME!（feat. Brendon Urie）"),
            normalize_name("ME!")
        );
        assert_eq!(
            normalize_name("Song [featuring Guest]"),
            normalize_name("Song")
        );
    }

    #[test]
    fn feat_clause_splits_into_two_artist_tokens() {
        assert_eq!(
            split_artists("Taylor Swift feat. Brendon Urie"),
            vec!["taylor swift".to_owned(), "brendon urie".to_owned()]
        );
    }

    #[test]
    fn nfkc_halfwidth_katakana_and_fullwidth_letters() {
        assert_eq!(normalize_name("ｶﾞ"), normalize_name("ガ"));
        assert_eq!(normalize_name("ＹＥＬＬＯＷ"), normalize_name("Yellow"));
    }

    #[test]
    fn slash_separated_artists_overlap_second_token() {
        let candidate_artists = names(&["Artist A / Artist B"]);
        assert!(artists_overlap("Artist B", &candidate_artists));
    }

    #[test]
    fn empty_title_after_normalize_never_matches() {
        assert!(!title_similar("(Official Video)", "Yellow"));
        assert!(!title_similar("", "Yellow"));
        let empty = names(&["(Official Video)"]);
        let artists = names(&["Coldplay"]);
        let items = [candidate(&empty, &artists)];
        assert_eq!(
            select_confident_index("(Official Video)", "Coldplay", &items),
            None
        );
    }

    #[test]
    fn empty_jaccard_token_set_does_not_match() {
        assert_eq!(token_jaccard("", "hello"), 0.0);
        assert_eq!(token_jaccard("hello", ""), 0.0);
        assert_eq!(token_jaccard("", ""), 0.0);
    }

    #[test]
    fn version_noise_is_stripped_and_piano_is_kept() {
        assert_eq!(
            normalize_name("Song (Official Video)"),
            normalize_name("Song")
        );
        assert_eq!(
            normalize_name("Song (Remastered 2011)"),
            normalize_name("Song")
        );
        assert_eq!(normalize_name("Song (re-recorded)"), normalize_name("Song"));
        assert_eq!(normalize_name("Song (Piano)"), "song (piano)");
        assert_ne!(normalize_name("Song (Piano)"), normalize_name("Song"));
    }

    #[test]
    fn short_artist_token_overlaps_by_containment() {
        let alan = names(&["Alan"]);
        assert!(artists_overlap("Al", &alan));
    }
}
