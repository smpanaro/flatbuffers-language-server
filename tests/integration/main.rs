mod harness;

use harness::TestHarness;

#[tokio::test]
async fn initialize_server_test() {
    let _harness = TestHarness::new().await;
    // The fact that `new()` doesn't panic means initialization was successful.
}
