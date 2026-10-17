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
    /// Accept XMPP connections and log what clients send, without answering,
    /// instead of running the XMPP server
    #[arg(long)]
    probe_xmpp: bool,
    /// Bind the free-roam and race UDP ports and log datagrams, without
    /// answering, instead of running the free-roam server
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

    // The core's pushes go straight to the XMPP hub, as upstream's went to Openfire.
    let (pushes, mut push_queue) = tokio::sync::mpsc::unbounded_channel();
    let mut openfire = sbrw_core::openfire::Openfire::default();
    let xmpp_accounts = openfire.accounts();
    let group_members = openfire.group_members.clone();
    let relay = sbrw_common::RelayRegistry::default();
    if !args.probe_xmpp {
        openfire.outbound = Some(pushes);
    }
    let world = sbrw_core::World::boot(sbrw_core::WorldOptions {
        db_path: Some(config.server.data_dir.join("world.db")),
        parameters: config.upstream_parameters(),
        env: sbrw_common::Env::system(),
        effects: sbrw_common::EffectLog::disabled(),
        openfire,
        build_info: sbrw_core::BuildInfo::of_this_build(),
        relay: relay.clone(),
    })
    .context("starting the world")?;
    let world = sbrw_core::WorldHandle::spawn(world);
    let rooms = world.call(|w| Ok(sbrw_core::dao::chat_room::find_all(w.db())?)).await.context("reading chat rooms")?;

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
    tasks.spawn(run_jobs(world.clone(), shutdown.clone()));
    tasks.spawn(run_timers(world.clone(), shutdown.clone()));
    let state = sbrw_http::AppState::new(config.clone(), capture.clone(), world);
    tasks.spawn(sbrw_http::serve(http, state, shutdown.clone()));

    if args.probe_xmpp {
        let xmpp = TcpListener::bind(config.xmpp.bind)
            .await
            .with_context(|| format!("binding XMPP to {}", config.xmpp.bind))?;
        tracing::info!(bind = %config.xmpp.bind, "XMPP probe listening");
        tasks.spawn(sbrw_xmpp::run_probe(xmpp, capture.clone(), shutdown.clone()));
    } else {
        let xmpp = TcpListener::bind(config.xmpp.bind)
            .await
            .with_context(|| format!("binding XMPP to {}", config.xmpp.bind))?;
        let domain = &config.server.public_host;
        let mut hub = sbrw_xmpp::hub::Hub::new(
            domain,
            Box::new(|| uuid::Uuid::new_v4().to_string()),
            Box::new(time::OffsetDateTime::now_utc),
        );
        hub.share_groups(group_members);
        // As OpenFireRestApiCli.init creates them.
        for room in rooms {
            if let (Some(short_name), Some(amount)) = (room.short_name, room.amount) {
                for n in 1..=amount {
                    hub.create_public_room(&format!("channel.{short_name}__{n}"));
                }
            }
        }
        let tls_dir = config.server.data_dir.join("tls");
        let identity = sbrw_xmpp::tls::load_or_create(&tls_dir, domain)
            .with_context(|| format!("loading or making the XMPP certificate in {}", tls_dir.display()))?;
        let acceptor =
            sbrw_xmpp::tls::acceptor(&identity, sbrw_xmpp::tls::DEFAULT_CIPHERS).context("setting up TLS")?;
        let server =
            Arc::new(sbrw_xmpp::server::Server::new(domain, acceptor, Arc::new(WorldAccounts(xmpp_accounts)), hub));
        tracing::info!(bind = %config.xmpp.bind, "XMPP listening");
        tasks.spawn(sbrw_xmpp::server::run(xmpp, server.clone(), shutdown.clone()));
        let stop = shutdown.clone();
        tasks.spawn(async move {
            loop {
                tokio::select! {
                    () = stop.cancelled() => return Ok(()),
                    push = push_queue.recv() => match push {
                        Some((to, body)) => server.with_hub(|hub| hub.push(to, &body)),
                        None => return Ok(()),
                    },
                }
            }
        });
    }
    if args.probe_udp {
        for (stream, bind) in
            [(CaptureStream::UdpFreeroam, config.udp.freeroam.bind), (CaptureStream::UdpRace, config.udp.race.bind)]
        {
            let socket = UdpSocket::bind(bind).await.with_context(|| format!("binding {} to {bind}", stream.name()))?;
            tracing::info!(%bind, server = stream.name(), "UDP probe listening");
            tasks.spawn(sbrw_udp::run_probe(stream, socket, capture.clone(), shutdown.clone()));
        }
    } else {
        let bind = config.udp.freeroam.bind;
        let socket = UdpSocket::bind(bind).await.with_context(|| format!("binding free-roam to {bind}"))?;
        tracing::info!(%bind, "free-roam listening");
        let settings = sbrw_udp::freeroam::Settings::from_config(&config.udp);
        tasks.spawn(sbrw_udp::run_freeroam(socket, settings, capture.clone(), shutdown.clone()));

        let bind = config.udp.race.bind;
        let socket = UdpSocket::bind(bind).await.with_context(|| format!("binding the race relay to {bind}"))?;
        tracing::info!(%bind, "race relay listening");
        let settings = sbrw_udp::race::Settings::from_config(&config.udp.race_relay);
        tasks.spawn(sbrw_udp::run_race(socket, settings, relay.clone(), capture.clone(), shutdown.clone()));
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

/// XMPP logins check the accounts the core creates at `GetPermanentSession`.
struct WorldAccounts(sbrw_core::openfire::Accounts);

impl sbrw_xmpp::stream::Accounts for WorldAccounts {
    fn password(&self, username: &str) -> Option<String> {
        self.0.read().ok()?.get(username).cloned()
    }
}

/// Fire upstream's single-action timers (the DNF countdown) when they're
/// due, checking every second.
async fn run_timers(world: sbrw_core::WorldHandle, shutdown: CancellationToken) -> std::io::Result<()> {
    let mut seconds = tokio::time::interval(Duration::from_secs(1));
    seconds.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            () = shutdown.cancelled() => return Ok(()),
            _ = seconds.tick() => loop {
                match world.call(sbrw_core::timers::fire_due_timer).await {
                    Ok(true) => world.run_async_jobs().await,
                    Ok(false) => break,
                    Err(fault) => {
                        // Any other timers due wait for the next second.
                        tracing::warn!("timer failed: {fault:#}");
                        break;
                    }
                }
            },
        }
    }
}

/// Upstream's `@Schedule` jobs at the start of every minute: the
/// every-minute ones, and on the hour and half hour the half-hourly ones.
async fn run_jobs(world: sbrw_core::WorldHandle, shutdown: CancellationToken) -> std::io::Result<()> {
    let into_minute = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(Duration::ZERO, |d| Duration::from_millis(u64::try_from(d.as_millis() % 60_000).unwrap_or(0)));
    let first = tokio::time::Instant::now() + (Duration::from_secs(60) - into_minute);
    let mut minutes = tokio::time::interval_at(first, Duration::from_secs(60));
    minutes.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            () = shutdown.cancelled() => return Ok(()),
            _ = minutes.tick() => {
                let minute = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| (d.as_secs() + 30) / 60 % 60);
                let half_hourly = if minute.is_multiple_of(30) { sbrw_core::jobs::EVERY_HALF_HOUR } else { &[] };
                for &job in sbrw_core::jobs::EVERY_MINUTE.iter().chain(half_hourly) {
                    if let Err(fault) = world.call(move |w| sbrw_core::jobs::run(w, job)).await {
                        tracing::warn!(job, "scheduled job failed: {fault:#}");
                    }
                    world.run_async_jobs().await;
                }
            }
        }
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
