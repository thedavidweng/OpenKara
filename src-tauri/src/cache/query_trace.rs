use rusqlite::{
    trace::{TraceEvent, TraceEventCodes},
    Connection,
};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueryTraceReport {
    pub statements: Vec<String>,
    pub total_duration: Duration,
}

impl QueryTraceReport {
    pub fn statement_count(&self) -> usize {
        self.statements.len()
    }
}

fn is_transaction_control_sql(sql: &str) -> bool {
    let trimmed = sql.trim_start();
    let upper_prefix: String = trimmed
        .chars()
        .take(10)
        .map(|ch| ch.to_ascii_uppercase())
        .collect();
    upper_prefix.starts_with("BEGIN")
        || upper_prefix.starts_with("COMMIT")
        || upper_prefix.starts_with("ROLLBACK")
        || upper_prefix.starts_with("SAVEPOINT")
        || upper_prefix.starts_with("RELEASE")
}

/// Trace top-level SQL statements executed on `connection` while running `operation`.
/// Excludes SQLite trigger subprograms (`-- ...`) and transaction/savepoint
/// bookkeeping so the count reflects application query round-trips.
pub fn trace_queries<T, F>(
    connection: &mut Connection,
    operation: F,
) -> rusqlite::Result<(T, QueryTraceReport)>
where
    F: FnOnce(&mut Connection) -> T,
{
    let captured = Arc::new(Mutex::new(QueryTraceReport::default()));
    let sink = Arc::clone(&captured);

    connection.trace_v2(
        TraceEventCodes::SQLITE_TRACE_STMT | TraceEventCodes::SQLITE_TRACE_PROFILE,
        Some(move |event: TraceEvent<'_>| match event {
            TraceEvent::Stmt(stmt, raw_sql) => {
                if raw_sql.starts_with("--") {
                    return;
                }
                let template = stmt.sql();
                let sql = if template.is_empty() {
                    raw_sql
                } else {
                    template.as_ref()
                };
                if is_transaction_control_sql(sql) {
                    return;
                }
                if let Ok(mut report) = sink.lock() {
                    report.statements.push(sql.trim().to_owned());
                }
            }
            TraceEvent::Profile(_stmt, elapsed) => {
                if let Ok(mut report) = sink.lock() {
                    report.total_duration += elapsed;
                }
            }
            _ => {}
        }),
    )?;

    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| operation(connection)));
    let _ = connection.trace_v2(TraceEventCodes::empty(), None::<fn(TraceEvent<'_>)>);

    let value = match outcome {
        Ok(value) => value,
        Err(payload) => std::panic::resume_unwind(payload),
    };

    let report = captured
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_default();

    Ok((value, report))
}

/// Returns a diagnostic message when SQL statement count grows with input size `N`.
pub fn detect_n_plus_one(
    operation: &str,
    small_n: usize,
    small_report: &QueryTraceReport,
    large_n: usize,
    large_report: &QueryTraceReport,
) -> Option<String> {
    if large_n > small_n && large_report.statement_count() > small_report.statement_count() {
        let preview = large_report
            .statements
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join(" | ");
        Some(format!(
            "N+1 query growth in {operation}: N={small_n} executed {} SQL statement(s), N={large_n} executed {} SQL statement(s). Sample: {preview}",
            small_report.statement_count(),
            large_report.statement_count(),
        ))
    } else {
        None
    }
}
