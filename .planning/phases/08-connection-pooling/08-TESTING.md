# Phase 8: Connection Pooling — Testing Strategy Research

**Researched:** 2026-03-01
**Domain:** Testing bb8 connection pools in async Rust — mock managers, Send bounds, concurrent checkout, health checks, registry
**Confidence:** HIGH for mock manager patterns (verified against bb8 test.rs source); HIGH for Send analysis (verified against docs.rs trait impls); MEDIUM for concurrent test patterns (derived from bb8 test structure + Tokio docs)

---

## Executive Summary

The single most important finding in this research is that **the existing `tokio_test::io::Mock` transport is `Send + Sync`** — it implements both auto traits as of tokio-test 0.4.x (the Send/Sync fix landed in commit `6919f7c`, changing the internal `rx` field from a boxed trait object to `UnboundedReceiverStream`). This means the existing `Transport` struct — which wraps `BufReader<ReadHalf<InnerStream>>` and `WriteHalf<InnerStream>` — is also `Send` when `InnerStream::Mock` is used, because `ReadHalf<T>: Send` when `T: Send`, and `BufReader<T>: Send` when `T: Send`.

**Conclusion for Question 5:** The mock transport does NOT need to be rewritten with `Arc<Mutex<Vec<u8>>>`. Pool tests can use `tokio_test::io::Mock` directly inside `Pop3Client`, and the resulting `Pop3Client` type satisfies bb8's `Connection: Send + 'static` requirement.

The recommended testing architecture uses **two-tier mocking**:

1. **Manager-level mocks** (for pool behavior tests): Bypass `Pop3Client` entirely. Implement a `MockPop3Manager` that returns cheaply-constructed fake connections. Tests for pool checkout/return/exhaustion/timeout live here.
2. **Client-level mocks** (for health check tests): Use the existing `tokio_test::io::Builder` infrastructure to construct `Pop3Client` instances with scripted I/O. The `Pop3ConnectionManager` can be tested by injecting a factory function.

---

## Part 1: Mocking bb8 Pools Without Real TCP

### The Core Pattern: Manager-Level Mock

bb8's own test suite (from `bb8/tests/test.rs`) defines minimal connection types and managers that have zero real I/O. These are the canonical patterns to copy:

```rust
// Pattern from bb8/tests/test.rs (verified against GitHub source)
// FakeConnection is the connection type — zero-sized, no real I/O
#[derive(Debug, Default)]
struct FakeConnection;

// Error type
#[derive(Debug)]
struct TestError;

// OkManager: always succeeds, never broken — for happy-path pool tests
struct OkManager;

impl bb8::ManageConnection for OkManager {
    type Connection = FakeConnection;
    type Error = TestError;

    fn connect(&self) -> impl Future<Output = Result<Self::Connection, Self::Error>> + Send {
        async { Ok(FakeConnection) }
    }

    fn is_valid(
        &self,
        _conn: &mut Self::Connection,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        async { Ok(()) }
    }

    fn has_broken(&self, _conn: &mut Self::Connection) -> bool {
        false
    }
}
```

**Why this works for Phase 8 pool behavior tests:** Pool exhaustion, checkout blocking, timeout, and return-by-drop tests do NOT need real POP3 connections. They only test bb8's scheduling behavior — which connection type is used is irrelevant. `FakeConnection` with `OkManager` is sufficient.

### Making Pop3ConnectionManager Testable

Two strategies for testing `Pop3ConnectionManager` itself without real TCP:

**Strategy A: Constructor injection (preferred)**

Add a `#[cfg(test)]`-only constructor that accepts a pre-built `Pop3Client` instead of connecting:

```rust
// In pool.rs, under #[cfg(test)]
impl Pop3ConnectionManager {
    /// Test-only constructor — injects a pre-built authenticated client.
    /// Used to test is_valid() and has_broken() without real TCP.
    pub(crate) fn from_client(client: Pop3Client) -> Self {
        // Store the client in an Option<Pop3Client> internal field
        // connect() will return it once, subsequent calls fail
        todo!()
    }
}
```

This is not needed for pool behavior tests. It's only needed for testing `is_valid()` / `has_broken()` logic on the manager directly.

**Strategy B: Separate MockPop3Manager (simpler)**

For pool-level tests, don't use `Pop3ConnectionManager` at all. Define a `MockPop3Manager` that implements `ManageConnection<Connection = FakeConnection>` and test the `Pop3Pool` registry logic with it via a generic parameter:

```rust
// Make Pop3Pool generic over the manager type for tests
// This requires Pop3Pool to be generic: Pop3Pool<M: ManageConnection>
// Or: use a type alias in tests
```

