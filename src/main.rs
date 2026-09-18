use acp_server::{app::app, config::Config};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let config = Config::from_env().expect("missing required environment variables");
    let addr = format!("0.0.0.0:{}", config.port);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    tracing::info!("acp-server listening on {addr}");

    axum::serve(listener, app()).await.unwrap();
}
