use acp_server::cli::client::Client;
use acp_server::cli::creds;
use clap::{Parser, Subcommand};
use serde_json::{json, Value};

#[derive(Parser)]
#[command(name = "acp", about = "Airtribe Control Plane")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Sign in with email and password
    Login {
        #[arg(long)]
        email: Option<String>,
    },
    /// First-time setup: redeem a setup code and choose a password
    Setup {
        #[arg(long)]
        email: Option<String>,
        #[arg(long)]
        code: Option<String>,
    },
    /// Revoke this session and forget the stored credential
    Logout,
    /// Who the stored credential says you are
    Whoami,
    /// Agent credentials you own
    Agent {
        #[command(subcommand)]
        action: AgentAction,
    },
    /// Administration (requires the `admin` scope)
    Admin {
        #[command(subcommand)]
        action: AdminAction,
    },
    /// Projects
    Project {
        #[command(subcommand)]
        action: ProjectAction,
    },
    /// Phases of a project
    Phase {
        #[command(subcommand)]
        action: PhaseAction,
    },
    /// Tasks
    Task {
        #[command(subcommand)]
        action: TaskAction,
    },
    /// List proposals waiting for approval
    Pending,
    /// Approve a pending proposal, replaying it
    Approve { id: String },
    /// Reject a pending proposal
    Reject { id: String },
    /// Attach a PR, doc, or link to something
    Link {
        parent_type: String,
        parent_id: String,
        #[arg(long)]
        kind: String,
        #[arg(long)]
        url: String,
        #[arg(long, default_value = "")]
        title: String,
    },
}

#[derive(Subcommand)]
enum AgentAction {
    /// Mint an agent credential. The token is printed once and never again.
    Mint {
        label: String,
        #[arg(long, default_value = "read,claim,propose")]
        scopes: String,
        #[arg(long, default_value_t = 30)]
        days: i64,
    },
    /// List the agent credentials you own
    Ls,
    /// Revoke one of your agent credentials
    Revoke { id: String },
}

#[derive(Subcommand)]
enum AdminAction {
    /// Issue a setup code for someone
    Invite { email: String },
    /// Promote or demote someone ('member' or 'admin')
    Role { email: String, role: String },
    /// Revoke a person entirely: every session and every agent they own
    Revoke { email: String },
    /// Active sessions across the team
    Sessions,
}

#[derive(Subcommand)]
enum ProjectAction {
    Ls,
    New { key: String, name: String },
}

#[derive(Subcommand)]
enum PhaseAction {
    Ls {
        project_id: String,
    },
    New {
        project_id: String,
        name: String,
        #[arg(long)]
        position: i32,
    },
}

#[derive(Subcommand)]
enum TaskAction {
    Ls {
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        phase: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        assignee: Option<String>,
    },
    New {
        phase_id: String,
        title: String,
    },
    Move {
        id: String,
        status: String,
    },
    Assign {
        id: String,
        #[arg(long)]
        to: Option<String>,
        #[arg(long)]
        agent: Option<String>,
    },
}

fn die(message: impl std::fmt::Display) -> ! {
    eprintln!("{message}");
    std::process::exit(1)
}

/// The stored credential, or a sentence telling the person what to do about it.
fn authed() -> Client {
    match creds::load() {
        Some(token) => Client::new(creds::base_url(), token),
        None => die("not signed in - run `acp login`"),
    }
}

fn prompt(label: &str) -> String {
    use std::io::Write;
    print!("{label}: ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() {
        die("could not read input");
    }
    line.trim().to_string()
}

fn secret(label: &str) -> String {
    rpassword::prompt_password(format!("{label}: ")).unwrap_or_else(|e| die(e))
}

fn ask(given: Option<String>, label: &str) -> String {
    given.unwrap_or_else(|| prompt(label))
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("-")
}

/// An RFC3339 timestamp cut down to what a person reads at a glance.
fn when(value: &Value, key: &str) -> String {
    match value.get(key).and_then(Value::as_str) {
        Some(s) if s.len() >= 16 => s[..16].replace('T', " "),
        Some(s) => s.to_string(),
        None => "-".to_string(),
    }
}

fn scopes(value: &Value) -> String {
    match value.get("scopes").and_then(Value::as_array) {
        Some(list) => list
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(","),
        None => "-".to_string(),
    }
}

/// Columns padded to their widest cell. Small n, so two passes is fine.
fn table(headers: &[&str], rows: &[Vec<String>]) {
    let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.len());
        }
    }
    let print = |cells: &[String]| {
        let line: Vec<String> = cells
            .iter()
            .zip(&widths)
            .map(|(c, w)| format!("{c:w$}"))
            .collect();
        println!("{}", line.join("  ").trim_end());
    };
    print(&headers.iter().map(|h| h.to_string()).collect::<Vec<_>>());
    for row in rows {
        print(row);
    }
}