The simpler path is to test `Pop3Pool` registry logic (get-or-create, concurrent creation, DashMap access patterns) using `MockPop3Manager`, and test `Pop3ConnectionManager` health check methods separately with scripted `Pop3Client` mocks.

---

## Part 2: Testing Pool Checkout/Return Behavior

### The Canonical `oneshot` Channel Pattern

bb8's test suite uses `tokio::sync::oneshot` channels to coordinate concurrent tasks. The pattern for testing "checkout blocks when pool is exhausted":

```rust
// Tests that a second checkout blocks until the first is returned.
// Verified pattern from bb8 test suite (test_acquire_release / test_get_timeout).
#[tokio::test]
async fn pool_blocks_second_checkout_until_first_is_returned() {
    use tokio::sync::oneshot;

    let pool = bb8::Pool::builder()
        .max_size(1)
        .connection_timeout(Duration::from_millis(500))
        .build(OkManager)
        .await
        .unwrap();

    // Checkout 1: acquire the single connection and hold it
    let (hold_tx, hold_rx) = oneshot::channel::<()>();
    let (released_tx, released_rx) = oneshot::channel::<()>();

    let pool_clone = pool.clone();
    tokio::spawn(async move {
        let _conn = pool_clone.get().await.unwrap(); // holds the connection
        hold_tx.send(()).unwrap();                   // signal: "I have it"
        released_rx.await.unwrap();                  // wait: "ok, drop now"
        // _conn drops here, returning to pool
    });

    // Wait until task 1 holds the connection
    hold_rx.await.unwrap();

    // Assert pool is exhausted
    assert_eq!(pool.state().connections, 1);
    assert_eq!(pool.state().idle_connections, 0);

    // Signal task 1 to release
    released_tx.send(()).unwrap();

    // Now checkout should succeed (connection back in pool)
    let _conn2 = pool.get().await.unwrap();
    assert_eq!(pool.state().statistics.connections_created, 1); // reused, not new
}
```

### Testing Timeout on Exhausted Pool

```rust
#[tokio::test]
async fn pool_checkout_times_out_when_exhausted() {
    use tokio::sync::oneshot;

    let pool = bb8::Pool::builder()
        .max_size(1)
        .connection_timeout(Duration::from_millis(100)) // short timeout
        .retry_connection(false)
        .build(OkManager)
        .await
        .unwrap();

    // Hold the single connection
    let (hold_tx, hold_rx) = oneshot::channel::<()>();
    let pool_clone = pool.clone();
    tokio::spawn(async move {
        let _conn = pool_clone.get().await.unwrap();
        hold_tx.send(()).unwrap();
        // hold forever (task never releases)
        tokio::time::sleep(Duration::from_secs(10)).await;
    });

    hold_rx.await.unwrap(); // confirm task has the connection

    // Second checkout must time out
    let result = pool.get().await;
    assert!(matches!(result, Err(bb8::RunError::TimedOut)));
    assert_eq!(pool.state().statistics.get_timed_out, 1);
}
```

### Testing Return by Drop (RAII)

```rust
#[tokio::test]
async fn connection_returns_to_pool_on_drop() {
    let pool = bb8::Pool::builder()
        .max_size(1)
        .build(OkManager)
        .await
        .unwrap();

    {
        let _conn = pool.get().await.unwrap();
        assert_eq!(pool.state().idle_connections, 0);
        // _conn drops here
    }

    // Connection must be back in the pool
    assert_eq!(pool.state().idle_connections, 1);

    // Can checkout again immediately
    let _conn2 = pool.get().await.unwrap();
}
```

### Testing Concurrent Access with Barrier

