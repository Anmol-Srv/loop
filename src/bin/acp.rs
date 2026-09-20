use acp_server::cli::client::Client;
use clap::{Parser, Subcommand};
use serde_json::json;

#[derive(Parser)]
#[command(name = "acp", about = "Airtribe Control Plane")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
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

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();
    let cli = Cli::parse();

    let client = match Client::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };

    let result = match cli.command {
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
        Command::Link { parent_type, parent_id, kind, url, title } => {
            client.send(reqwest::Method::POST, "/api/user/artifacts", json!({
                "parentType": parent_type, "parentId": parent_id,
                "kind": kind, "url": url, "title": title
            })).await
        }
    };

    match result {
        Ok(data) => println!("{}", serde_json::to_string_pretty(&data).unwrap()),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
