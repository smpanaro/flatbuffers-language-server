## LSP Integration Testing Design Document

### 1. Overview

This document outlines the design for a new integration testing suite for the FlatBuffers language server. The primary goal is to create a robust, maintainable, and layered testing framework that can validate the server's correctness for both static schema analysis and dynamic, stateful IDE interactions.

The proposed solution is a fixture-based integration test suite built within a new `tests/` directory. It will leverage `tower-lsp`'s built-in testing client to simulate the LSP client-server communication in-memory, directly exercising the existing `Backend` service.

This design specifies two primary types of integration tests, which can be used in concert:

1.  **Programmatic Integration Tests:** These tests use explicit, handwritten assertions (`assert_eq!`) to verify specific, critical assumptions about the server's behavior (e.g., "a diagnostic appears on the correct line"). They are resilient to minor textual changes in messages or documentation.
2.  **Snapshot-based Integration Tests:** These tests capture the full JSON output of an LSP response and compare it against a stored "snapshot" file. They are excellent for ensuring the overall correctness of complex responses and preventing unintended regressions.

### 2. Goals

*   **Validate Core LSP Features:** Establish a testing harness for core features like `textDocument/hover` and `textDocument/publishDiagnostics`.
*   **Verify Critical Invariants:** Use programmatic tests to confirm fundamental logic, such as the precise location of definitions or diagnostics, independently of output formatting.
*   **Prevent Regressions:** Use comprehensive snapshot tests to detect any change in the server's output for a given input.
*   **Test Stateful Scenarios:** Create tests that simulate sequences of user actions (e.g., opening, changing, and closing files) to catch bugs related to incorrect state management within the `Workspace` and `document_map`.
*   **High Maintainability & Clarity:** Test cases should be easy to write, read, and update, with a clear distinction between high-level snapshot tests and targeted programmatic tests.

### 3. Non-Goals

*   **FFI Parser Unit Testing:** This design does not cover unit tests for the C++ parser logic.
*   **Full Editor Emulation:** We will not be testing against a real instance of VS Code.
*   **Performance Benchmarking:** While the framework can be extended for it, performance testing is not an initial goal.

### 4. Proposed Design

The testing strategy is centered around a single, shared test harness that can be used to write both programmatic and snapshot-based tests.

#### 4.1. Core Tooling

The implementation will rely on a few key crates added as `dev-dependencies`:

*   **`tower-lsp/testing`:** The official testing utility for `tower-lsp`. It allows us to instantiate our `Backend` and a connected `Client` in-memory.
*   **`insta`:** A snapshot testing library for Rust. Used for our snapshot-based tests.
*   **`tokio`:** As our server and the test client are asynchronous, all tests will be Tokio tests (`#[tokio::test]`).

#### 4.2. Test Structure

A new `tests` directory will be created at the project root, sibling to `src`.

```
./
├── src/
│   ├── ... (existing source files)
├── tests/
│   └── integration/
│       ├── main.rs         # Test runner and module declarations
│       ├── harness.rs      # The shared test harness logic
│       ├── hover.rs        # All tests related to hover functionality
│       └── diagnostics.rs  # All tests for diagnostics
│       └── scenarios.rs     # Tests for complex, multi-step scenarios
└── Cargo.toml
```

Programmatic and snapshot tests for the same feature (e.g., hover) will be co-located in the same file (e.g., `hover.rs`) but will have distinct, descriptive function names.

#### 4.3. The Test Harness (`harness.rs`)

A central `TestHarness` struct will be created to encapsulate setup logic, avoiding boilerplate.

**Responsibilities:**

1.  Initialize the server (`Backend`) and the test `Client` using the same setup logic found in `main.rs`.
2.  Provide a method to perform the LSP `initialize` handshake.
3.  Offer helper methods to simulate a virtual workspace by sending `textDocument/didOpen` notifications.

```rust
// in tests/integration/harness.rs
use flatbuffers_language_server::server::Backend;
use flatbuffers_language_server::workspace::Workspace;
use flatbuffers_language_server::parser::FlatcFFIParser;
use dashmap::DashMap;
use tower_lsp::{LspService, Client};
use tower_lsp::lsp_types::*;

pub struct TestHarness {
    pub client: Client,
    // The service needs to be held to keep the server alive.
    _service: tower_lsp::Server,
}

impl TestHarness {
    /// Creates a new harness and performs the initialization handshake.
    pub async fn new() -> Self {
        let (service, client) = LspService::new(|client| Backend {
            client,
            document_map: DashMap::new(),
            workspace: Workspace::new(),
            parser: FlatcFFIParser,
        }).test();

        // Since there are no custom initialization options, we can use default params.
        client.initialize(InitializeParams::default()).await;
        Self { client, _service: service }
    }

    /// Simulates opening all files in a virtual workspace.
    /// The workspace is defined as a slice of (filename, content).
    pub async fn open_workspace(&self, workspace: &[(&str, &str)]) {
        for (name, content) in workspace {
            // Use a valid root path for the virtual file system.
            let uri = Url::from_file_path(format!("/{}", name)).unwrap();
            self.client.did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem::new(
                    uri,
                    "flatbuffers".to_string(),
                    1,
                    (*content).to_string()
                ),
            }).await;
        }
    }
}
```

#### 4.4. Writing Test Cases: A Dual Approach

All tests will use the same fixture-driven style, but will differ in their assertion strategy.

**4.4.1. Programmatic Integration Tests**

These tests deconstruct the server's response and make targeted assertions on specific fields.

*Example: A diagnostics test that checks only the location of an error.*

