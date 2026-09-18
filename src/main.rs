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
    let addr = format!("0.0.0.0:{}", config.port);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    tracing::info!("acp-server listening on {addr}");

    axum::serve(listener, app(state)).await.unwrap();
}
