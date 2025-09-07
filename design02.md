# Workspace and Symbol Management Refactor

## 1. Motivation

The current language server implementation has several architectural limitations that make it difficult to maintain and extend:

1.  **Data Duplication:** The `flatc` parser processes a file and all its includes, leading to the same symbol definitions being stored multiple times in different per-file `SymbolTable`s. This is inefficient and can lead to inconsistent state.
2.  **Complex Request Logic:** Feature implementations like `on_hover` and `goto_definition` contain complex, duplicated logic for resolving symbols. This makes them brittle and hard to reason about.
3.  **Inadequate Data Modeling:** Special cases like `root_type` declarations and built-in scalar types are not modeled cleanly, leading to awkward workarounds in the request handling logic.
4.  **Lack of Central State:** Without a centralized, queryable model of the entire workspace, implementing advanced cross-file features ("Find All References", project-wide diagnostics) is difficult and inefficient.

This document proposes a refactor to address these issues by introducing a centralized workspace model.

## 2. Proposed Architecture

The core of the new architecture is a `Workspace` struct, which will serve as the single source of truth for all semantic information in the project. The per-file `symbol_map` will be removed and replaced by this central store.

### Core Data Structures

```rust
// In a new `src/workspace.rs`
pub struct Workspace {
    /// A map from a fully qualified symbol name to the Symbol object.
    /// This is the single source of truth for all symbol definitions.
    symbols: DashMap<String, Symbol>,

    /// A map from a file's URI to a list of the fully qualified names of symbols
    /// defined within that file. This is crucial for efficiently updating
    /// the `symbols` map when a file changes.
    file_definitions: DashMap<Url, Vec<String>>,

    /// A map to store information about `root_type` declarations, keyed by file URI.
    root_types: DashMap<Url, RootTypeInfo>,
}

pub struct RootTypeInfo {
    /// The location of the `root_type` keyword and type name.
    pub location: Location,
    /// The name of the type that is declared as the root type (e.g., "MyTable").
    pub type_name: String,
}

// In `src/main.rs`
struct Backend {
    client: Client,
    document_map: DashMap<String, String>,
    workspace: Workspace, // The new central store
    parser: FlatcFFIParser,
}
```

### Key Concepts

-   **Centralization:** All symbol definitions from all parsed files will live in `workspace.symbols`. This eliminates data duplication and ensures consistency.
-   **Built-in Types:** The `workspace.symbols` map will be pre-populated on server startup with definitions for built-in types (`int`, `string`, etc.) containing documentation but no source location.
-   **Symbol Resolution:** A new internal function, `resolve_symbol_at(uri, position)`, will be created to centralize the logic of finding what symbol or reference exists at a specific cursor position. This will dramatically simplify `on_hover` and `goto_definition`.

## 3. Phased Implementation Plan

This refactor can be broken down into three distinct, testable phases.

### Phase 1: Introduce `Workspace` and Centralize Symbols

**Goal:** Replace the `symbol_map` with the new `Workspace` and adapt the parsing pipeline to populate the central `symbols` map.

1.  **Create `workspace.rs`:** Define the `Workspace` and `RootTypeInfo` structs.
2.  **Update `Backend`:** Replace `symbol_map: DashMap<String, SymbolTable>` with `workspace: Workspace`.
3.  **Modify Parsing Logic:**
    -   In `on_change` (or `did_open`/`did_change`), when a file is parsed, retrieve the list of symbols.
    -   Before adding the new symbols, use `workspace.file_definitions` to find and remove the old symbols for that file from `workspace.symbols`.
    -   Iterate through the new symbols from the parser, insert them into `workspace.symbols`, and update `workspace.file_definitions` with the new list of symbol names for the file.
4.  **Adapt Existing Features:**
    -   Temporarily modify `on_hover` and `goto_definition` to query the new `workspace.symbols` map. The logic will still be complex, but it will be pointed at the new data source. This involves iterating through `workspace.symbols.values()` instead of the old `symbol_map`.

**Testing:** After this phase, diagnostics and single-file language features should still work. The key is to verify that the `Workspace` is being populated correctly and that features can read from it.

### Phase 2: Refactor `hover` and `goto_definition` with a Resolver

**Goal:** Consolidate all symbol lookup logic into a single, clean function.

1.  **Define `ResolvedSymbol`:** Create an enum that represents the outcome of a symbol lookup (e.g., `Definition(&Symbol)`, `Reference{ to: &Symbol }`, `BuiltIn(&Symbol)`).
2.  **Implement `resolve_symbol_at`:** Create this new private method on `Backend`. Move all the complex symbol-finding logic from `on_hover` and `goto_definition` into it. This function will query the `Workspace` and contain the logic for handling field types, union variants, etc.
3.  **Rewrite `on_hover` and `goto_definition`:** These methods should now be very simple. They will call `resolve_symbol_at` and then use a `match` on the `ResolvedSymbol` result to build the appropriate LSP response.

**Testing:** `hover` and `goto_definition` should have the exact same behavior as before, but the underlying code will be vastly cleaner and duplication-free. Test by hovering and going to definition on a wide variety of symbols.

### Phase 3: Improve `root_type` and Built-in Type Handling

**Goal:** Cleanly model `root_type` and add support for built-in type documentation.

1.  **Update Parser for `root_type`:** Modify `FlatcFFIParser` to stop creating a fake symbol named `"root_type"`. Instead, it should identify the `root_type` declaration and return an `Option<RootTypeInfo>`.
2.  **Populate `workspace.root_types`:** The `on_change` logic will now take this `RootTypeInfo` from the parser and store it in the `workspace.root_types` map.
3.  **Pre-populate Built-ins:** In `main`, upon `Backend` creation, add a function that populates `workspace.symbols` with `Symbol` objects for all built-in FlatBuffers types.
4.  **Enhance `resolve_symbol_at`:** Update the resolver to:
    -   Use `workspace.root_types` to correctly identify when the cursor is on a `root_type` declaration.
    -   Correctly identify references to built-in types and return the `ResolvedSymbol::BuiltIn` variant.

**Testing:** Hovering over a `root_type` declaration should now work correctly. Hovering over a built-in type like `string` in a field definition should show its documentation.

## 4. Future Work

This refactor provides a solid foundation for many future enhancements, including:

-   **Performance:** Implement a positional index (e.g., an interval tree) for `resolve_symbol_at` to avoid linear scans.
-   **Project-Wide Awareness:** Implement a full workspace scan on startup to provide features for unopened files.
-   **Dependency Tracking:** Build a full include graph to provide instant, accurate diagnostics for dependent files when a change is made.