/// Login and setup return the identical shape, so they land the same way.
fn signed_in(data: Value) {
    let token = text(&data, "token");
    if let Err(e) = creds::store(token) {
        die(e);
    }
    let me = data.get("me").cloned().unwrap_or(Value::Null);
    println!(
        "signed in as {} ({}), scopes: {}",
        text(&me, "email"),
        text(&me, "role"),
        scopes(&me)
    );
    println!("session expires {}", when(&data, "expiresAt"));
}

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();
    let cli = Cli::parse();

    // Login and setup are the two commands that must work with no credential
    // at all; everything else resolves one first and says so if there is none.
    let client = match &cli.command {
        Command::Login { .. } | Command::Setup { .. } => {
            Client::new(creds::base_url(), String::new())
        }
        Command::Logout => Client::new(creds::base_url(), creds::load().unwrap_or_default()),
        _ => authed(),
    };

    let result = match cli.command {
        Command::Login { email } => {
            let email = ask(email, "email");
            let password = secret("password");
            match client
                .send(reqwest::Method::POST, "/api/auth/login", json!({ "email": email, "password": password }))
                .await
            {
                Ok(data) => return signed_in(data),
                Err(e) => die(e),
            }
        }
        Command::Setup { email, code } => {
            let email = ask(email, "email");
            let code = ask(code, "setup code");
            let password = secret("new password");
            if secret("confirm password") != password {
                die("passwords do not match");
            }
            match client
                .send(reqwest::Method::POST, "/api/auth/setup",
                    json!({ "email": email, "code": code, "password": password }))
                .await
            {
                Ok(data) => return signed_in(data),
                Err(e) => die(e),
            }
        }
        Command::Logout => {
            // The token may already be revoked or expired; the local copy is
            // worthless either way, so clearing it is not conditional.
            match client.send(reqwest::Method::POST, "/api/auth/logout", json!({})).await {
                Ok(_) => println!("session revoked on the server"),
                Err(e) => println!("server did not revoke the session ({e}); clearing locally anyway"),
            }
            if let Err(e) = creds::clear() {
                die(e);
            }
            return println!("signed out");
        }
        Command::Whoami => match client.get("/api/user/me").await {
            Ok(me) => {
                let email = match me.get("email").and_then(Value::as_str) {
                    Some(e) => e.to_string(),
                    None => format!("{} (agent)", text(&me, "label")),
                };
                // /api/user/me does not carry `role` today; print it only if it appears.
                let role = me
                    .get("role")
                    .and_then(Value::as_str)
                    .map(|r| format!("  role: {r}"))
                    .unwrap_or_default();
                return println!("{email}{role}  scopes: {}", scopes(&me));
            }
            Err(e) => die(e),
        },
        Command::Agent { action } => match action {
            AgentAction::Mint { label, scopes: requested, days } => {
                let list: Vec<&str> = requested.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
                match client
                    .send(reqwest::Method::POST, "/api/user/agents",
                        json!({ "label": label, "scopes": list, "validDays": days }))
                    .await
                {
                    Ok(data) => {
                        let agent = data.get("agent").cloned().unwrap_or(Value::Null);
                        println!("minted '{}'  scopes: {}  expires {}",
                            text(&agent, "label"), scopes(&agent), when(&agent, "expiresAt"));
                        println!("id: {}", text(&agent, "id"));
                        println!();
                        println!("token (shown once, it is not stored anywhere you can read it again):");
                        return println!("  {}", text(&data, "token"));
                    }
                    Err(e) => die(e),
                }
            }
            AgentAction::Ls => match client.get("/api/user/agents").await {
                Ok(data) => {
                    let agents = data.as_array().cloned().unwrap_or_default();
                    if agents.is_empty() {
                        return println!("no agent credentials");
                    }
                    let rows: Vec<Vec<String>> = agents
                        .iter()
                        .map(|a| vec![
                            text(a, "label").to_string(),
                            scopes(a),
                            when(a, "expiresAt"),
                            when(a, "lastUsedAt"),
                            text(a, "id").to_string(),
                        ])
                        .collect();
                    return table(&["LABEL", "SCOPES", "EXPIRES", "LAST USED", "ID"], &rows);
                }
                Err(e) => die(e),
            },
            AgentAction::Revoke { id } => {
                match client.send(reqwest::Method::DELETE, &format!("/api/user/agents/{id}"), Value::Null).await {
                    Ok(agent) => return println!("revoked '{}'", text(&agent, "label")),
                    Err(e) => die(e),
                }
            }
        },
        Command::Admin { action } => match action {
            AdminAction::Invite { email } => {
                match client.send(reqwest::Method::POST, "/api/admin/invite", json!({ "email": email })).await {
                    Ok(data) => {
                        println!("setup code for {}:", text(&data, "email"));
                        println!();
                        println!("    {}", text(&data, "code"));
                        println!();
                        return println!("valid 48h, single use. Pass it on out of band; it is not stored in the clear.");
                    }
                    Err(e) => die(e),
                }
            }
            AdminAction::Role { email, role } => {
                match client.send(reqwest::Method::POST, "/api/admin/role", json!({ "email": email, "role": role })).await {
                    Ok(data) => {
                        let ended = data.get("sessionsEnded").and_then(Value::as_u64).unwrap_or(0);
                        return println!("{} is now {}; {ended} session(s) ended", text(&data, "email"), text(&data, "role"));
                    }
                    Err(e) => die(e),
                }
            }
            AdminAction::Revoke { email } => {
                match client.send(reqwest::Method::POST, "/api/admin/revoke", json!({ "email": email })).await {
                    Ok(data) => {
                        let n = data.get("credentialsRevoked").and_then(Value::as_u64).unwrap_or(0);
                        return println!("revoked {n} credential(s) for {}", text(&data, "email"));
                    }
                    Err(e) => die(e),
                }
            }
            AdminAction::Sessions => match client.get("/api/admin/sessions").await {
                Ok(data) => {
                    let sessions = data.as_array().cloned().unwrap_or_default();
                    if sessions.is_empty() {
                        return println!("no active sessions");
                    }
                    let rows: Vec<Vec<String>> = sessions
                        .iter()
                        .map(|s| vec![
                            text(s, "email").to_string(),
                            when(s, "createdAt"),
                            when(s, "lastUsedAt"),
                            when(s, "expiresAt"),
                        ])
                        .collect();
                    return table(&["EMAIL", "STARTED", "LAST USED", "EXPIRES"], &rows);
                }
                Err(e) => die(e),
            },
        },
        Command::Project { action } => match action {
            ProjectAction::Ls => client.get("/api/user/projects").await,
            ProjectAction::New { key, name } => {
                client.send(reqwest::Method::POST, "/api/user/projects", json!({ "key": key, "name": name })).await
            }
        },
        Command::Phase { action } => match action {
            PhaseAction::Ls { project_id } => {
                client.get(&format!("/api/user/projects/{project_id}/phases")).await
            }
            PhaseAction::New { project_id, name, position } => {
                client.send(reqwest::Method::POST, &format!("/api/user/projects/{project_id}/phases"),
                    json!({ "name": name, "position": position })).await
            }
        },
        Command::Task { action } => match action {
            TaskAction::Ls { project, phase, status, assignee } => {
                let mut query = Vec::new();
                if let Some(v) = project { query.push(format!("projectId={v}")); }
                if let Some(v) = phase { query.push(format!("phaseId={v}")); }
                if let Some(v) = status { query.push(format!("status={v}")); }
                if let Some(v) = assignee { query.push(format!("assigneeEmail={v}")); }
                let suffix = if query.is_empty() { String::new() } else { format!("?{}", query.join("&")) };
                client.get(&format!("/api/user/tasks{suffix}")).await
            }
            TaskAction::New { phase_id, title } => {
                client.send(reqwest::Method::POST, &format!("/api/user/phases/{phase_id}/tasks"),
                    json!({ "title": title })).await
            }
            TaskAction::Move { id, status } => {
                client.send(reqwest::Method::PATCH, &format!("/api/user/tasks/{id}"),
                    json!({ "status": status })).await
            }
            TaskAction::Assign { id, to, agent } => {
                client.send(reqwest::Method::POST, &format!("/api/user/tasks/{id}/assign"),
                    json!({ "personEmail": to, "agentLabel": agent })).await
            }
        },
        Command::Pending => client.get("/api/user/changes/pending").await,
        Command::Approve { id } => {
            client.send(reqwest::Method::POST, &format!("/api/user/changes/{id}/approve"), json!({})).await
        }
        Command::Reject { id } => {
            client.send(reqwest::Method::POST, &format!("/api/user/changes/{id}/reject"), json!({})).await
        }
        Command::Link { parent_type, parent_id, kind, url, title } => {
            client.send(reqwest::Method::POST, "/api/user/artifacts", json!({
                "parentType": parent_type, "parentId": parent_id,
                "kind": kind, "url": url, "title": title
            })).await
        }
    };

    match result {
        // Mutations come back wrapped in an Outcome; unwrap it so the CLI
        // prints the entity, or says plainly that a human has to approve.
        Ok(data) => match data.get("status").and_then(serde_json::Value::as_str) {
            Some("applied") => {
                let entity = data.get("entity").cloned().unwrap_or(data);
                println!("{}", serde_json::to_string_pretty(&entity).unwrap());
            }
            Some("proposed") => {
                let id = data.get("changeId").and_then(serde_json::Value::as_str).unwrap_or("?");
                println!("proposed: {id} (awaiting approval)");
            }
            _ => println!("{}", serde_json::to_string_pretty(&data).unwrap()),
        },
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
