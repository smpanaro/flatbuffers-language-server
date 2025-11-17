## Unreleased

### New Features
- Fuzzy search across all project types (tables, structs, enums etc).
  - VSCode calls this "Go to Symbol in Workspace". Zed, "project symbols".
- Automatically add include statements if needed when accepting a field or root_type completion.
- Allow more features to work when there are parsing errors. This includes hint diagnostics and hovers, among others.
- Add full support for rpc_service definitions: hover, references, completions, etc.
- Add namespace to completions so editors can show and style based on it.

### Bug Fixes
- Fixed invalid syntax in enum and union hovers. This resulted in incorrect highlighting in some editors.
- Fixed reporting of diagnostics from included files on the wrong line. This occasionally led to crashes.
- Fixed unused include detection so it works in many more cases. This rarely worked previously.
  - This will flag transitive-only imports as unused (e.g. B includes C but does not use it. A includes B and uses C). If you have this use case please open an issue.

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
