//! `dekopon-console` — an interactive terminal view over a running Dekopon broker.
//!
//! This binary owns exactly one decision the library cannot make for it: what a command line means.
//! Everything after that — connecting, authenticating, drawing, running turns — is `dekopon-tui`.
//!
//! It is an ordinary unprivileged client of `dekopon-brokerd`'s Unix socket. It holds a model
//! credential in turn mode and nothing else: no policy, no provider credential, no authorization. Every
//! capability call it makes is a proposal the broker alone decides.

#![forbid(unsafe_code)]

use std::{
    error::Error as _,
    io::{self, IsTerminal as _},
    path::PathBuf,
    process::ExitCode,
};

use clap::Parser;
use dekopon_config::load_discovered;
use dekopon_core::ExternalSubject;
use dekopon_tui::{
    App, ConsoleOptions, ModelChoice,
    session::{TRACE_PREFIX, connect, resolve_console_credential},
};
use thiserror::Error;
use tracing_subscriber::EnvFilter;

/// Connection, identity, and model settings for the console.
///
/// Every one of these has a resolved default except the subject, so `dekopon-console` with no flags
/// is the ordinary invocation; each flag exists for the deployment that is not the local
/// single-UID one.
#[derive(Clone, Debug, Parser)]
#[command(
    name = "dekopon-console",
    version,
    about = "Interactive terminal console for a running Dekopon broker",
    long_about = None
)]
struct Cli {
    /// Use the existing broker shell without any model or credential setup. Turns stay disabled.
    #[arg(long, conflicts_with_all = ["chat_socket", "auth_file", "endpoint", "api_key_env", "model"])]
    shell: bool,

    /// Development-only dekopond local socket (0600). Skips catalog and model setup.
    #[arg(long, value_name = "PATH", conflicts_with_all = ["socket", "server_uid", "config", "auth_file", "endpoint", "api_key_env"])]
    chat_socket: Option<PathBuf>,

    /// Local chat conversation identity; reuse deliberately to resume gateway history.
    #[arg(long, requires = "chat_socket", default_value = "dev")]
    conversation: String,

    /// Path to a YAML or JSON agent catalog.
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,

    /// Broker socket path.
    ///
    /// Resolves as `dekopon-run` documents: this flag, then `$DEKOPON_BROKER_SOCKET`, then
    /// `$XDG_RUNTIME_DIR/dekopon/broker.sock`, then `$HOME/.local/run/dekopon/broker.sock`.
    #[arg(long, value_name = "PATH")]
    socket: Option<PathBuf>,

    /// Trusted UID owning the broker process; defaults to the caller's own.
    #[arg(long, value_name = "UID")]
    server_uid: Option<u32>,

    /// Canonical external subject sessions propose on behalf of.
    ///
    /// The broker still has to hold an attestor grant covering its namespace and a mapping
    /// resolving it to a principal; declaring one here grants nothing at all.
    ///
    /// Optional only so it can come from the environment. There is no default: an identity the
    /// console guessed would be an identity nobody chose, and the broker would refuse it anyway one
    /// step later having told you nothing useful.
    #[arg(long, value_name = "SUBJECT", env = "DEKOPON_CONSOLE_SUBJECT")]
    subject: Option<ExternalSubject>,

    /// Model name handed to the backend.
    #[arg(long, value_name = "MODEL", default_value = DEFAULT_MODEL)]
    model: String,

    /// ChatGPT credential file.
    ///
    /// Defaults to the console's own `chatgpt-auth.console.json` rather than the file every other
    /// surface resolves to, because the refresh token rotates and sharing it would invalidate the
    /// gateway's copy. Passing this explicitly accepts whatever it points at.
    #[arg(long, value_name = "PATH", conflicts_with = "endpoint")]
    auth_file: Option<PathBuf>,

    /// OpenAI-compatible endpoint to use instead of the ChatGPT subscription.
    #[arg(long, value_name = "URL")]
    endpoint: Option<String>,

    /// Name of the environment variable holding the endpoint's bearer token.
    #[arg(long, value_name = "NAME", requires = "endpoint")]
    api_key_env: Option<String>,

    /// Maximum model turns one session may take.
    #[arg(long, value_name = "COUNT", default_value_t = 8)]
    max_steps: u32,

    /// Capability invocations one session may drive in total.
    #[arg(long, value_name = "COUNT", default_value_t = 16)]
    max_capability_calls: u32,

    /// Disable ANSI colors in diagnostics.
    #[arg(long)]
    no_color: bool,

    /// Increase diagnostics (`-v` for info, `-vv` for debug details).
    #[arg(short = 'v', action = clap::ArgAction::Count)]
    verbose: u8,
}

/// Model a console session talks to when nothing names one.
const DEFAULT_MODEL: &str = "gpt-5.6-luna";

