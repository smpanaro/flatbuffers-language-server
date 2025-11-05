use crate::harness::TestHarness;
use flatbuffers_language_server::ext::all_diagnostics::AllDiagnostics;
use tower_lsp_server::lsp_types::notification;

#[tokio::test]
async fn include_paths_are_discovered_correctly() {
    let mut harness = TestHarness::new();

    // This would be compiled from the repo root:
    // `flatc -I ./schemas/ ./services/api.fbs`
    let common_content = "struct CommonData { id: ulong; }";
    let api_content = r#"
include "schemas/common.fbs";
table ApiRequest { data: CommonData; }
root_type ApiRequest;
"#;

    harness
        .initialize_and_open(&[
            ("schemas/common.fbs", common_content),
            ("services/api.fbs", api_content),
        ])
        .await;

    let api_uri = harness.file_uri("services/api.fbs");
    let common_uri = harness.file_uri("schemas/common.fbs");

    // The server will send two `PublishDiagnostics` notifications,
    // one each for api.fbs and common.fbs. We need to check both.
    for _ in 0..2 {
        let params = harness
            .notification::<notification::PublishDiagnostics>()
            .await;
        if params.uri == api_uri {
            assert!(
                params.diagnostics.is_empty(),
                "services/api.fbs should have no diagnostics"
            );
        } else if params.uri == common_uri {
            assert!(
                params.diagnostics.is_empty(),
                "schemas/common.fbs should have no diagnostics"
            );
        } else {
            panic!(
                "Received diagnostics for unexpected file: {:?}\n{:?}",
                params.uri, params.diagnostics
            );
        }
    }
    assert_eq!(harness.call::<AllDiagnostics>(()).await.len(), 2);
}
