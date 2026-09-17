mod mcp_bridge;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().nth(1).as_deref() == Some("mcp-bridge") {
        std::process::exit(mcp_bridge::run().await);
    }
    webcodex_cli::run().await
}