/// Failure opening or running the console.
#[derive(Debug, Error)]
enum ConsoleError {
    /// The development transport refused the connection.
    #[error(transparent)]
    Chat(#[from] dekopon_tui::chat::ChatError),
    /// The catalog would not load.
    #[error(transparent)]
    Config(Box<dekopon_config::ConfigError>),
    /// Connecting, authenticating, or choosing a model failed.
    #[error(transparent)]
    Session(#[from] dekopon_tui::SessionError),
    /// The console ran and the terminal failed under it.
    #[error(transparent)]
    Exit(#[from] dekopon_tui::ConsoleExit),
    /// The async runtime could not be built.
    #[error("could not start the console runtime")]
    Runtime(#[source] io::Error),
    /// No subject was supplied.
    #[error(
        "no console subject: pass --subject <SUBJECT> or set DEKOPON_CONSOLE_SUBJECT, for example \
         dev.console.{}. The broker must also set allowDevelopmentSubjects, hold an attestor grant \
         covering that namespace, and map the subject to a principal",
        whoami()
    )]
    NoSubject,
}

/// A plausible name for the example in the missing-subject refusal.
///
/// Cosmetic: an operator reading `dev.console.<their own name>` copies it, where a placeholder is
/// one more thing to work out. It never becomes a default, because an identity nobody chose is an
/// identity the broker would refuse having explained nothing.
fn whoami() -> String {
    std::env::var("USER")
        .ok()
        .filter(|user| !user.is_empty() && user.chars().all(|c| c.is_ascii_alphanumeric()))
        .map_or_else(|| "you".to_owned(), |user| user.to_ascii_lowercase())
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    initialize_tracing(cli.verbose, cli.no_color);
    match execute(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            if cli.verbose > 0 {
                let mut source = error.source();
                while let Some(cause) = source {
                    eprintln!("  caused by: {cause}");
                    source = cause.source();
                }
            }
            ExitCode::FAILURE
        }
    }
}

/// Opens the console.
///
/// Returns before drawing anything when the catalog will not load, no broker is listening, or the
/// credential guard refuses. Once the console is drawing, only a terminal failure comes back here:
/// a refused hop or a failed session is something the console shows and stays open after.
fn execute(cli: &Cli) -> Result<(), ConsoleError> {
    // Command-line facts first, filesystem facts second. A missing subject is not something an
    // operator fixes by finding a catalog, so reporting the catalog's absence ahead of it would
    // send them to the wrong problem.
    let subject = cli.subject.clone().ok_or(ConsoleError::NoSubject)?;
    if let Some(socket) = &cli.chat_socket {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(ConsoleError::Runtime)?;
        let client = runtime.block_on(dekopon_tui::chat::ChatClient::connect(
            socket,
            subject.clone(),
            cli.conversation.clone(),
        ))?;
        runtime.block_on(dekopon_tui::run::run_chat(
            client,
            subject.to_string(),
            cli.conversation.clone(),
        ))?;
        return Ok(());
    }
    let catalog = load_discovered(cli.config.clone())
        .map_err(|error| ConsoleError::Config(Box::new(error)))?;
    let agents = catalog.agents().cloned().collect();

    let mut options = ConsoleOptions::new(subject.clone(), cli.model.clone());
    options.catalog = cli.config.clone();
    options.socket = cli.socket.clone();
    options.server_uid = cli.server_uid;
    options.prompt_limits.max_steps = cli.max_steps;
    options.prompt_limits.max_capability_calls = cli.max_capability_calls;
    options.model_choice = if cli.shell {
        ModelChoice::ShellOnly
    } else {
        match &cli.endpoint {
            Some(endpoint) => ModelChoice::OpenAiCompatible {
                endpoint: endpoint.clone(),
                api_key_env: cli.api_key_env.clone(),
            },
            None => ModelChoice::ChatGptSubscription {
                auth_file: cli.auth_file.clone(),
            },
        }
    };

    // Resolved before the screen opens, so the refusal an operator has to act on arrives as a line
    // on their terminal rather than inside a full-screen frame they then have to quit out of.
    let credential = match &options.model_choice {
        ModelChoice::ShellOnly => String::new(),
        ModelChoice::ChatGptSubscription { auth_file } => {
            resolve_console_credential(auth_file.as_deref())?
                .display()
                .to_string()
        }
        ModelChoice::OpenAiCompatible { endpoint, .. } => endpoint.clone(),
    };

    // The runtime comes up before the screen does, because connecting proves a broker is actually
    // answering and that refusal has to reach a plain terminal rather than a frame the operator
    // then has to quit out of.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(ConsoleError::Runtime)?;

    let (client, socket) = runtime.block_on(connect(&options))?;
    tracing::debug!(
        trace_prefix = TRACE_PREFIX,
        socket_tier = socket.tier().label(),
        "opening the console",
    );

    let app = App::new(
        agents,
        subject.to_string(),
        socket.path().display().to_string(),
        credential,
    );
    runtime.block_on(dekopon_tui::run(app, client, options))?;
    Ok(())
}

/// Sends diagnostics to standard error, or to a sink when that is the screen this process is about
/// to take over.
///
/// The alternate screen *is* the terminal, so a `tracing` line written to a terminal standard error
/// lands inside a frame and stays until something overdraws it — and the operator's next action is
/// then against a display that is no longer true. Redirecting standard error keeps every
/// diagnostic and costs nothing: `dekopon-console -vv 2> console.log` works exactly as it reads.
fn initialize_tracing(verbosity: u8, no_color: bool) {
    let level = match verbosity {
        0 => "warn",
        1 => "info",
        _ => "debug",
    };
    let builder = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(level))
        .with_ansi(!no_color)
        .with_target(verbosity > 1)
        .without_time();
    let _subscriber_result = if io::stderr().is_terminal() {
        builder.with_writer(io::sink).try_init()
    } else {
        builder.with_writer(io::stderr).try_init()
    };
}
