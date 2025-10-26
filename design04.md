# Design: Namespace Support

This document outlines the plan to add comprehensive support for namespaces to the FlatBuffers language server. The goal is to create an intuitive and efficient user experience when working with schemas that use namespaces. This will involve changes to the data model, parser, and several LSP feature handlers.

## 1. Core Data Model Changes

To properly support namespaces, we need a canonical representation of symbols that is not dependent on the context in which they are used.

### `SymbolInfo` Struct (`symbol_table.rs`)

The `SymbolInfo` struct will be updated to store namespace information explicitly.

-   **`name: String`**: This will store the base name of the symbol (e.g., `MyType`).
-   **`namespace: Vec<String>`**: This new field will store the ordered components of the namespace (e.g., `vec!["MyNamespace", "SubNamespace"]`). An empty vector represents the global namespace.

The fully qualified name (e.g., `MyNamespace.SubNamespace.MyType`) can be constructed on demand.

### `SymbolTable`

The `SymbolTable` will continue to use the fully qualified name as the key for its internal map. This ensures that symbols with the same base name but in different namespaces are treated as distinct entries.

## 2. Parser and Symbol Extraction (`parser.rs` & FFI)

The C++ FFI layer, which wraps the official `flatc` parser, is responsible for extracting the namespace for each symbol definition.

-   The FFI will provide the fully qualified namespace as a dot-separated string.
-   In `parser.rs`, the symbol extraction logic will populate the `namespace` vector by splitting the string provided by the FFI by `.`.
-   It will then construct the fully qualified name to be used as the key in the `SymbolTable`.

## 3. LSP Feature Behavior

This section details the desired user experience for each LSP feature.

### General Principle: Resolving Qualified Names (`analysis.rs`)

A key requirement is that LSP features should only act on the relevant part of a qualified name. For a field defined as `my_field: MyNamespace.MyType;`, features should be active only when the cursor is over the `MyType` portion.

-   The `Field` symbol in the symbol table will store a `type_range` that covers the entire type string as written in the source (i.e., `MyNamespace.MyType`).
-   The `resolve_symbol_at` function in `analysis.rs` will be updated. When it identifies a symbol as a field's type, it will calculate the precise sub-`Range` that covers only the base type name (e.g., `MyType`).
-   The `ResolvedSymbol` struct returned by this function will contain this narrowed range, which will be used by all feature handlers.

### Hover (`hover.rs`)

-   **Triggering:** A hover is triggered only when the cursor is over the base type part of a symbol reference.
-   **Content:** The hover card will display the documentation for the fully qualified symbol. If the symbol is in a namespace, the hover content will be formatted to show the namespace on a preceding line.

    ```
    namespace MyNamespace;
    table MyType { ... }
    ```

### Go To Definition & Find References (`goto_definition.rs`, `references.rs`)

-   **Triggering:** These features are activated only when the cursor is on the base type part of a symbol reference.
-   **Behavior:** The action will operate on the fully qualified symbol. "Go to Definition" on `MyType` will navigate to the definition of `table MyType` within its corresponding `namespace MyNamespace;` block.

### Completion (`completion.rs`)

Completion requires sophisticated, server-side filtering and sorting to provide an intuitive experience.

#### Completion Item Structure

For a symbol `MyNamespace.MyType`, the server will generate a `CompletionItem` with the following structure:

-   **`label`**: `MyType`
-   **`insertText`**: `MyType`
-   **`filterText`**: `MyNamespace.MyType`
-   **`detail`**: `MyType in MyNamespace` (or just `MyType` if there is no namespace).

#### Server-Side Filtering Logic

The server will pre-filter the list of all symbols based on the text the user has typed (`partial_text`).

-   **Case 1: `partial_text` contains a dot (e.g., `MyNS.MyT`)**
    -   The server will match symbols whose fully qualified name (`filterText`) starts with `partial_text`.

-   **Case 2: `partial_text` does not contain a dot (e.g., `MyT` or `MyNS`)**
    -   The server will match if `partial_text` is a prefix of the symbol's base name OR a prefix of any of its namespace components.

#### Sorting Logic

To ensure the most relevant results appear first, completion items will be sorted. When `partial_text` does not contain a dot, matches on a symbol's base name will be prioritized above matches on its namespace. This can be achieved by adding a prefix to the `sortText` property (e.g., `a_` for base name matches, `b_` for namespace matches).

## 4. Implementation Plan

1.  **Update Data Model:** Modify the `SymbolInfo` struct in `symbol_table.rs`.
2.  **Enhance Parser:** Ensure the FFI and `parser.rs` correctly extract and store namespace information.
3.  **Refine Symbol Resolution:** Update `analysis.rs` to correctly calculate the range for the base type within a qualified name reference.
4.  **Update LSP Handlers:**
    -   Modify Hover, Go To Definition, and Find References.
    -   Implement the server-side completion filtering and sorting logic.
5.  **Testing:** Add comprehensive integration tests for all namespace-related scenarios.

## 5. Future Considerations

This design establishes a foundation that can be extended. The current focus is on resolving the base type within a qualified name (e.g., `MyType` in `MyNamespace.MyType`).

In the future, this could be enhanced to support features on the namespace components themselves. For example, hovering over `MyNamespace` could trigger a hover card that lists all types defined within that namespace. The `analysis.rs` logic could be extended to determine which namespace component the cursor is on, and the `SymbolTable` could be augmented with information about namespaces as entities themselves. The `namespace: Vec<String>` data structure is well-suited to support such an extension without requiring a major redesign.
