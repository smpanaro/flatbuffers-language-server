# Design 05: Refactoring for Testability and Clarity

## 1. Motivation

The current language server architecture has evolved organically. While functional, its key data structures (`Backend`, `Workspace`) have accumulated a wide range of responsibilities, leading to several challenges:

1.  **Poor Testability:** The tight coupling between LSP communication, file management, and code analysis makes it difficult to write unit tests for the core logic. Most tests must be "integration-level," simulating a full client-server exchange, which makes it hard to assert specific internal states.
2.  **Unclear Separation of Concerns:** The `Backend` struct mixes LSP protocol handling with state management. The `Workspace` struct is a large collection of concurrent maps, where the ownership and mutation rules are not always clear, and the concurrency is often not needed.
3.  **Redundant State:** Data is sometimes duplicated (e.g., `Workspace.symbols` vs. `Workspace.file_definitions`), and runtime-constant data (like built-in keywords) is handled the same as dynamic data.

This document proposes a significant refactoring of the server's core architecture. The goal is to create a layered, more modular design that is easier to understand, maintain, and, most importantly, test.

## 2. Goals and Non-Goals

*   **Goals:**
    *   Decouple the LSP protocol layer from the core analysis engine.
    *   Introduce clear, single-responsibility components for state management and analysis.
    *   Enable fine-grained unit testing of parsing, symbol indexing, and diagnostic generation.
    *   Simplify concurrency by preferring immutable snapshots for queries and centralizing mutations.
    *   Maintain all existing language server functionality.

*   **Non-Goals:**
    *   Introducing new user-facing features as part of this refactor.
    *   Changing the underlying parsing logic (`flatc`).

## 3. Proposed Architecture

The monolithic `Backend` and `Workspace` structs will be broken down into a set of collaborating services, each with a distinct responsibility.

### 3.1. Core Components

| Layer / Type           | Responsibility                                                                                             |
| ---------------------- | ---------------------------------------------------------------------------------------------------------- |
| **`Backend`**          | Thin LSP runtime layer. Handles JSON-RPC, routes notifications to other services, and spawns async tasks. It owns the other services but does not contain analysis logic itself. |
| **`DocumentStore`**    | A thread-safe repository for the content of open files (`Rope`s). It handles applying text changes and providing snapshots of document contents. |
| **`SearchPathManager`**| Manages workspace roots and other include paths provided by the client during initialization.              |
| **`AnalysisEngine`**   | The "brains" of the server. It orchestrates parsing and analysis, building and updating the `WorkspaceIndex`. It exposes query methods for the LSP feature handlers. |
| **`WorkspaceIndex`**   | The central, passive data store for all semantic information. It contains the symbol tables, dependency graphs, and diagnostics for the entire workspace. It is not directly aware of the LSP. |
| **`WorkspaceSnapshot`**| An immutable, cheap-to-clone (`Arc`) "view" into the `WorkspaceIndex` at a specific point in time. All read-only queries (hover, definition) operate on a snapshot to ensure a consistent view of the data. |

### 3.2. Component API Sketch

This is a high-level sketch of the new data structures.

