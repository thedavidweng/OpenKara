//! Application re-export of `openkara_remote::net_policy`.

#[allow(unused_imports)]
pub(crate) use openkara_remote::net_policy::{
    build_http_client, classify_reqwest_error, classify_status, full_jitter_delay,
    parse_retry_after, production_sleep, remote_error_with_retry_after, run_with_default_retry,
    shared_http_client, stream_response_body, stream_response_body_capped, AttemptOutcome,
    JitterRng, NoopProgress, RetryDriver, RetryPolicy, RetryProgress, SeededJitter, ThreadJitter,
};
