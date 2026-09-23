use std::io::IsTerminal;

use acp_server::{app::app, config::Config, db};
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();

    // `info` unless told otherwise: the default used to be `error`, which on
    // a server means an empty log and no way to tell it had even started.
    // Colour only on a terminal — escape codes in `docker logs` or journald
    // are noise in every line.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn,tower_http=info")),
        )
        .with_ansi(std::io::stdout().is_terminal())
        .init();

    let config = Config::from_env().expect("missing required environment variables");
    let pool = db::connect(&config.database_url)
        .await
        .expect("failed to connect to Postgres");

    sqlx::migrate!()
        .run(&pool)
        .await
        .expect("failed to run migrations");

    let state = db::AppState { db: pool };

    // The lease reaper rides the job queue, so it must be scheduled before the
    // worker starts looking for work.
    if let Err(e) = acp_server::jobs::worker::ensure_lease_sweep(&state).await {
        tracing::error!(error = %e, "could not schedule the lease sweep");
    }

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let worker = tokio::spawn(acp_server::jobs::worker::run_loop(state.clone(), shutdown_rx));

    let addr = format!("{}:{}", config.host, config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    tracing::info!("acp-server listening on {addr}");

    // One line per request — method, path, status, latency. Headers are not
    // logged, so a bearer token never reaches the log.
    axum::serve(listener, app(state).layer(TraceLayer::new_for_http()))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();

    tracing::info!("draining the job worker");
    let _ = shutdown_tx.send(true);
    let _ = worker.await;
    tracing::info!("stopped");
}

/// Ctrl-C at a terminal, or SIGTERM from `docker stop`, systemd or a deploy.
///
/// Only Ctrl-C was handled, so every deploy killed the server outright and
/// dropped whatever requests were in flight; now both let in-flight requests
/// finish and the job worker stop cleanly.
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(e) => {
                tracing::error!(error = %e, "could not listen for SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };
    tokio::select! {
        _ = ctrl_c => tracing::info!("interrupted, shutting down"),
        _ = terminate => tracing::info!("terminated, shutting down"),
    }
}
