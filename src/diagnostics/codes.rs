use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DiagnosticCode {
    ExpectingToken,
    NonSnakeCase,
    UnusedInclude,
    UndefinedType,
}

impl DiagnosticCode {
    #[must_use] pub fn as_str(&self) -> &'static str {
        match self {
            DiagnosticCode::ExpectingToken => "expecting-token",
            DiagnosticCode::NonSnakeCase => "non-snake-case",
            DiagnosticCode::UnusedInclude => "unused-include",
            DiagnosticCode::UndefinedType => "undefined-type",
        }
    }
}

impl TryFrom<String> for DiagnosticCode {
    type Error = ();

    fn try_from(value: String) -> std::result::Result<Self, Self::Error> {
        match value.as_str() {
            "expecting-token" => Ok(DiagnosticCode::ExpectingToken),
            "non-snake-case" => Ok(DiagnosticCode::NonSnakeCase),
            "unused-include" => Ok(DiagnosticCode::UnusedInclude),
            "undefined-type" => Ok(DiagnosticCode::UndefinedType),
            _ => Err(()),
        }
    }
}
