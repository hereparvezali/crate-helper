mod crates_client;
mod popular_crates;
mod server;
mod toml_parser;

use tower_lsp::{LspService, Server};
use crate::server::Backend;

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind localhost ephemeral port for hover clicks");
    let port = listener.local_addr().unwrap().port();

    let (service, socket) = LspService::new(|client| {
        let backend = Backend::new(client.clone(), port);
        server::spawn_http_replacer(listener, client, backend.documents.clone());
        backend
    });

    Server::new(stdin, stdout, socket).serve(service).await;
}
