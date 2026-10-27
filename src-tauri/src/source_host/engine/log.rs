use serde_json::Value;
use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

const REDACTED: &str = "[redacted]";
const MAX_MESSAGE_CHARS: usize = 1024;
const MAX_FIELDS_CHARS: usize = 4096;
const MAX_SECRETS: usize = 1024;
const MIN_SECRET_CHARS: usize = 4;
const RECORDS_PER_WINDOW: u32 = 100;
const WINDOW: Duration = Duration::from_secs(60);
const SENSITIVE_FIELD_PARTS: &[&str] = &[
    "authorization",
    "cookie",
    "credential",
    "csrf",
    "key",
    "pass",
    "secret",
    "session",
    "sign",
    "token",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

/// Structured source logging. Every record is redacted before it reaches the
/// host log: credential values and cookies the source has seen, values under
/// sensitive field names, and URL queries and fragments, which often carry
/// signed tokens.
pub(super) struct SourceLog {
    source_id: String,
    dispatch: tracing::Dispatch,
    secrets: BTreeSet<String>,
    window_start: Instant,
    records: u32,
}
impl SourceLog {
    pub(super) fn new(source_id: String, dispatch: tracing::Dispatch) -> Self {
        Self {
            source_id,
            dispatch,
            secrets: BTreeSet::new(),
            window_start: Instant::now(),
            records: 0,
        }
    }
    pub(super) fn remember(&mut self, secret: &str) {
        if secret.chars().count() >= MIN_SECRET_CHARS && self.secrets.len() < MAX_SECRETS {
            self.secrets.insert(secret.to_owned());
        }
    }
    pub(super) fn remember_credentials(&mut self, credentials: &Value) {
        if let Some(credentials) = credentials.as_object() {
            for value in credentials.values().filter_map(Value::as_str) {
                self.remember(value);
            }
        }
    }
    pub(super) fn record(&mut self, level: Level, message: &str, fields: Option<Value>) {
        if self.window_start.elapsed() >= WINDOW {
            self.window_start = Instant::now();
            self.records = 0;
        }
        self.records += 1;
        if self.records > RECORDS_PER_WINDOW + 1 {
            return;
        }
        let (level, message, fields) = if self.records > RECORDS_PER_WINDOW {
            (
                Level::Warn,
                "Source log rate limit reached; later records in this minute are dropped."
                    .to_owned(),
                String::new(),
            )
        } else {
            let fields = fields
                .map(|fields| truncate(&self.redact_value(fields).to_string(), MAX_FIELDS_CHARS))
                .unwrap_or_default();
            (
                level,
                truncate(&self.redact(message), MAX_MESSAGE_CHARS),
                fields,
            )
        };
        let source = self.source_id.as_str();
        macro_rules! emit {
            ($macro:ident) => {
                tracing::$macro!(target: "openkara::streaming_source", source, fields = %fields, "{message}")
            };
        }
        tracing::dispatcher::with_default(&self.dispatch, || match level {
            Level::Debug => emit!(debug),
            Level::Info => emit!(info),
            Level::Warn => emit!(warn),
            Level::Error => emit!(error),
        });
    }
    pub(super) fn redact(&self, text: &str) -> String {
        let mut secrets: Vec<&String> = self.secrets.iter().collect();
        secrets.sort_by_key(|secret| std::cmp::Reverse(secret.len()));
        let mut text = text.to_owned();
        for secret in secrets {
            text = text.replace(secret.as_str(), REDACTED);
        }
        strip_url_queries(&text)
    }
    fn redact_value(&self, value: Value) -> Value {
        match value {
            Value::String(text) => Value::String(self.redact(&text)),
            Value::Array(items) => Value::Array(
                items
                    .into_iter()
                    .map(|item| self.redact_value(item))
                    .collect(),
            ),
            Value::Object(entries) => Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| {
                        let lower = key.to_ascii_lowercase();
                        let value = if SENSITIVE_FIELD_PARTS
                            .iter()
                            .any(|part| lower.contains(part))
                        {
                            Value::String(REDACTED.into())
                        } else {
                            self.redact_value(value)
                        };
                        (key, value)
                    })
                    .collect(),
            ),
            other => other,
        }
    }
}

fn strip_url_queries(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = ["https://", "http://"]
        .iter()
        .filter_map(|scheme| rest.find(scheme))
        .min()
    {
        let end = rest[start..]
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | '`'))
            .map_or(rest.len(), |end| start + end);
        let url = &rest[start..end];
        output.push_str(&rest[..start]);
        match url.find(['?', '#']) {
            Some(cut) => {
                output.push_str(&url[..cut]);
                output.push_str("?[redacted]");
            }
            None => output.push_str(url),
        }
        rest = &rest[end..];
    }
    output.push_str(rest);
    output
}

fn truncate(text: &str, limit: usize) -> String {
    match text.char_indices().nth(limit) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn log() -> SourceLog {
        SourceLog::new("fixture".into(), tracing::Dispatch::none())
    }

    #[test]
    fn credentials_cookies_and_signed_urls_are_redacted() {
        let mut log = log();
        log.remember_credentials(&json!({"MUSIC_U":"secret-session-value","short":"ab"}));
        log.remember("cookie-from-response");
        assert_eq!(
            log.redact("restored secret-session-value with cookie-from-response and ab"),
            "restored [redacted] with [redacted] and ab"
        );
        assert_eq!(
            log.redact("fetch https://cdn.example.test/a.mp3?sig=abc&exp=1 done"),
            "fetch https://cdn.example.test/a.mp3?[redacted] done"
        );
        assert_eq!(
            log.redact("see \"https://api.example.test/v1#token\""),
            "see \"https://api.example.test/v1?[redacted]\""
        );
        let fields = log.redact_value(json!({
            "playlist": "42",
            "Cookie": "a=b",
            "nested": {"access_token": "t", "note": "secret-session-value"},
            "list": ["https://api.example.test/x?y=z"],
        }));
        assert_eq!(
            fields,
            json!({
                "playlist": "42",
                "Cookie": "[redacted]",
                "nested": {"access_token": "[redacted]", "note": "[redacted]"},
                "list": ["https://api.example.test/x?[redacted]"],
            })
        );
    }

    #[test]
    fn long_records_are_truncated_on_character_boundaries() {
        assert_eq!(truncate("añb", 2), "añ…");
        assert_eq!(truncate("ab", 2), "ab");
    }
}
