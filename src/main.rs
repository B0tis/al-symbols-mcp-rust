mod app_parser;
mod database;
mod manifest;
mod package_manager;
mod server;
mod symbol_parser;
mod types;

use rmcp::ServiceExt;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    eprintln!("AL Symbols MCP Server starting (Rust)...");

    let server = server::AlMcpServer::new();

    let service = server
        .serve(rmcp::transport::io::stdio())
        .await
        .inspect_err(|e| {
            eprintln!("Failed to start server: {}", e);
        })?;

    eprintln!("AL Symbols MCP Server started successfully (packages will be auto-loaded on first use)");

    service.waiting().await?;

    Ok(())
}
