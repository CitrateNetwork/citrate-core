// created: 2026-10-18 | branch: beta/k1a-gate0-harness | author: Larry Klosowski + Claude Opus 5.5 (MAC lane)
// status: active (SCL-S1.3 fault-injection harness)
//
// The lifecycle fault-injection harness (SCL-S1.3), shared by the `lifecycle_faults` suite.
// See `fixture.rs` (owned processes, marker-checked guards) and `timing.rs` (monotonic waits).

pub mod fixture;
pub mod timing;