```rust
// in tests/integration/diagnostics.rs
use tower_lsp::lsp_types::{Position, Range};

#[tokio::test]
async fn diagnostic_error_has_correct_range() {
    // 1. Define the fixture
    let content = "table MyTable { a: invalid_type; }";

    // 2. Setup the harness and open the file
    let harness = TestHarness::new().await;
    harness.open_workspace(&[("schema.fbs", content)]).await;

    // 3. Wait for the diagnostics notification
    let diagnostics_params = harness.client.read_notification::<PublishDiagnosticsParams>().await.unwrap();

    // 4. Perform programmatic assertions
    assert_eq!(diagnostics_params.uri.path(), "/schema.fbs");
    assert_eq!(diagnostics_params.diagnostics.len(), 1, "Expected exactly one diagnostic");

    let diagnostic = &diagnostics_params.diagnostics[0];
    let expected_range = Range::new(Position::new(0, 20), Position::new(0, 32)); // "invalid_type"
    assert_eq!(diagnostic.range, expected_range);
    // Note: We deliberately do NOT assert on `diagnostic.message`.
}
```

**4.4.2. Snapshot-based Integration Tests**

These tests serialize the entire server response and compare it to a stored snapshot, ensuring nothing has changed unexpectedly.

*Example: The same diagnostics test, but using a snapshot for comprehensive coverage.*

```rust
// in tests/integration/diagnostics.rs
use insta::assert_snapshot;

#[tokio::test]
async fn diagnostic_error_matches_snapshot() {
    // 1. Define the fixture
    let content = "table MyTable { a: invalid_type; }";

    // 2. Setup the harness and open the file
    let harness = TestHarness::new().await;
    harness.open_workspace(&[("schema.fbs", content)]).await;

    // 3. Wait for the diagnostics notification
    let diagnostics_params = harness.client.read_notification::<PublishDiagnosticsParams>().await.unwrap();

    // 4. Assert the entire result using a snapshot
    assert_snapshot!(serde_json::to_string_pretty(&diagnostics_params).unwrap());
}
```

#### 4.5. Assertion Strategies

The two assertion strategies are complementary and should be chosen based on the intent of the test.

*   **Programmatic Assertions**
    *   **Use for:** Verifying fundamental, stable contracts of your server's logic. (e.g., "Go to Definition always lands on the correct line").
    *   **Pros:** Resilient to changes in descriptive text. Makes the specific intent of the test very explicit.
    *   **Cons:** More verbose to write. Can miss unintended changes in other fields of the response.

*   **Snapshot Assertions**
    *   **Use for:** Ensuring the overall structure and content of a complex response remain consistent. Excellent for preventing regressions in hover documentation, completion items, or detailed error messages.
    *   **Pros:** Extremely concise to write. Provides comprehensive coverage of the entire response object.
    *   **Cons:** Can be brittle; a simple wording change in a hover message will require updating the snapshot.

#### 4.6. Testing Scenarios

This dual approach extends naturally to complex, stateful scenarios. A single scenario test can even mix both assertion types to leverage their respective strengths.

*Example: A scenario test that programmatically checks state changes and snapshots the final result.*

```rust
// in tests/integration/scenarios.rs
#[tokio::test]
async fn error_appears_on_change_and_is_then_cleared() {
    let initial_content = "table MyTable {}";
    let content_with_error = "table MyTable { a: invalid_type; }";

    let harness = TestHarness::new().await;
    harness.open_workspace(&[("schema.fbs", initial_content)]).await;

    // 1. Send a change to introduce an error.
    harness.client.did_change(/* ... */).await;

    // 2. Programmatically assert that ONE diagnostic appeared.
    let error_diags = harness.client.read_notification::<PublishDiagnosticsParams>().await.unwrap();
    assert_eq!(error_diags.diagnostics.len(), 1);

    // 3. Send a change to fix the error.
    harness.client.did_change(/* ... */).await;

    // 4. Snapshot the clearing notification to ensure it's a correctly formed empty list.
    let cleared_diags = harness.client.read_notification::<PublishDiagnosticsParams>().await.unwrap();
    assert_snapshot!(serde_json::to_string_pretty(&cleared_diags).unwrap());
    // Also add a programmatic check for the most critical part.
    assert!(cleared_diags.diagnostics.is_empty());
}
```

### 5. Phased Implementation Plan

This work can be broken down into the following incremental phases:

*   **Phase 1: Scaffolding and Initial Setup**
    1.  Add `insta`, `tokio`, and `serde_json` as `dev-dependencies`.
    2.  Create the directory structure `tests/integration/`.
    3.  Implement the `TestHarness` struct in `harness.rs` to instantiate `crate::server::Backend`.
    4.  Write a "hello world" test that simply initializes the server and asserts success to validate the harness.

*   **Phase 2: Establishing Both Test Patterns**
    1.  Implement the `parse_fixture` helper function to handle the `$0` cursor marker.
    2.  Implement a **programmatic** diagnostics test in `diagnostics.rs` that asserts on the `Range` of an error, as shown in section 4.4.1.
    3.  Implement a **snapshot-based** hover test in `hover.rs` to validate the full hover content for a symbol. This establishes both core patterns for the team.

*   **Phase 3: Dynamic Scenario Testing**
    1.  Implement a stateful scenario test in `scenarios.rs` that uses both programmatic and snapshot assertions, similar to the example in section 4.6.
    2.  Implement a test for the "closing a file" scenario, verifying that diagnostics are correctly cleared.

*   **Phase 4: Expansion and CI Integration**
    1.  Expand the test suite with more edge cases (e.g., schemas with `include` statements, duplicate definitions) using the most appropriate assertion strategy for each case.
    2.  Refactor any repeated logic in the test files into the central `harness.rs`.
    3.  Ensure the tests are configured to run as part of the CI pipeline (`cargo test`). Add a CI step to fail on uncommitted snapshot changes (`cargo insta test --review`).
