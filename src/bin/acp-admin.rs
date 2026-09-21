use std::path::PathBuf;

use acp_server::{
    config::Config,
    controllers::{people, token},
    db,
};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "acp-admin", about = "Bootstrap administration for the control plane")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Add a person who can own credentials
    AddPerson { email: String, name: String },
    /// Mint a session for a person, bypassing the password. The §7 escape
    /// hatch: for scripts, and for the day the password path is broken.
    Session { email: String },
    /// Set a password directly, bypassing the length policy.
    ///
    /// The break-glass path: `acp-admin` already talks straight to the
    /// database, and a local instance sometimes needs a password you can type
    /// quickly. It warns when the password would not survive the real rules,
    /// so nobody sets one of these on a shared deployment by accident.
    SetPassword { email: String, password: String },
    /// Mint an agent credential and print it once
    Mint {
        label: String,
        #[arg(long)]
        owner: String,
        #[arg(long, value_delimiter = ',', default_value = "read")]
        scopes: Vec<String>,
        #[arg(long, default_value_t = 30)]
        days: i64,
    },
    /// Create or promote the first admin and print a setup code
    BootstrapAdmin {
        email: String,
        name: String,
        /// Add another admin even though one already exists
        #[arg(long)]
        force: bool,
    },
    /// Create people from a file of `email,Name` lines
    SeedTeam { path: PathBuf },
    /// Issue a setup code, voiding any code the person still holds
    Invite { email: String },
    /// Revoke every session and agent a person owns, and the person
    RevokePerson { email: String },
}

fn die(e: impl std::fmt::Display) -> ! {
    eprintln!("{e}");
    std::process::exit(1);
}

fn print_code(email: &str, code: &str) {
    println!("setup code for {email} (valid 48h, single use):\n\n  {code}\n");
    println!("Hand this over out of band. Issuing another code voids this one.");
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
        Command::Session { email } => {
            match token::mint_session(&state, &email).await {
                Ok((raw, row)) => {
                    println!("session for {} (expires {})", row.label, row.expires_at);
                    println!("{raw}");
                }
                Err(e) => {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
            }
        }
        Command::SetPassword { email, password } => {
            let weak = password.chars().count() < 12;
            let hash = match acp_server::models::password::hash(&password) {
                Ok(h) => h,
                Err(e) => {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
            };
            let updated = sqlx::query(
                "UPDATE person
                    SET password_hash = $2, password_set_at = now(),
                        failed_attempts = 0, locked_until = NULL
                  WHERE email = $1 AND deleted_at IS NULL",
            )
            .bind(&email)
            .bind(&hash)
            .execute(&state.db)
            .await
            .expect("update failed");

            if updated.rows_affected() == 0 {
                eprintln!("no person with email '{email}'");
                std::process::exit(1);
            }
            println!("password set for {email}");
            if weak {
                println!(
                    "  warning: {} characters. The API enforces a 12-character \n  minimum, so this password could not be set through the app.",
                    password.chars().count()
                );
            }
        }
        Command::Mint { label, owner, scopes, days } => {
            match token::mint_agent(&state, &label, &owner, scopes, days).await {
                Ok((raw, row)) => {
                    println!("agent credential '{}' (expires {})", row.label, row.expires_at);
                    println!("{raw}");
                    println!("\nThis is shown once. Export it:\n  export ACP_TOKEN={raw}");
                }
                Err(e) => die(e),
            }
        }
        Command::BootstrapAdmin { email, name, force } => {
            match people::bootstrap_admin(&state, &email, &name, force).await {
                Ok(code) => {
                    println!("{email} is an admin.");
                    print_code(&email, &code);
                }
                Err(e) => die(e),
            }
        }
        Command::SeedTeam { path } => match people::seed_team(&state, &path).await {
            Ok(report) => {
                for line in &report {
                    println!("{line}");
                }
                println!("\n{} line(s) processed", report.len());
            }
            Err(e) => die(e),
        },
        Command::Invite { email } => match people::invite(&state, &email).await {
            Ok(code) => print_code(&email, &code),
            Err(e) => die(e),
        },
        Command::RevokePerson { email } => match people::revoke_person(&state, &email).await {
            Ok(n) => println!("{email} revoked; {n} credential(s) ended"),
            Err(e) => die(e),
        },
    }
}
