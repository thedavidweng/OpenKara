# src-tauri/tests

Rust integration tests and fixture assets that validate backend behavior
against real audio inputs. The fixture files are intentionally small so they
can stay in the repository and keep metadata/import tests reproducible for
future handoffs.

All test files compile into one test binary, `integration`. The root module
`integration.rs` declares each file as a module, and Cargo test
auto-discovery is off. Add each new test file to `integration.rs`. A test
name starts with its file name, for example
`phase5_perf::backend_performance_report_stays_within_phase5_thresholds`.
Use `support` for shared helpers. A test that changes process-wide state
holds `support::process_state_lock()`. A test that uses ONNX Runtime holds
`support::acquire_test_runtime()`.
