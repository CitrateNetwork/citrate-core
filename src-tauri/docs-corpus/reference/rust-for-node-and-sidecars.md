# Rust for the Node and Sidecars

Reference knowledge for the Rust that powers citrate-core, its sidecars, and the
federation crates. The desktop app is a Tauri v2 application: a Rust core exposing
commands to a web UI.

## Ownership, borrowing, and lifetimes

Rust guarantees memory safety without a garbage collector through ownership: each value
has a single owner, and it is dropped when the owner goes out of scope. Borrows are
either one mutable reference or any number of shared references, never both at once,
enforced at compile time. Lifetimes are the compiler's way of proving a reference never
outlives the data it points at. This model eliminates data races and use-after-free at
compile time.

## Error handling

Fallible functions return `Result<T, E>`; the `?` operator propagates an error early.
Prefer typed error enums over stringly-typed errors at module boundaries, and convert to
a string only at the UI seam. `Option<T>` models absence. Panics are for unrecoverable
bugs, not for expected failures — a command that can fail returns a `Result` the UI can
render honestly.

## Async and blocking work

Async functions run on an executor; `.await` yields until a future is ready. In Tauri
v2 a synchronous `#[tauri::command]` runs on the main thread, so any blocking work
(network I/O, disk, a subprocess) freezes the UI. The fix is to make the command
`async` and move blocking work onto a worker with `tauri::async_runtime::spawn_blocking`,
then `.await` its result. This is the single most important pattern for a responsive
desktop node: never block the main thread.

## Traits and generics

Traits define shared behavior; generics let a function work over any type that
implements a trait bound. This is how the node abstracts over transports: a client is
generic over a transport trait, so production uses a real socket and tests inject a
stub. Prefer dependency injection through a trait over hard-coding a concrete type, so
the logic is testable headless.

## Testing

`cargo test` runs unit tests (`#[test]`) and integration tests. Keep pure logic
decoupled from I/O so it can be tested without a network or a daemon: pass a closure or
a trait object for the side effect and assert on what it received. A test suite whose
count only ever grows is a discipline that catches silent regressions.

## Serialization

`serde` derives serialization. `#[serde(rename_all = "camelCase")]` bridges Rust
snake_case fields to the camelCase a web UI expects, and `#[serde(rename = "...")]`
maps a single field. A Rust struct that mirrors a TypeScript interface field-for-field
is how the Tauri command boundary stays type-safe across the language seam.

## Secrets hygiene

Never log or return a key, seed, or entropy from a command. Wrap secret bytes in a type
that zeroizes on drop, keep signing behind a single gated path, and let the command
surface return only public status, decoded intent, or a signature — never the material
that produced it.