```rust
// --- In backend.rs ---
pub struct Backend {
    client: Client,
    documents: Arc<DocumentStore>,
    analysis: Arc<AnalysisEngine>,
    search_paths: Arc<SearchPathManager>,
    // ... other runtime fields like `ready` flags
}

impl Backend {
    // Example of a notification handler
    async fn on_did_change(&self, params: DidChangeTextDocumentParams) {
        self.documents.apply_changes(params);
        // Trigger a re-analysis of affected files
        self.analysis.request_reindex().await;
    }

    // Example of a request handler
    async fn on_hover(&self, params: HoverParams) -> Option<Hover> {
        // 1. Get a consistent view of the world
        let snapshot = self.analysis.snapshot();
        // 2. Delegate to pure, testable logic
        handlers::hover::compute(&snapshot, params)
    }
}

// --- In analysis/mod.rs ---
pub struct AnalysisEngine {
    // The single source of truth, protected by a lock for writes
    index: RwLock<WorkspaceIndex>,
    // The underlying parser
    parser: Parser,
}

impl AnalysisEngine {
    /// Re-parses files and updates the index.
    pub async fn reindex(&self, docs: DocumentSnapshot, search_paths: SearchPathSnapshot) {
        // ... logic to determine which files to parse ...
        let new_index_data = self.parser.parse_workspace(docs, search_paths);
        // ... update the index ...
        *self.index.write().unwrap() = new_index_data;
    }

    /// Returns a cheap, thread-safe, immutable snapshot for querying.
    pub fn snapshot(&self) -> WorkspaceSnapshot {
        WorkspaceSnapshot {
            inner: Arc::new(self.index.read().unwrap().clone()),
        }
    }
}

// --- In analysis/workspace_index.rs ---
#[derive(Default, Clone)]
pub struct WorkspaceIndex {
    pub symbols: SymbolIndex,
    pub dependencies: DependencyGraph,
    pub diagnostics: DiagnosticStore,
    pub root_types: RootTypeStore,
}

// An immutable, queryable view
#[derive(Clone)]
pub struct WorkspaceSnapshot {
    inner: Arc<WorkspaceIndex>,
}

impl WorkspaceSnapshot {
    // Example query method
    pub fn symbol_at(&self, location: &Location) -> Option<&Symbol> {
        // ... logic to find a symbol ...
    }
}

// --- In analysis/symbol_index.rs ---
#[derive(Default, Clone)]
pub struct SymbolIndex {
    /// Map from a fully-qualified name to its definition.
    global: HashMap<String, Symbol>,
    /// Map from a file path to the list of symbol keys defined in it.
    per_file: HashMap<PathBuf, Vec<String>>,
    /// Pre-populated, immutable map of built-in symbols.
    builtins: Arc<HashMap<String, Symbol>>,
    /// Pre-populated, immutable set of keywords.
    keywords: Arc<HashSet<String>>,
}
```

## 4. Benefits of the New Design

*   **Testability:**
    *   The `AnalysisEngine` can be instantiated in a unit test with a mock `DocumentStore`. We can call `reindex` and then inspect the `WorkspaceSnapshot` directly to assert that symbols, diagnostics, and dependencies were created correctly, without any LSP ceremony.
    *   Feature handlers like `hover::compute` become pure functions that take a `WorkspaceSnapshot` and return a result, making them trivial to test.
*   **Clarity and Separation of Concerns:** Each component has a well-defined job. The `Backend` only handles LSP plumbing. The `AnalysisEngine` handles analysis orchestration. The `WorkspaceIndex` is just dumb data.
*   **Simplified Concurrency:** The overuse of `DashMap` is eliminated. Mutations are centralized within the `AnalysisEngine`, which takes a write lock on the `WorkspaceIndex`. All read queries are non-blocking, operating on cheap-to-clone `Arc` snapshots. This makes reasoning about state much simpler.
*   **Foundation for Future Features:** This clean, queryable model makes it easier to implement complex, workspace-wide features like "Find All References" or "Rename Symbol" by simply adding new query methods to the `WorkspaceSnapshot`.

## 5. Migration Path

This refactor can be performed incrementally to minimize disruption.

1.  **Phase 1: Extract `DocumentStore` and `SearchPathManager`**
    *   Create the `DocumentStore` struct and move `Backend.document_map` into it. Update `didOpen`/`didChange`/`didClose` handlers to delegate to the new store.
    *   Create the `SearchPathManager` and move `search_paths` and `workspace_roots` into it.

2.  **Phase 2: Introduce `WorkspaceIndex` and Snapshots**
    *   Create a new `WorkspaceIndex` struct that, for now, still contains the `DashMap`s from the old `Workspace` struct.
    *   Create the `AnalysisEngine` to own this new index.
    *   Introduce the `WorkspaceSnapshot` that holds an `Arc` to the index.
    *   Refactor all feature handlers (`hover`, `goto_definition`, etc.) to take a `WorkspaceSnapshot` as their primary input, even if they are still accessing the concurrent maps within it.

3.  **Phase 3: Decompose the `WorkspaceIndex`**
    *   Break the `WorkspaceIndex` into the smaller, more focused stores (`SymbolIndex`, `DependencyGraph`, `DiagnosticStore`).
    *   Replace the `DashMap`s with standard `HashMap`s within these new stores.
    *   Update the `AnalysisEngine`'s `reindex` logic to build these new structures.
    *   Update the query methods on `WorkspaceSnapshot` to read from the new `HashMap`s.

4.  **Phase 4: Isolate Built-ins**
    *   Modify the `SymbolIndex` to store built-in symbols and keywords in separate, immutable `Arc<HashMap<...>>` structures that are initialized only once at startup. This removes them from the hot path of re-indexing.

By following this phased approach, a junior engineer can methodically and safely transition the codebase to the new architecture, with a clear and testable state at the end of each phase.