The `tokio::sync::Barrier` pattern is used when you want N tasks to all reach a point simultaneously before proceeding:

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_checkouts_from_different_accounts_do_not_block_each_other() {
    use std::sync::Arc;
    use tokio::sync::Barrier;

    // Two accounts: each has its own pool with max_size(1)
    let pool_a = Arc::new(
        bb8::Pool::builder().max_size(1).build(OkManager).await.unwrap()
    );
    let pool_b = Arc::new(
        bb8::Pool::builder().max_size(1).build(OkManager).await.unwrap()
    );

    let barrier = Arc::new(Barrier::new(3)); // 2 tasks + 1 main

    let pool_a_clone = Arc::clone(&pool_a);
    let barrier_clone = Arc::clone(&barrier);
    let task_a = tokio::spawn(async move {
        let _conn = pool_a_clone.get().await.unwrap();
        barrier_clone.wait().await; // signal: I have account A's connection
        tokio::time::sleep(Duration::from_millis(50)).await;
    });

    let pool_b_clone = Arc::clone(&pool_b);
    let barrier_clone = Arc::clone(&barrier);
    let task_b = tokio::spawn(async move {
        let _conn = pool_b_clone.get().await.unwrap();
        barrier_clone.wait().await; // signal: I have account B's connection
        tokio::time::sleep(Duration::from_millis(50)).await;
    });

    // Both tasks must reach the barrier (both acquired their connections)
    // without blocking each other
    tokio::time::timeout(Duration::from_millis(200), barrier.wait())
        .await
        .expect("tasks must not block each other");

    let _ = task_a.await;
    let _ = task_b.await;
}
```

**Important runtime note:** Concurrent tests that use `tokio::spawn` require the multi-thread runtime. Use `#[tokio::test(flavor = "multi_thread", worker_threads = 2)]` for tests involving concurrent pool access. The default `#[tokio::test]` uses a single-thread runtime where `tokio::spawn` still works but tasks run cooperatively, not truly in parallel. For pool blocking tests, the single-thread runtime is sufficient because async tasks yield at `.await` points. For true parallelism tests (like the Barrier test above), use multi-thread.

---

## Part 3: Testing Health Checks

### Testing `is_valid()` (NOOP probe)

`is_valid()` is only called when `test_on_check_out(true)` is set on the pool. To test it:

**Option A: Test `Pop3ConnectionManager::is_valid()` directly (unit test)**

```rust
// In pool.rs tests — tests is_valid directly, no pool involved
#[tokio::test]
async fn is_valid_sends_noop_and_succeeds_on_ok_response() {
    use tokio_test::io::Builder;
    let mock = Builder::new()
        .write(b"NOOP\r\n")
        .read(b"+OK\r\n")
        .build();
    let mut client = build_authenticated_test_client(mock);
    let manager = Pop3ConnectionManager::new(/* config */);
    // Call is_valid directly
    manager.is_valid(&mut client).await.unwrap();
}

#[tokio::test]
async fn is_valid_fails_on_server_error() {
    use tokio_test::io::Builder;
    let mock = Builder::new()
        .write(b"NOOP\r\n")
        .read(b"-ERR server error\r\n")
        .build();
    let mut client = build_authenticated_test_client(mock);
    let manager = Pop3ConnectionManager::new(/* config */);
    let result = manager.is_valid(&mut client).await;
    assert!(result.is_err());
}
```

**Option B: Test via pool with AtomicBool-controlled manager**

For testing that a failing `is_valid()` causes bb8 to close and recreate the connection:

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

struct FlipValidManager {
    make_invalid: Arc<AtomicBool>,
}

impl bb8::ManageConnection for FlipValidManager {
    type Connection = FakeConnection;
    type Error = TestError;

    fn connect(&self) -> impl Future<Output = Result<Self::Connection, Self::Error>> + Send {
        async { Ok(FakeConnection) }
    }

    fn is_valid(
        &self,
        _conn: &mut Self::Connection,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        let invalid = self.make_invalid.load(Ordering::SeqCst);
        async move {
            if invalid {
                Err(TestError)
            } else {
                Ok(())
            }
        }
    }

    fn has_broken(&self, _conn: &mut Self::Connection) -> bool {
        false
    }
}

#[tokio::test]
async fn invalid_connection_is_closed_and_recreated() {
    let make_invalid = Arc::new(AtomicBool::new(false));
    let manager = FlipValidManager { make_invalid: Arc::clone(&make_invalid) };

    let pool = bb8::Pool::builder()
        .max_size(1)
        .test_on_check_out(true)
        .retry_connection(false)
        .build(manager)
        .await
        .unwrap();

    // First checkout: valid
    let _conn = pool.get().await.unwrap();
    drop(_conn);

    // Mark connections as invalid
    make_invalid.store(true, Ordering::SeqCst);

    // Next checkout: is_valid() fails → bb8 closes connection and calls connect() again
    // With retry_connection(false) and no valid connect() either, this should fail
    let result = pool.get().await;
    assert!(result.is_err()); // or check statistics
}
```

### Testing `has_broken()` (synchronous state check)

`has_broken()` checks in-memory state — specifically `Pop3Client::is_closed()` from Phase 5. For Phase 8 tests:

```rust
// Unit test for has_broken — tests the Pop3ConnectionManager method directly
#[test]
fn has_broken_returns_true_for_closed_client() {
    let manager = Pop3ConnectionManager::new(/* config */);
    let mut client = make_closed_client(); // Pop3Client in Closed state
    assert!(manager.has_broken(&mut client));
}

