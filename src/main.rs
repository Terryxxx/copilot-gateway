use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tokio::net::TcpListener;

use copilot_gateway::auth;
use copilot_gateway::config;
use copilot_gateway::copilot;
use copilot_gateway::github::copilot_token::{fetch_copilot_token, spawn_refresh_task};
use copilot_gateway::server;
use copilot_gateway::state::AppState;

#[derive(Parser)]
#[command(
    name = "copilot-gateway",
    about = "Expose GitHub Copilot models through an Anthropic-compatible API for Claude Code."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Authenticate with GitHub via the device authorization flow.
    Auth,
    /// Start the Anthropic-compatible gateway server.
    Start {
        /// Port to listen on.
        #[arg(long, default_value_t = 4141)]
        port: u16,
        /// Address to bind to.
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Account type: "individual", or an org/enterprise slug.
        #[arg(long, default_value = "individual")]
        account_type: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();
    let http = build_http_client()?;

    match cli.command {
        Command::Auth => {
            auth::run_device_flow(&http).await?;
        }
        Command::Start {
            port,
            host,
            account_type,
        } => {
            start(http, port, host, account_type).await?;
        }
    }

    Ok(())
}

fn build_http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .build()
        .context("failed to build HTTP client")
}

async fn start(
    http: reqwest::Client,
    port: u16,
    host: String,
    account_type: String,
) -> Result<()> {
    // Ensure we have a GitHub token, running the device flow if needed.
    let github_token = match config::load_github_token()? {
        Some(token) => token,
        None => {
            tracing::info!("No saved GitHub token found; starting device authorization.");
            auth::run_device_flow(&http).await?
        }
    };

    // Exchange for a Copilot token and start the refresh loop.
    let token_resp = fetch_copilot_token(&http, &github_token)
        .await
        .context("failed to obtain Copilot token (try re-running `auth`)")?;

    let state = AppState::new(http.clone(), account_type);
    *state.copilot_token.write().await = token_resp.token.clone();
    spawn_refresh_task(
        http,
        github_token,
        state.copilot_token.clone(),
        token_resp.refresh_in,
    );

    // Warm the model cache (non-fatal).
    match copilot::models::fetch_models(&state).await {
        Ok(models) => {
            tracing::info!("Loaded {} models", models.data.len());
            *state.models.write().await = Some(models);
        }
        Err(err) => tracing::warn!("Could not preload models: {err}"),
    }

    let router = server::build_router(state);
    let addr = format!("{host}:{port}");
    let listener = TcpListener::bind(&addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;

    println!("copilot-gateway listening on http://{addr}");
    println!("Point Claude Code at it: set ANTHROPIC_BASE_URL=http://{addr}");

    axum::serve(listener, router)
        .await
        .context("server error")?;

    Ok(())
}
