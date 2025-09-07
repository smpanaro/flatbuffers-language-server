use flatbuffers_language_server::server::Backend;
use tower_lsp::lsp_types::{DidOpenTextDocumentParams, InitializeParams, TextDocumentItem, Url};
use tower_lsp::{LanguageServer, LspService};

pub struct TestHarness {
    pub backend: Backend,
    _service: LspService<Backend>,
}

impl TestHarness {
    pub async fn new() -> Self {
        let (service, _) = LspService::new(Backend::new);

        let backend = service.inner().clone();

        let mut harness = Self {
            backend,
            _service: service,
        };
        harness.initialize().await;
        harness
    }

    async fn initialize(&mut self) {
        self.backend
            .initialize(InitializeParams::default())
            .await
            .expect("initialize failed");
    }

    #[allow(dead_code)]
    pub async fn open_workspace(&self, workspace: &[(&str, &str)]) {
        for (name, content) in workspace {
            let uri = Url::from_file_path(format!("/{}", name)).unwrap();
            self.backend
                .did_open(DidOpenTextDocumentParams {
                    text_document: TextDocumentItem::new(
                        uri,
                        "flatbuffers".to_string(),
                        1,
                        (*content).to_string(),
                    ),
                })
                .await;
        }
    }
}
