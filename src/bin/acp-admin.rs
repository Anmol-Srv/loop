use std::path::PathBuf;

use acp_server::{
    config::Config,
    controllers::{agent, people, token},
    db,
    models::change::Actor,
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
    /// Set a password directly, and end the person's sessions.
    ///
    /// The break-glass path, held to the app's rules: it used to accept any
    /// password with a warning, and a short one set "for now" is exactly what
    /// outlives the emergency on a shared deployment.
    SetPassword { email: String, password: String },
    /// Move a person to design, frontend or backend; their in-flight tasks
    /// follow onto the new track
    SetDepartment { email: String, department: String },
    /// Make a person a member, manager or admin; their sessions end
    SetRole { email: String, role: String },
    /// Connect an agent for someone and print its onboarding prompt once
    Mint {
        handle: String,
        #[arg(long)]
        owner: String,
        #[arg(long, default_value = "")]
        name: String,
        /// hermes, claude-code, codex or other
        #[arg(long, default_value = "other")]
        runtime: String,
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
        Command::AddPerson { email, name } => match people::add_person(&state, &email, &name).await {
            Ok(_) => println!("person ready: {}", email.trim().to_lowercase()),
            Err(e) => die(e),
        },
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
            match people::set_password_directly(&state, &email, &password).await {
                Ok(()) => println!("password set for {email}; their sessions have ended"),
                Err(e) => die(e),
            }
        }
        Command::SetDepartment { email, department } => {
            let actor = Actor { label: "acp-admin".into(), person_id: None, can_apply: true };
            let moved = match people::id_of(&state, &email).await {
                Ok(id) => people::set_department(&state, &actor, id, &department).await,
                Err(e) => Err(e),
            };
            match moved {
                Ok(p) => println!("{} is now in {}", p.email, p.department),
                Err(e) => die(e),
            }
        }
        Command::SetRole { email, role } => {
            let changed = match people::id_of(&state, &email).await {
                Ok(id) => people::set_role(&state, id, &role).await,
                Err(e) => Err(e),
            };
            match changed {
                Ok((p, ended)) => println!("{} is now {}; {ended} session(s) ended", p.email, p.role),
                Err(e) => die(e),
            }
        }
        Command::Mint { handle, owner, name, runtime } => {
            // Where the agent will reach the server; this binary has no request
            // to read it from.
            let server = std::env::var("PUBLIC_URL").unwrap_or_else(|_| "http://localhost:8080".into());
            let made = match people::id_of(&state, &owner).await {
                Ok(id) => agent::create(&state, id, &handle, &name, &runtime, &server).await,
                Err(e) => Err(e),
            };
            match made {
                Ok(m) => {
                    println!("agent '{}' for {owner}. Paste this to the agent, once:\n", m.agent.handle);
                    println!("{}", m.prompt);
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