#[test]
fn has_broken_returns_false_for_live_client() {
    let manager = Pop3ConnectionManager::new(/* config */);
    let mock = Builder::new().build();
    let mut client = build_authenticated_test_client(mock);
    assert!(!manager.has_broken(&mut client));
}
```

**The BrokenConnectionManager pattern** (from bb8 test suite) for testing that bb8 correctly discards broken connections:

```rust
// From bb8/tests/test.rs — BrokenConnectionManager
struct AlwaysBrokenManager;

impl bb8::ManageConnection for AlwaysBrokenManager {
    type Connection = FakeConnection;
    type Error = TestError;

    fn connect(&self) -> impl Future<Output = Result<Self::Connection, Self::Error>> + Send {
        async { Ok(FakeConnection) }
    }

    fn is_valid(&self, _conn: &mut Self::Connection)
        -> impl Future<Output = Result<(), Self::Error>> + Send {
        async { Ok(()) }
    }

    fn has_broken(&self, _conn: &mut Self::Connection) -> bool {
        true // always broken
    }
}

#[tokio::test]
async fn broken_connections_are_discarded_and_not_returned_to_pool() {
    let pool = bb8::Pool::builder()
        .max_size(1)
        .build(AlwaysBrokenManager)
        .await
        .unwrap();

    let _conn = pool.get().await.unwrap();
    drop(_conn);

    // bb8 detects has_broken() == true on return → closes the connection
    assert_eq!(pool.state().statistics.connections_closed_broken, 1);
    assert_eq!(pool.state().idle_connections, 0);
}
```

### Simulating Connection Failure Mid-Use

For testing the scenario where a connection dies while checked out (e.g., server closes TCP):

Using `tokio_test::io::Builder`, you can cause a read error mid-sequence:

```rust
#[tokio::test]
async fn is_valid_detects_connection_dropped_by_server() {
    use tokio_test::io::Builder;
    // Mock that returns EOF immediately (simulates server closing connection)
    let mock = Builder::new()
        .write(b"NOOP\r\n")
        // No read response — EOF simulates dead connection
        .build();
    let mut client = build_authenticated_test_client(mock);
    let manager = Pop3ConnectionManager::new(/* config */);
    let result = manager.is_valid(&mut client).await;
    assert!(result.is_err()); // is_valid must fail on EOF
}
```

---

## Part 4: Testing the Account Registry

### Testing Two Accounts Accessed Concurrently

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_accounts_can_be_checked_out_simultaneously() {
    use std::sync::Arc;
    use tokio::sync::Barrier;

    let pool = Arc::new(Pop3Pool::new());
    let key_a = AccountKey { host: "a.example.com".into(), port: 110, username: "alice".into() };
    let key_b = AccountKey { host: "b.example.com".into(), port: 110, username: "bob".into() };

    let barrier = Arc::new(Barrier::new(3));

    let pool_a = Arc::clone(&pool);
    let key_a_clone = key_a.clone();
    let barrier_a = Arc::clone(&barrier);
    let task_a = tokio::spawn(async move {
        let _conn = pool_a.get(&key_a_clone).await.unwrap();
        barrier_a.wait().await; // signal: both tasks have their connections
        tokio::time::sleep(Duration::from_millis(100)).await;
    });

    let pool_b = Arc::clone(&pool);
    let key_b_clone = key_b.clone();
    let barrier_b = Arc::clone(&barrier);
    let task_b = tokio::spawn(async move {
        let _conn = pool_b.get(&key_b_clone).await.unwrap();
        barrier_b.wait().await;
        tokio::time::sleep(Duration::from_millis(100)).await;
    });

    // Both tasks must reach the barrier — i.e., neither blocked the other
    tokio::time::timeout(Duration::from_millis(500), barrier.wait())
        .await
        .expect("accounts must not block each other");

    let _ = task_a.await;
    let _ = task_b.await;
}
```

**Note:** This test requires `Pop3Pool` to use real `Pop3ConnectionManager` or a test-injectable factory. Use the generic-manager pattern or inject a mock factory.

### Testing Same Account Blocks Second Caller

