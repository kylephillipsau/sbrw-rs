use std::io::{IsTerminal, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use clap::{Args, Parser, Subcommand};
use sbrw_common::{CaptureSink, CaptureStream, Config};
use tokio::net::{TcpListener, UdpSocket};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(version, about = "A single-binary Need for Speed World server compatible with the SBRW launcher")]
struct Cli {
    /// Config file [default: <data dir>/sbrw.toml]
    #[arg(long, global = true, env = sbrw_common::config::CONFIG_PATH_ENV)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the server
    Serve(ServeArgs),
    /// Exit 0 if the local server answers /health (used by the Docker HEALTHCHECK)
    Healthcheck,
    /// Print the effective configuration as TOML
    Config,
}

#[derive(Args)]
struct ServeArgs {
    /// Accept XMPP connections and log what clients send, without answering
    #[arg(long)]
    probe_xmpp: bool,
    /// Bind the free-roam and race UDP ports and log datagrams, without answering
    #[arg(long)]
    probe_udp: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    init_tracing();
    let config = match Config::load(cli.config.as_deref()) {
        Ok(config) => config,
        Err(err) => {
            eprintln!("sbrw: invalid configuration: {err}");
            return ExitCode::FAILURE;
        }
    };
    match cli.command {
        Command::Serve(args) => {
            let runtime = tokio::runtime::Runtime::new().expect("failed to start the tokio runtime");
            match runtime.block_on(serve(config, args)) {
                Ok(()) => ExitCode::SUCCESS,
                Err(err) => {
                    tracing::error!("{err:#}");
                    ExitCode::FAILURE
                }
            }
        }
        Command::Healthcheck => healthcheck(&config),
        Command::Config => match toml::to_string_pretty(&config) {
            Ok(text) => {
                print!("{text}");
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("sbrw: {err}");
                ExitCode::FAILURE
            }
        },
    }
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).with_ansi(std::io::stdout().is_terminal()).init();
}

async fn serve(config: Config, args: ServeArgs) -> anyhow::Result<()> {
    for warning in config.warnings() {
        tracing::warn!("{warning}");
    }
    std::fs::create_dir_all(&config.server.data_dir)
        .with_context(|| format!("creating data directory {}", config.server.data_dir.display()))?;
    let capture = CaptureSink::start(&config.capture_dir(), &config.capture.streams)
        .with_context(|| format!("starting capture in {}", config.capture_dir().display()))?;
    if let Some(dir) = capture.run_dir() {
        tracing::info!(dir = %dir.display(), streams = ?config.capture.streams, "capturing protocol traffic");
    }

    let config = Arc::new(config);
    let shutdown = CancellationToken::new();
    let mut tasks: JoinSet<std::io::Result<()>> = JoinSet::new();

    let http =
        TcpListener::bind(config.http.bind).await.with_context(|| format!("binding HTTP to {}", config.http.bind))?;
    tracing::info!(
        bind = %config.http.bind,
        launcher_url = %format!("{}/Engine.svc", config.public_http_url()),
        "HTTP listening; add launcher_url as a server in the SBRW launcher"
    );
    let state = sbrw_http::AppState::new(config.clone(), capture.clone());
    tasks.spawn(sbrw_http::serve(http, state, shutdown.clone()));

    if args.probe_xmpp {
        let xmpp = TcpListener::bind(config.xmpp.bind)
            .await
            .with_context(|| format!("binding XMPP to {}", config.xmpp.bind))?;
        tracing::info!(bind = %config.xmpp.bind, "XMPP probe listening");
        tasks.spawn(sbrw_xmpp::run_probe(xmpp, capture.clone(), shutdown.clone()));
    }
    if args.probe_udp {
        for (stream, bind) in
            [(CaptureStream::UdpFreeroam, config.udp.freeroam.bind), (CaptureStream::UdpRace, config.udp.race.bind)]
        {
            let socket = UdpSocket::bind(bind).await.with_context(|| format!("binding {} to {bind}", stream.name()))?;
            tracing::info!(%bind, server = stream.name(), "UDP probe listening");
            tasks.spawn(sbrw_udp::run_probe(stream, socket, capture.clone(), shutdown.clone()));
        }
    }

    let early_exit = tokio::select! {
        () = shutdown_signal() => {
            tracing::info!("shutting down");
            None
        }
        Some(result) = tasks.join_next() => Some(result),
    };
    shutdown.cancel();
    while let Some(result) = tasks.join_next().await {
        result??;
    }
    match early_exit {
        Some(result) => {
            result??;
            anyhow::bail!("a listener stopped unexpectedly")
        }
        None => Ok(()),
    }
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install the SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}

/// Plain blocking HTTP/1.1 so the healthcheck needs no runtime or extra crates.
fn healthcheck(config: &Config) -> ExitCode {
    let ip = match config.http.bind.ip() {
        ip if ip.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        ip => ip,
    };
    let addr = SocketAddr::new(ip, config.http.bind.port());
    let check = || -> std::io::Result<bool> {
        let timeout = Duration::from_secs(3);
        let mut stream = TcpStream::connect_timeout(&addr, timeout)?;
        stream.set_read_timeout(Some(timeout))?;
        stream.write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;
        let mut response = String::new();
        stream.read_to_string(&mut response)?;
        Ok(response.starts_with("HTTP/1.1 200"))
    };
    match check() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => {
            eprintln!("sbrw: {addr}/health did not answer 200");
            ExitCode::FAILURE
        }
        Err(err) => {
            eprintln!("sbrw: healthcheck against {addr} failed: {err}");
            ExitCode::FAILURE
        }
    }
}
