use acp_server::{app::app, config::Config, db};

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt::init();

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

    let addr = format!("0.0.0.0:{}", config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    tracing::info!("acp-server listening on {addr}");

    axum::serve(listener, app(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .unwrap();

    let _ = shutdown_tx.send(true);
    let _ = worker.await;
}
