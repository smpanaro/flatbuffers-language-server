use std::collections::HashMap;
use tower_lsp_server::lsp_types::{request::Request, Diagnostic, Uri};

pub enum AllDiagnostics {}

impl Request for AllDiagnostics {
    type Params = ();
    type Result = HashMap<Uri, Vec<Diagnostic>>;
    const METHOD: &'static str = "test/allDiagnostics";
}
