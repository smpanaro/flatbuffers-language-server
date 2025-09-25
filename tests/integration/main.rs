use harness::TestHarness;

mod code_action;
mod completion;
mod diagnostics;
mod harness;
mod helpers;
mod hover;
mod references;
mod scenarios;
mod test_logger;

#[tokio::test]
async fn initialize_server_test() {
    let mut harness = TestHarness::new();
    harness.initialize_and_open(&[]).await;
}
