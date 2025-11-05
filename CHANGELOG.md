## Unreleased

- Fuzzy search across all project types (tables, structs, enums etc).
  - VSCode calls this "Go to Symbol in Workspace". Zed, "project symbols".
- Add namespace to completions so editors can show and style based on it.
- Fixed invalid syntax in enum and union hovers. This resulted in incorrect highlighting in some editors.
- Fixed reporting diagnostics from included files on the wrong line. This occasionally led to crashes.

## 0.0.1 - October 19, 2025

This is the initial release!

The language server supports several standard LSP features when editing FlatBuffers schema files:

- Hover to see type definitions and comments.
- Click to go to definition or see references.
- Completions for types and keywords.
- Real `flatc` errors and warnings in your editor.
- Quick fixes for some errors.
- Rename custom types across files.

You can use it with extensions for [Zed](https://github.com/smpanaro/zed-flatbuffers) and [VSCode](https://marketplace.visualstudio.com/items?itemName=smpanaro.flatbuffers-language-server).


## 0.0.0 - October 18, 2025

Pre-release for testing purposes.
