use anyhow::{bail, Context, Result};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LyricLine {
    pub time_ms: u64,
    pub text: String,
}

pub fn parse_lrc(lrc: &str) -> Result<Vec<LyricLine>> {
    let mut parsed_lines = Vec::new();

    for raw_line in lrc.lines() {
        let mut cursor = raw_line;
        let mut timestamps = Vec::new();

        while let Some(stripped) = cursor.strip_prefix('[') {
            let closing = stripped
                .find(']')
                .with_context(|| format!("missing closing ] in LRC line: {raw_line}"))?;
            let tag = &stripped[..closing];

            if let Some(timestamp_ms) = parse_timestamp_tag(tag)? {
                timestamps.push(timestamp_ms);
                cursor = &stripped[closing + 1..];
                continue;
            }

            timestamps.clear();
            break;
        }

        if timestamps.is_empty() {
            continue;
        }

        let lyric_text = cursor.trim().to_owned();
        for timestamp_ms in timestamps {
            parsed_lines.push(LyricLine {
                time_ms: timestamp_ms,
                text: lyric_text.clone(),
            });
        }
    }

    parsed_lines.sort_by_key(|line| line.time_ms);
    Ok(parsed_lines)
}

fn parse_timestamp_tag(tag: &str) -> Result<Option<u64>> {
    let Some((minutes, remainder)) = tag.split_once(':') else {
        return Ok(None);
    };
    let Some((seconds, fractional)) = remainder.split_once('.') else {
        return Ok(None);
    };

    let minutes: u64 = minutes
        .parse()
        .with_context(|| format!("invalid LRC minutes value: {tag}"))?;
    let seconds: u64 = seconds
        .parse()
        .with_context(|| format!("invalid LRC seconds value: {tag}"))?;
    if seconds >= 60 {
        bail!("invalid LRC seconds value {seconds} in tag {tag}");
    }

    let fraction_ms = match fractional.len() {
        1 => {
            fractional
                .parse::<u64>()
                .with_context(|| format!("invalid LRC fractional value: {tag}"))?
                * 100
        }
        2 => {
            fractional
                .parse::<u64>()
                .with_context(|| format!("invalid LRC fractional value: {tag}"))?
                * 10
        }
        3 => fractional
            .parse::<u64>()
            .with_context(|| format!("invalid LRC fractional value: {tag}"))?,
        _ => bail!("unsupported LRC fractional precision in tag {tag}"),
    };

    Ok(Some(minutes * 60_000 + seconds * 1_000 + fraction_ms))
}
