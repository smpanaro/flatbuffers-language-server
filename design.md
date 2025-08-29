### **Design Document: FlatBuffers Language Server**

**1. Introduction**

This document outlines the design for a Language Server Protocol (LSP) server for the FlatBuffers schema language (`.fbs`). The server will provide IDE-like features to any editor that supports the LSP. This will improve the developer experience by offering real-time feedback and code assistance when writing FlatBuffers schemas.

The primary goal is to create a server that supports the following core LSP features:
*   **Diagnostics (Linting):** Real-time error and warning reporting for syntax and semantic issues.
*   **Go to Definition:** Ability to jump from a symbol usage (e.g., a type used in a field) to its definition.
*   **Hover Information:** Displaying information about a symbol (e.g., its type and definition) when the user hovers over it.
*   **Completions:** Providing context-aware suggestions for keywords, types, and fields.

**2. Goals and Non-Goals**

*   **Goals:**
    *   Provide accurate and performant language features for `.fbs` files.
    *   Leverage the official FlatBuffers C++ compiler (`flatc`) as the source of truth for parsing to ensure correctness and compatibility.
    *   Create a well-structured and maintainable codebase.
    *   The server should be a standalone executable that communicates over standard I/O, making it editor-agnostic.

*   **Non-Goals:**
    *   Semantic Highlighting: This feature will not be part of the initial implementation.
    *   Workspace-wide symbol search or refactoring tools.
    *   Code formatting.
    *   Support for parsing JSON data that conforms to a schema; the focus is solely on the schema definition files.

**3. High-Level Architecture**

The FlatBuffers Language Server will operate as a separate process, communicating with the client (the text editor or IDE) via the Language Server Protocol. The communication will happen over standard input/output.

The server's architecture will be centered around three main components:

1.  **LSP Communication Layer:** This component is responsible for handling the LSP message transport. It will parse incoming JSON-RPC messages from the client and serialize outgoing messages. We will use a library to handle the boilerplate of LSP communication.

2.  **Document Manager:** This component will keep track of the state of open `.fbs` files in the editor. When a file is opened, changed, or closed, the Document Manager will update its internal representation. It will trigger parsing and semantic analysis upon any change.

3.  **Core Language Engine:** This is the heart of the server and consists of three sub-components:
    *   **Parser & AST Generator:** Uses the `flatc` library to parse the text of a `.fbs` file and generate an Abstract Syntax Tree (AST).
    *   **Semantic Analyzer & Symbol Table:** Traverses the AST to build a "Symbol Table," which maps every defined name (like tables, structs, and fields) to its definition, type, and scope. This stage also detects semantic errors.
    *   **Feature Providers:** These are individual modules that implement the logic for each LSP feature (e.g., `GoToDefinitionProvider`, `CompletionProvider`) by querying the Symbol Table and AST.

**Workflow:**

1.  The client sends a `textDocument/didOpen` notification with the content of a `.fbs` file.
2.  The Document Manager stores the file content.
3.  The Core Language Engine is invoked:
    *   The Parser generates an AST.
    *   The Semantic Analyzer builds the Symbol Table and identifies any errors.
4.  The server sends a `textDocument/publishDiagnostics` notification to the client with any found errors.
5.  The user types in the editor, and the client sends a `textDocument/didChange` notification. The server updates the document and repeats the analysis.
6.  The user requests a feature (e.g., hover). The client sends a request like `textDocument/hover`.
7.  The appropriate Feature Provider is invoked, which looks up the symbol at the given cursor position in the Symbol Table and returns the required information.

**4. Detailed Component Design**

#### 4.1 Parser and AST Generator

This component's responsibility is to convert the source text of a `.fbs` file into a structured representation (an AST).

*   **Technology:** We will directly use the C++ parser from the official FlatBuffers library. The `flatbuffers::Parser` class is designed to be used as a library and provides a way to parse schema files.
*   **Process:**
    1.  When a document is updated, the server will create an instance of `flatbuffers::Parser`.
    2.  The content of the `.fbs` file will be passed to the `parser.Parse()` method.
    3.  The `flatc` parser internally builds its own representation of the schema. Our server will traverse this internal representation (e.g., `parser.structs_`, `parser.enums_`) to build our own, more convenient AST.
*   **Our Custom AST:** The AST we build will be simpler and tailored for LSP operations. It needs to store:
    *   The type of each node (e.g., `Table`, `Struct`, `Field`, `Enum`).
    *   The name of the declared symbol.
    *   Crucially, the **source location** (line and column numbers) for every node and symbol. This information is available from the `flatc` parser.
    *   Relationships between nodes (e.g., a `Field` node will be a child of a `Table` node).

