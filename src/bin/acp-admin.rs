use acp_server::{config::Config, controllers::token, db};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "acp-admin", about = "Bootstrap administration for the control plane")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Add a person who can own tokens
    AddPerson { email: String, name: String },
    /// Mint a token and print it once
    Mint {
        label: String,
        #[arg(long)]
        owner: String,
        #[arg(long, value_delimiter = ',', default_value = "read")]
        scopes: Vec<String>,
        #[arg(long, default_value_t = 30)]
        days: i64,
    },
}

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();
    let cli = Cli::parse();

    let config = Config::from_env().expect("missing DATABASE_URL");
    let pool = db::connect(&config.database_url).await.expect("cannot connect to Postgres");
    let state = db::AppState { db: pool };

    match cli.command {
        Command::AddPerson { email, name } => {
            sqlx::query("INSERT INTO person (email, name) VALUES ($1, $2) ON CONFLICT (email) DO NOTHING")
                .bind(&email)
                .bind(&name)
                .execute(&state.db)
                .await
                .expect("insert failed");
            println!("person ready: {email}");
        }
        Command::Mint { label, owner, scopes, days } => {
            match token::mint(&state, &label, &owner, scopes, days).await {
                Ok((raw, row)) => {
                    println!("token for '{}' (expires {})", row.label, row.expires_at);
                    println!("{raw}");
                    println!("\nThis is shown once. Export it:\n  export ACP_TOKEN={raw}");
                }
                Err(e) => {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
            }
        }
    }
}