```rust
#[tokio::test]
async fn same_account_blocks_second_caller_until_first_returns() {
    use tokio::sync::oneshot;

    let pool = Pop3Pool::new();
    let key = AccountKey { host: "mail.example.com".into(), port: 110, username: "user".into() };

    // Get the per-account bb8 pool (or test via Pop3Pool::get)
    // This test verifies max_size(1) enforcement at the Pop3Pool level

    let (hold_tx, hold_rx) = oneshot::channel::<()>();
    let (release_tx, release_rx) = oneshot::channel::<()>();

    let pool = Arc::new(pool);
    let pool_clone = Arc::clone(&pool);
    let key_clone = key.clone();

    tokio::spawn(async move {
        let _conn = pool_clone.get(&key_clone).await.unwrap();
        hold_tx.send(()).unwrap();   // signal: I have the connection
        release_rx.await.unwrap();  // wait for release signal
        // _conn drops → connection returned
    });

    hold_rx.await.unwrap(); // confirm task holds the connection

    // Start timing
    let start = std::time::Instant::now();

    // Signal release after 50ms
    let release_tx = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        release_tx.send(()).unwrap();
    });

    // This should block until the first task releases (~50ms)
    let _conn2 = pool.get(&key).await.unwrap();
    let elapsed = start.elapsed();

    assert!(elapsed >= Duration::from_millis(40), "must have waited for first connection");
    let _ = release_tx.await;
}
```

### Testing Pool Creation Race Condition

When two tasks simultaneously call `get()` for the same new account key, the registry must not create two pools:

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_pool_creation_for_same_account_creates_only_one_pool() {
    use std::sync::Arc;
    use tokio::sync::Barrier;

    let registry = Arc::new(Pop3Pool::new());
    let key = AccountKey { host: "race.example.com".into(), port: 110, username: "user".into() };
    let barrier = Arc::new(Barrier::new(5)); // 4 tasks + 1 main

    let mut handles = Vec::new();
    for _ in 0..4 {
        let registry_clone = Arc::clone(&registry);
        let key_clone = key.clone();
        let barrier_clone = Arc::clone(&barrier);
        handles.push(tokio::spawn(async move {
            barrier_clone.wait().await; // all 4 start simultaneously
            let _pool = registry_clone.get_or_create_pool(&key_clone).await.unwrap();
        }));
    }

    barrier.wait().await; // release all 4 tasks simultaneously
    for h in handles {
        h.await.unwrap();
    }

    // Regardless of race, there should be exactly one pool for this key
    assert_eq!(registry.pool_count(), 1);
}
```

This test verifies the DashMap `or_insert_with` pattern correctly handles concurrent inserts (the entry API is atomic — only one winner).

---

## Part 5: Send Analysis and Mock Transport Decision

### The Send Chain for `Pop3Client`

The question was: does the existing mock transport infrastructure (using `tokio_test::io::Mock`) satisfy bb8's `Connection: Send + 'static` requirement?

The chain is:

```
tokio_test::io::Mock  ──→  impl Send + Sync (confirmed: auto trait impl, commit 6919f7c)
     ↓ wrapped in
InnerStream::Mock(tokio_test::io::Mock)  ──→  Send when Mock: Send ✓
     ↓ split by io::split() into
ReadHalf<InnerStream>   ──→  impl<T: Send> Send for ReadHalf<T> ✓
WriteHalf<InnerStream>  ──→  impl<T: Send> Send for WriteHalf<T> ✓
     ↓ ReadHalf wrapped in
BufReader<ReadHalf<InnerStream>>  ──→  impl<T: Send> Send for BufReader<T> ✓
     ↓ fields of
Transport { reader: BufReader<ReadHalf<InnerStream>>, writer: WriteHalf<InnerStream>, ... }
     ──→  Send ✓ (all fields are Send)
     ↓ field of
Pop3Client { transport: Transport, greeting: String, state: SessionState }
     ──→  Send ✓ (all fields are Send)
```

**Result: `Pop3Client` with a mock transport IS `Send`.** The `Rc<RefCell<Vec<u8>>>` concern in the question prompt refers to an older test infrastructure design that this codebase does NOT use. The current codebase uses `tokio_test::io::Mock` which is `Send + Sync`.

### What This Means Practically

- Pool integration tests CAN use `Pop3Client` with `tokio_test::io::Mock` as the connection type.
- `Pop3ConnectionManager` can be tested by constructing `Pop3Client` instances from mock I/O and directly calling `is_valid()` and `has_broken()`.
- No rewrite of the mock transport is needed.
- The `Pop3Client` mock construction helpers (`build_test_client`, `build_authenticated_test_client`) remain usable in pool tests.

### The One Limitation: `'static` Bound

bb8 requires `Connection: Send + 'static`. The `Pop3Client` struct itself has no lifetime parameters and contains no borrowed data, so it satisfies `'static`. However, if `Pop3Client` were changed in the future to borrow something (e.g., a reference to a config), the pool could not hold it. Keep `Pop3Client` ownership-based.

### Recommended Test Infrastructure for pool.rs

