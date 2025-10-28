use harness::TestHarness;

mod analyzer;
mod code_action;
mod completion;
mod diagnostics;
mod harness;
mod helpers;
mod hover;
mod include_paths;
mod references;
mod rename;
mod scenarios;
mod test_logger;
mod workspace;
mod workspace_layout;

#[tokio::test]
async fn initialize_server_test() {
    let mut harness = TestHarness::new();
    harness.initialize_and_open(&[]).await;
}
