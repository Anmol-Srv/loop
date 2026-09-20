//! MCP stdio shim.
//!
//! Hermes and Claude Code attach to a local command far more easily than to an
//! authenticated remote endpoint. This process holds the token so the agent
//! never sees it, and speaks newline-delimited JSON-RPC on stdin and stdout.

use acp_server::cli::{client::Client, shim::forward};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();

    let client = match Client::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };

    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();

    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }
        let response = forward(&client, &line).await;
        // Flush per line: the client blocks until it sees the response.
        let _ = stdout.write_all(response.as_bytes()).await;
        let _ = stdout.write_all(b"\n").await;
        let _ = stdout.flush().await;
    }
}
