use std::result::Result;

use serde::{Deserialize, Serialize};
use tower_lsp_server::lsp_types::NumberOrString;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DiagnosticCode {
    ExpectingToken,
    NonSnakeCase,
    UnusedInclude,
    UndefinedType,
    Deprecated,
    DuplicateDefinition,
}

impl DiagnosticCode {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            DiagnosticCode::ExpectingToken => "expecting-token",
            DiagnosticCode::NonSnakeCase => "non-snake-case",
            DiagnosticCode::UnusedInclude => "unused-include",
            DiagnosticCode::UndefinedType => "undefined-type",
            DiagnosticCode::Deprecated => "deprecated",
            DiagnosticCode::DuplicateDefinition => "duplicate-definition",
        }
    }
}

impl TryFrom<String> for DiagnosticCode {
    type Error = ();

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "expecting-token" => Ok(DiagnosticCode::ExpectingToken),
            "non-snake-case" => Ok(DiagnosticCode::NonSnakeCase),
            "unused-include" => Ok(DiagnosticCode::UnusedInclude),
            "undefined-type" => Ok(DiagnosticCode::UndefinedType),
            "deprecated" => Ok(DiagnosticCode::Deprecated),
            "duplicate-definition" => Ok(DiagnosticCode::DuplicateDefinition),
            _ => Err(()),
        }
    }
}

impl From<DiagnosticCode> for NumberOrString {
    fn from(val: DiagnosticCode) -> Self {
        NumberOrString::String(val.as_str().to_string())
    }
}