```rust
// In src/pool.rs, under #[cfg(test)]
#[cfg(test)]
mod tests {
    use super::*;
    use tokio_test::io::Builder;

    // Build an authenticated Pop3Client backed by scripted mock I/O.
    // This type IS Send + 'static and can be used as a bb8 connection.
    fn make_mock_client(mock: tokio_test::io::Mock) -> Pop3Client {
        // Uses the existing build_authenticated_test_client helper from client.rs
        // OR constructs Pop3Client directly if needed
        crate::client::build_authenticated_test_client_pub(mock)
    }

    // Lightweight fake connection for pool-level behavior tests
    // (no POP3 protocol involved)
    #[derive(Debug, Default)]
    struct FakeConn;

    #[derive(Debug)]
    struct FakeError;

    struct AlwaysOkManager;

    impl bb8::ManageConnection for AlwaysOkManager {
        type Connection = FakeConn;
        type Error = FakeError;

        fn connect(&self) -> impl Future<Output = Result<Self::Connection, Self::Error>> + Send {
            async { Ok(FakeConn) }
        }

        fn is_valid(&self, _: &mut Self::Connection)
            -> impl Future<Output = Result<(), Self::Error>> + Send {
            async { Ok(()) }
        }

        fn has_broken(&self, _: &mut Self::Connection) -> bool { false }
    }
}
```

---

## Part 6: Complete Test Inventory

These are the specific tests to write for Phase 8. Organized by what they prove.

### Group 1: Pop3ConnectionManager Unit Tests

These tests call `Pop3ConnectionManager` methods directly — no pool involved.

| Test name | What it proves | Mock pattern |
|-----------|---------------|--------------|
| `manager_connect_sends_user_pass_and_returns_authenticated_client` | `connect()` issues USER+PASS and returns `Pop3Client` in Authenticated state | Builder with USER/PASS exchange |
| `manager_is_valid_sends_noop_and_succeeds` | `is_valid()` sends NOOP, returns Ok on +OK | Builder with NOOP/+OK |
| `manager_is_valid_fails_on_server_error` | `is_valid()` returns Err on -ERR | Builder with NOOP/-ERR |
| `manager_is_valid_fails_on_eof` | `is_valid()` returns Err on connection drop | Builder with NOOP + no read (EOF) |
| `manager_has_broken_returns_false_for_live_connection` | `has_broken()` is false normally | mock client in Authenticated state |
| `manager_has_broken_returns_true_for_closed_connection` | `has_broken()` is true after `is_closed()` | mock client in Closed/Quit state |

### Group 2: bb8 Pool Behavior Tests (using FakeConn)

These tests verify bb8 scheduling behavior. They use `FakeConn` / `AlwaysOkManager` — no POP3 involved.

| Test name | What it proves |
|-----------|---------------|
| `pool_checkout_succeeds_when_connection_available` | Basic get() works |
| `pool_connection_returned_to_pool_on_drop` | RAII return by drop |
| `pool_blocks_second_checkout_until_first_is_returned` | max_size(1) enforces sequential access |
| `pool_checkout_times_out_when_exhausted` | RunError::TimedOut returned after connection_timeout |
| `broken_connection_is_discarded_not_returned` | has_broken() == true → connections_closed_broken incremented |
| `invalid_connection_is_closed_and_new_one_created` | is_valid() failure → connection replaced |
| `pool_statistics_track_connections_created` | connections_created == 1 after first checkout |
| `pool_statistics_track_waited_checkouts` | get_waited incremented when second task waits |

### Group 3: Pop3Pool Registry Tests

These tests verify the `Pop3Pool` outer struct — key lookup, concurrent creation, multiple accounts.

| Test name | What it proves | Runtime |
|-----------|---------------|---------|
| `registry_creates_pool_for_new_account` | get_or_create works for unknown key | single-thread |
| `registry_reuses_existing_pool_for_known_account` | same key returns same pool (no duplicate creation) | single-thread |
| `concurrent_creation_same_key_creates_one_pool` | race condition → exactly one pool | multi-thread |
| `two_different_accounts_do_not_block_each_other` | account A + account B checkout concurrently | multi-thread |
| `same_account_blocks_second_caller` | one account, two callers → second waits | multi-thread |
| `pool_count_correct_after_multiple_accounts` | registry.pool_count() after adding N accounts | single-thread |

### Group 4: Integration Test (Pop3Pool + Pop3ConnectionManager + Mock I/O)

This group requires `Pop3Client` to satisfy `Connection: Send + 'static` — which it does.

