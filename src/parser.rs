use tower_lsp::lsp_types::{Diagnostic, Url};

/// A trait for parsing FlatBuffers schema files.
/// This allows for different parsing strategies to be used.
pub trait Parser {
    /// Parses a FlatBuffers schema and returns a list of diagnostics.
    fn parse(&self, uri: &Url, content: &str) -> Vec<Diagnostic>;
}

/// A parser that uses the `flatc` command-line tool.
pub struct FlatcCommandLineParser;

impl Parser for FlatcCommandLineParser {
    fn parse(&self, _uri: &Url, _content: &str) -> Vec<Diagnostic> {
        // In Phase 2, we will implement the logic to:
        // 1. Save the content to a temporary file.
        // 2. Run `flatc --json-ast <temp_file>`
        // 3. Parse the output (stdout for AST, stderr for errors).
        // 4. Convert `flatc` errors into LSP Diagnostics.
        vec![] // Return no diagnostics for now.
    }
}
