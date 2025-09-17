use harness::TestHarness;

mod diagnostics;
mod harness;
mod helpers;
mod hover;
mod scenarios;

#[tokio::test]
async fn initialize_server_test() {
    let mut harness = TestHarness::new();
    harness.initialize_and_open(&[]).await;
}