| Test name | What it proves |
|-----------|---------------|
| `pool_get_returns_authenticated_pop3client` | full checkout flow using mock POP3 server exchange |
| `pool_runs_noop_on_checkout_when_test_on_check_out_enabled` | NOOP sent to scripted mock on checkout |
| `pool_replaces_dead_connection_on_next_checkout` | connection returns EOF → is_valid() fails → new connect() called |

---

## Part 7: Anti-Patterns and Pitfalls

### Pitfall 1: Single-Thread Runtime for Concurrent Tests

```rust
// WRONG — default tokio::test is single-threaded; blocking in spawn may deadlock
#[tokio::test]
async fn blocking_test() { ... tokio::spawn(...) ... }

// CORRECT — use multi_thread for tests that need real parallelism
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_test() { ... }
```

**When single-thread is fine:** Tests using `oneshot` channels where task B only needs to run when task A yields at `.await`. Async tasks cooperate at yield points even on single-thread.

**When multi-thread is required:** Tests using `Barrier` where all N tasks must make progress simultaneously. On a single-thread runtime, if the main thread is blocked on the barrier, the spawned tasks may never run.

### Pitfall 2: Holding Pool Handle Across Await in Registry Test

When testing `get_or_create_pool`, ensure you follow the same DashMap-safe pattern being tested:

```rust
// WRONG in tests — holding DashMap Ref across .await will deadlock
let entry = pool.pools.get(&key);  // holds shard lock
some_async_op().await;             // DEADLOCK if DashMap ref still live
drop(entry);

// CORRECT — clone the Arc before releasing the ref
let pool_arc = {
    let entry = pool.pools.get(&key);
    entry.map(|e| Arc::clone(&e))
};
// entry (DashMap Ref) is dropped here, shard lock released
if let Some(p) = pool_arc {
    p.get().await?;  // safe — no DashMap lock held
}
```

### Pitfall 3: Using `std::sync::Mutex` in `connect()` Closures

If a manager's `connect()` needs to access shared state (e.g., a counter), use `Arc<tokio::sync::Mutex<T>>`, not `Arc<std::sync::Mutex<T>>`. The `connect()` future must be `Send` and may be awaited across thread boundaries.

```rust
// WRONG — std::sync::MutexGuard is not Send
struct BadManager { counter: Arc<std::sync::Mutex<u32>> }
impl ManageConnection for BadManager {
    fn connect(&self) -> impl Future<Output = ...> + Send {
        let counter = Arc::clone(&self.counter);
        async move {
            let mut guard = counter.lock().unwrap(); // guard held across await → not Send
            *guard += 1;
            Ok(FakeConnection)
        }
    }
}

// CORRECT — use std::sync::Mutex but drop guard before .await, OR use AtomicU32
struct GoodManager { counter: Arc<std::sync::AtomicU32> }
impl ManageConnection for GoodManager {
    fn connect(&self) -> impl Future<Output = ...> + Send {
        let counter = Arc::clone(&self.counter);
        async move {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(FakeConnection)
        }
    }
}
```

### Pitfall 4: `tokio::time::pause()` in Pool Tests

`tokio::time::pause()` from `tokio::test` fast-forwards time for tests involving `sleep()` and `timeout()`. Pool timeout tests benefit from this. However, `tokio::time::pause()` only works in the `current_thread` (single-thread) runtime. If a pool timeout test needs `multi_thread`, use real `sleep()` with short durations (e.g., 50ms), not paused time.

```rust
// Timeout test using paused time — single-thread only
#[tokio::test]
async fn pool_timeout_with_paused_time() {
    tokio::time::pause(); // works in current_thread
    let pool = bb8::Pool::builder()
        .max_size(1)
        .connection_timeout(Duration::from_secs(30))
        .build(OkManager).await.unwrap();

    let _conn = pool.get().await.unwrap();
    tokio::time::advance(Duration::from_secs(31)).await;

    let result = pool.get().await;
    assert!(matches!(result, Err(bb8::RunError::TimedOut)));
}
```

### Pitfall 5: Pool Builder `build()` vs `build_unchecked()`

`Pool::builder().build(manager).await` eagerly creates `min_idle` connections (default: 0, so no connections are created). With `min_idle(Some(1))`, `build()` blocks until one connection is established. For tests that want lazy initialization:

```rust
// Use min_idle(None) or min_idle(Some(0)) to avoid eager connection in tests
let pool = bb8::Pool::builder()
    .max_size(1)
    .min_idle(Some(0))  // don't pre-connect
    .build(manager)
    .await
    .unwrap();
```

Alternatively, `build_unchecked()` returns immediately without establishing connections but also doesn't validate the manager.

---

## Part 8: Recommended Test File Structure

