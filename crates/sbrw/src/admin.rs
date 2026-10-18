//! `sbrw admin`: commands for a running server, sent to its operator API on
//! this machine with the token `sbrw serve` wrote into the data directory.
//! In Docker: `docker exec sbrw sbrw admin …`.

use std::io::{BufRead, IsTerminal, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use clap::Subcommand;
use sbrw_common::Config;
use serde_json::{Value, json};

#[derive(Subcommand)]
pub enum AdminCommand {
    /// Ban an account, named by a driver or an email, and kick it
    Ban {
        /// A driver's name, or the account's email
        target: String,
        /// How long, as the in-game /ban reads it (1d, 2h30m, 45m); for good if left out
        #[arg(long = "for", value_name = "LENGTH")]
        length: Option<String>,
        #[arg(long)]
        reason: Option<String>,
    },
    /// End an account's ban
    Unban {
        /// A driver's name, or the account's email
        target: String,
    },
    /// End an account's sessions, as the in-game /kick does
    Kick {
        /// A driver's name, or the account's email
        target: String,
    },
    /// Send a chat message to every player
    Announce {
        #[arg(required = true, num_args = 1..)]
        message: Vec<String>,
    },
    /// Make (or find) an invite ticket, for when registration needs one (TICKET_TOKEN)
    Ticket { discord_name: String },
    /// Let an account use the in-game admin commands (/kick, /ban, /unban) from its next login
    SetAdmin {
        email: String,
        /// Take the rights away instead
        #[arg(long)]
        revoke: bool,
    },
    /// Set an account's password, read from standard input
    SetPassword { email: String },
    /// Print the tokens the original's endpoints and web pages take, as this run has them
    Tokens,
    /// Copy the database into <data dir>/backups now
    Backup,
}

/// Where `sbrw serve` keeps the operator API's token.
pub fn token_path(config: &Config) -> PathBuf {
    config.server.data_dir.join("admin.token")
}

/// Write a fresh token, readable only by this user.
pub fn write_token(path: &Path, token: &str) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        options.mode(0o600);
        let mut file = options.open(path)?;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        file.write_all(token.as_bytes())
    }
    #[cfg(not(unix))]
    {
        options.open(path)?.write_all(token.as_bytes())
    }
}

pub fn run(config: &Config, command: AdminCommand) -> ExitCode {
    let (name, body) = match command {
        AdminCommand::Ban { target, length, reason } => {
            ("ban", json!({ "target": target, "length": length, "reason": reason }))
        }
        AdminCommand::Unban { target } => ("unban", json!({ "target": target })),
        AdminCommand::Kick { target } => ("kick", json!({ "target": target })),
        AdminCommand::Announce { message } => ("announce", json!({ "message": message.join(" ") })),
        AdminCommand::Ticket { discord_name } => ("ticket", json!({ "discord_name": discord_name })),
        AdminCommand::SetAdmin { email, revoke } => ("set-admin", json!({ "email": email, "admin": !revoke })),
        AdminCommand::SetPassword { email } => {
            if std::io::stdin().is_terminal() {
                eprint!("New password for {email}: ");
            }
            let mut password = String::new();
            if let Err(err) = std::io::stdin().lock().read_line(&mut password) {
                eprintln!("sbrw: reading the password: {err}");
                return ExitCode::FAILURE;
            }
            let password = password.trim_end_matches(['\r', '\n']).to_owned();
            ("set-password", json!({ "email": email, "password": password }))
        }
        AdminCommand::Tokens => ("tokens", json!({})),
        AdminCommand::Backup => ("backup", json!({})),
    };
    match send(config, name, &body) {
        Ok(reply) => {
            match reply.get("message").and_then(Value::as_str) {
                Some(message) => println!("{message}"),
                None => {
                    for (key, value) in reply.as_object().into_iter().flatten() {
                        println!("{key} = {}", value.as_str().unwrap_or("(unset)"));
                    }
                }
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("sbrw: {err}");
            ExitCode::FAILURE
        }
    }
}

/// One request to the local server, in plain blocking HTTP/1.1.
fn send(config: &Config, command: &str, body: &Value) -> Result<Value, String> {
    let path = token_path(config);
    let token = std::fs::read_to_string(&path).map_err(|err| {
        format!("reading {}: {err} (is sbrw serve running with this data directory?)", path.display())
    })?;
    let ip = match config.http.bind.ip() {
        ip if ip.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        ip => ip,
    };
    let addr = SocketAddr::new(ip, config.http.bind.port());
    let body = body.to_string();
    let request = format!(
        "POST /admin/{command} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {}\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        token.trim(),
        body.len()
    );
    let mut response = Vec::new();
    let timeout = Duration::from_secs(30);
    TcpStream::connect_timeout(&addr, Duration::from_secs(3))
        .and_then(|mut stream| {
            stream.set_read_timeout(Some(timeout))?;
            stream.write_all(request.as_bytes())?;
            stream.read_to_end(&mut response)
        })
        .map_err(|err| format!("talking to the server at {addr}: {err} (is sbrw serve running?)"))?;
    let response = String::from_utf8_lossy(&response);
    let (head, payload) = response.split_once("\r\n\r\n").ok_or("the server sent no response")?;
    // The server closes each connection, so a body is all that follows; it
    // may come chunked.
    let payload = if head.to_ascii_lowercase().contains("transfer-encoding: chunked") {
        dechunk(payload)
    } else {
        payload.to_owned()
    };
    let status = head.split(' ').nth(1).unwrap_or("");
    let reply: Value = serde_json::from_str(&payload).unwrap_or(Value::Null);
    if status == "200" {
        Ok(reply)
    } else {
        Err(reply
            .get("error")
            .and_then(Value::as_str)
            .map_or_else(|| format!("the server answered {status}"), str::to_owned))
    }
}

fn dechunk(mut body: &str) -> String {
    let mut out = String::new();
    while let Some((size, rest)) = body.split_once("\r\n") {
        let Ok(size) = usize::from_str_radix(size.trim(), 16) else { break };
        if size == 0 || rest.len() < size {
            break;
        }
        out.push_str(&rest[..size]);
        body = rest[size..].trim_start_matches("\r\n");
    }
    out
}