#### 4.2 Semantic Analyzer and Symbol Table

Once we have an AST, we need to understand its meaning. This is the job of the Semantic Analyzer. Its primary output is a Symbol Table.

*   **Symbol Table Structure:**
    *   The Symbol Table is essentially a map where the key is an identifier's name (e.g., "Monster") and the value contains information about that symbol.
    *   It must handle scoping. FlatBuffers has namespaces, so the table will be hierarchical, mapping from a fully qualified name to its definition.
    *   **Symbol Information:** For each symbol, we will store:
        *   `name`: The name of the symbol.
        *   `kind`: The type of symbol (`Table`, `Struct`, `Enum`, `Field`, etc.).
        *   `definitionLocation`: The file URI and source range where it is defined.
        *   `type`: For fields, this will store the resolved type (e.g., `string`, or a link to another `Table` symbol).
        *   `documentation`: Any `///` documentation comments associated with the symbol.

*   **Process (AST Traversal):**
    1.  The analyzer will perform a "walk" of the AST generated by the parser.
    2.  As it visits each declaration node (e.g., `table MyTable`), it will create an entry and add it to the Symbol Table.
    3.  During this traversal, it will perform semantic checks:
        *   **Duplicate Definitions:** Check if a symbol name is already defined in the current scope.
        *   **Undefined Types:** For each field, verify that its type exists in the Symbol Table (e.g., if a field is `my_table:MyOtherTable`, ensure `MyOtherTable` is defined).
    4.  Any errors found will be collected and sent to the client as diagnostics.

#### 4.3 Language Feature Implementation

Each feature will be implemented by querying the data structures built by the previous components.

*   **Diagnostics (Linting):**
    *   **Syntax Errors:** These are captured directly from the `flatbuffers::Parser` object after calling `Parse()`. The `parser.error_` string will contain the error message. We will need to parse this string to get a meaningful location.
    *   **Semantic Errors:** These are collected during the Semantic Analysis phase (e.g., duplicate definitions, unknown types).
    *   Both sets of errors are combined and sent to the client via a `textDocument/publishDiagnostics` notification.

*   **Go to Definition:**
    1.  Receive a `textDocument/definition` request with a text document URI and a position.
    2.  Identify the symbol/token at the given position.
    3.  Query the Symbol Table for that symbol's name.
    4.  If found, return the `definitionLocation` stored in the symbol's entry.

*   **Hover Information:**
    1.  Receive a `textDocument/hover` request.
    2.  Identify the symbol at the cursor position.
    3.  Look up the symbol in the Symbol Table.
    4.  Format the stored information (e.g., `(table) MySchema.Monster` and any documentation comments) into a user-friendly string and return it.

*   **Completions:**
    1.  Receive a `textDocument/completion` request.
    2.  Analyze the text immediately preceding the cursor to determine the context. For example:
        *   Inside a table, after a field name and colon (`my_field:`), we expect a type.
        *   At the start of a line inside a table, we expect a field name or a keyword.
        *   After the `root_type` keyword, we expect a table or struct name.
    3.  Based on the context, query the Symbol Table for all valid symbols in scope (e.g., all defined tables, structs, and built-in scalar types).
    4.  Return the list of suggestions.

**5. Implementation Plan**

This project can be implemented in phases to ensure steady progress.

*   **Phase 1: Basic Server Setup & Communication**
    1.  Choose a language and an LSP library (e.g., C++ with `lsp-server-cpp`, or Rust with `lsp-types`).
    2.  Set up a minimal server that can initialize with a client (like VS Code) and log messages.
    3.  Implement the Document Manager to handle `didOpen`, `didChange`, and `didClose` notifications, storing file contents in memory.

*   **Phase 2: Parser Integration and Diagnostics**
    1.  Integrate the `flatbuffers` library.
    2.  When a document is updated, run the `flatc` parser on its content.
    3.  Extract syntax errors from the parser and implement the Diagnostics feature. This provides immediate value to the user.

*   **Phase 3: AST, Symbol Table, and Semantic Analysis**
    1.  Define the data structures for the custom AST and the Symbol Table.
    2.  Implement the AST traversal logic to populate the Symbol Table.
    3.  Add semantic checks (e.g., for undefined types) and report these as diagnostics.

*   **Phase 4: Implement Language Features**
    1.  **Go to Definition:** Implement this first as it's a direct lookup in the Symbol Table.
    2.  **Hover:** Implement hover by formatting the information from the Symbol Table.
    3.  **Completions:** This is the most complex feature. Start with simple keyword completions and then add context-aware type and field completions.

By following this phased approach, a junior engineer can focus on one component at a time, building a functional and useful FlatBuffers Language Server.