```
src/
└── pool.rs   ← all pool code + inline tests

// Inside pool.rs:

#[cfg(test)]
mod tests {
    mod manager_tests {
        // Group 1: Pop3ConnectionManager unit tests
        // Use tokio_test::io::Builder directly
    }

    mod pool_behavior_tests {
        // Group 2: FakeConn + AlwaysOkManager tests
        // Tests bb8 scheduling — no POP3 involved
        // Most tests: #[tokio::test] (single-thread sufficient)
    }

    mod registry_tests {
        // Group 3: Pop3Pool registry tests
        // concurrent tests: #[tokio::test(flavor = "multi_thread")]
    }

    mod integration_tests {
        // Group 4: Pop3Pool + Pop3ConnectionManager + tokio_test mock
        // Requires Pop3Client Send bound — confirmed working
    }
}
```

**No separate integration test file is needed** — bb8's own test suite pattern puts everything in-module under `#[cfg(test)]`. The pool tests for this crate follow the same convention used in `client.rs` and `transport.rs`.

---

## Sources

### Primary (HIGH confidence)

- [bb8 ManageConnection trait](https://docs.rs/bb8/latest/bb8/trait.ManageConnection.html) — `Connection: Send + 'static`, `Error: Debug + Send + 'static`, trait: `Sized + Send + Sync + 'static`
- [bb8 Statistics struct](https://docs.rs/bb8/latest/bb8/struct.Statistics.html) — `connections_created`, `connections_closed_broken`, `connections_closed_invalid`, `get_direct`, `get_waited`, `get_timed_out`, `get_wait_time` fields
- [bb8 State struct](https://docs.rs/bb8/latest/bb8/struct.State.html) — `connections: u32`, `idle_connections: u32`, `statistics: Statistics`
- [bb8 Pool::get() vs get_owned()](https://docs.rs/bb8/latest/bb8/struct.Pool.html) — `get()` returns `PooledConnection<'_, M>`, `get_owned()` returns `PooledConnection<'static, M>`
- [tokio_test::io::Mock auto traits](https://docs.rs/tokio-test/latest/tokio_test/io/struct.Mock.html) — confirmed `impl Send for Mock`, `impl Sync for Mock`
- [tokio::io::ReadHalf Send bound](https://docs.rs/tokio/latest/tokio/io/struct.ReadHalf.html) — `impl<T: Send> Send for ReadHalf<T>`
- [tokio commit 6919f7c](https://github.com/tokio-rs/tokio/commit/6919f7cede68dd5176525c24ad520af668bae37a) — "Make Mock both Send and Sync" — changed `rx: Pin<Box<dyn Stream + Send>>` to `UnboundedReceiverStream`

### Secondary (MEDIUM confidence)

- [bb8 test.rs source patterns](https://github.com/djc/bb8/blob/main/bb8/tests/test.rs) — `FakeConnection`, `OkManager`, `NthConnectionFailManager`, `BrokenConnectionManager` patterns; `test_get_timeout`, `test_acquire_release` test structure; oneshot channel coordination
- [Tokio Unit Testing docs](https://tokio.rs/tokio/topics/testing) — `tokio::test` flavors, `tokio::time::pause()`, single vs multi-thread runtime selection
- [Tokio Channels tutorial](https://tokio.rs/tokio/tutorial/channels) — oneshot channel pattern for task coordination
- [bb8 issue #141: auth failure hang](https://github.com/djc/bb8/issues/141) — `retry_connection(false)` required in tests to avoid 30s timeouts on expected failures

### Tertiary (LOW confidence — derived/inferred)

- `tokio::sync::Barrier` pattern for N-task synchronization — derived from Tokio sync docs + bb8 test structure; no direct bb8 example found
- `pool_count()` test assertion — assumes `Pop3Pool` will expose a method for the pool count; API not yet designed

---

## Metadata

**Confidence breakdown:**
- Send analysis for `tokio_test::io::Mock` and `Transport`: HIGH — verified against docs.rs trait impl listing and commit history
- bb8 mock manager patterns (FakeConnection, OkManager, etc.): HIGH — directly from bb8 test.rs source
- oneshot coordination pattern: HIGH — confirmed from bb8 test suite description
- Barrier pattern for concurrent tests: MEDIUM — standard Tokio sync primitive, but not seen explicitly in bb8 test suite
- `pool.state().statistics` field names: HIGH — verified against docs.rs Statistics struct
- Integration test group (Pop3Pool + mock transport): HIGH for correctness; MEDIUM for exact API shape (depends on Phase 8 implementation decisions)

**Research date:** 2026-03-01
**Valid until:** 2026-06-01
