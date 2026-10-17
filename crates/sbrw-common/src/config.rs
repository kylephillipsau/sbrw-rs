//! Typed server configuration.
//!
//! Layers, lowest priority first: compiled defaults, the TOML file
//! (`<data_dir>/sbrw.toml` unless `--config` says otherwise), then environment
//! variables such as `SBRW__SERVER__PUBLIC_HOST=1.2.3.4`. `SBRW_CAPTURE=http,xmpp`
//! is a shorthand for `capture.streams`.
//!
//! Upstream SBRW kept these values in its `parameter` table; the doc comment on
//! each field names the parameter it replaces.

use std::collections::BTreeMap;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};

use figment::Figment;
use figment::providers::{Env, Format, Serialized, Toml};
use serde::{Deserialize, Serialize};

use crate::capture::CaptureStream;

/// Environment variable naming the config file.
pub const CONFIG_PATH_ENV: &str = "SBRW_CONFIG";
/// Shorthand environment variable for `capture.streams`.
pub const CAPTURE_ENV: &str = "SBRW_CAPTURE";
/// Prefix for nested environment overrides (`SBRW__SECTION__KEY`).
pub const ENV_PREFIX: &str = "SBRW__";
/// Prefix for upstream parameter overrides, which keep their case (`SBRW_PARAMETER_TICKET_TOKEN`).
pub const PARAMETER_ENV_PREFIX: &str = "SBRW_PARAMETER_";

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error(transparent)]
    Figment(#[from] Box<figment::Error>),
    #[error("{CAPTURE_ENV}: unknown stream {0:?} (expected http, xmpp, udp-freeroam, udp-race)")]
    CaptureStream(String),
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Overrides for upstream's `parameter` table (`SESSION_LENGTH_MINUTES`,
    /// `TICKET_TOKEN`, ...), by upstream's names. Also settable as
    /// `SBRW_PARAMETER_<NAME>=value`.
    pub parameters: BTreeMap<String, String>,
    pub server: ServerConfig,
    pub http: HttpConfig,
    pub xmpp: XmppConfig,
    pub udp: UdpConfig,
    pub launcher: LauncherConfig,
    pub modding: ModdingConfig,
    pub capture: CaptureConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerConfig {
    /// Host the game client is told to connect to for HTTP, XMPP, UDP and mods.
    /// Use an IP literal reachable by players: `127.0.0.1` when the game runs on
    /// the same machine, the public IP for friends. Replaces `SERVER_ADDRESS`,
    /// `XMPP_IP`, `UDP_FREEROAM_IP` and `UDP_RACE_IP`.
    pub public_host: String,
    /// Directory for the database, TLS material, mods and captures.
    pub data_dir: PathBuf,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self { public_host: "127.0.0.1".into(), data_dir: PathBuf::from("data") }
    }
}

/// A listening socket plus the port advertised to clients, which differs from
/// the bound port when a container or NAT remaps it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Listener {
    pub bind: SocketAddr,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_port: Option<u16>,
}

impl Listener {
    fn any(port: u16) -> Self {
        Self { bind: SocketAddr::new(IpAddr::from([0, 0, 0, 0]), port), public_port: None }
    }

    pub fn advertised_port(&self) -> u16 {
        self.public_port.unwrap_or(self.bind.port())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HttpConfig {
    pub bind: SocketAddr,
    pub public_port: Option<u16>,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self { bind: SocketAddr::new(IpAddr::from([0, 0, 0, 0]), 8680), public_port: None }
    }
}

impl HttpConfig {
    pub fn advertised_port(&self) -> u16 {
        self.public_port.unwrap_or(self.bind.port())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct XmppConfig {
    /// Replaces `XMPP_PORT`.
    pub bind: SocketAddr,
    pub public_port: Option<u16>,
}

impl Default for XmppConfig {
    fn default() -> Self {
        Self { bind: SocketAddr::new(IpAddr::from([0, 0, 0, 0]), 5222), public_port: None }
    }
}

impl XmppConfig {
    pub fn advertised_port(&self) -> u16 {
        self.public_port.unwrap_or(self.bind.port())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UdpConfig {
    /// Replaces `UDP_FREEROAM_PORT`.
    pub freeroam: Listener,
    /// Replaces `UDP_RACE_PORT`.
    pub race: Listener,
    /// Free-roam: how far apart players see each other, in the game's units.
    /// The rest of this section follows the Go free-roam server's `[UDP]`
    /// settings (`VisibilityRadius` here) and defaults.
    pub visibility_radius: f64,
    /// Free-roam: how many other players each player sees at once
    /// (`MaxVisiblePlayers`).
    pub max_visible_players: usize,
    /// Free-roam: one more player comes into view this often, in
    /// milliseconds; 0 means 200 (`PlayerSpawnDelayMs`).
    pub player_spawn_delay_ms: u64,
    /// Free-roam: every player sees every other, however far
    /// (`DisableRadiusSync`).
    pub disable_radius_sync: bool,
    /// The race relay's limits (`docs/protocol/race.md`, "sbrw-rs differs").
    pub race_relay: RaceRelayConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RaceRelayConfig {
    /// How long a race waits for all its racers to say hello before it goes
    /// on with those there; 0 waits forever, as the original relay does.
    pub wait_for_all_secs: u64,
    /// A racer silent this long no longer holds the others up.
    pub racer_idle_secs: u64,
    /// A race with no traffic this long is over.
    pub session_idle_secs: u64,
    /// Answer every datagram with the byte `1`, as the original relay does.
    pub legacy_ack_byte: bool,
}

impl Default for RaceRelayConfig {
    fn default() -> Self {
        Self { wait_for_all_secs: 60, racer_idle_secs: 10, session_idle_secs: 30, legacy_ack_byte: true }
    }
}

impl Default for UdpConfig {
    fn default() -> Self {
        Self {
            freeroam: Listener::any(9999),
            race: Listener::any(9998),
            visibility_radius: 300.0,
            max_visible_players: 14,
            player_spawn_delay_ms: 200,
            disable_radius_sync: false,
            race_relay: RaceRelayConfig::default(),
        }
    }
}

/// What the launcher shows in its server list (`GET /GetServerInformation`).
/// Each field sets the upstream parameter it names; an unset optional field
/// keeps upstream's default row (country `US`, admins and owners `???`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LauncherConfig {
    /// `SERVER_INFO_NAME`
    pub server_name: String,
    /// `SERVER_INFO_MESSAGE`
    pub message: Option<String>,
    /// `SERVER_INFO_COUNTRY`
    pub country: Option<String>,
    /// `SERVER_INFO_TIMEZONE`
    pub timezone: i32,
    /// `SERVER_INFO_BANNER_URL`
    pub banner_url: Option<String>,
    /// `SERVER_INFO_HOMEPAGE_URL`
    pub homepage_url: Option<String>,
    /// `SERVER_INFO_FACEBOOK_URL`
    pub facebook_url: Option<String>,
    /// `SERVER_INFO_DISCORD_URL`
    pub discord_url: Option<String>,
    /// `SERVER_INFO_ADMINS`
    pub admins: Option<String>,
    /// `SERVER_INFO_OWNERS`
    pub owners: Option<String>,
    /// `SERVER_INFO_ALLOWED_COUNTRIES`
    pub allowed_countries: Option<String>,
    /// `SERVER_INFO_SHUTDOWN_TIMER`
    pub seconds_to_shutdown: i32,
    /// `SERVER_INFO_ENABLED_SCENERY`: holiday scenery the game turns on.
    /// Defaults to upstream's `SCENERY_GROUP_NORMAL`.
    pub enabled_scenery: Vec<String>,
    /// `SERVER_INFO_DISABLED_SCENERY`. Defaults to upstream's
    /// `SCENERY_GROUP_NORMAL_DISABLE`.
    pub disabled_scenery: Vec<String>,
}

impl Default for LauncherConfig {
    fn default() -> Self {
        Self {
            server_name: "sbrw-rs".into(),
            message: None,
            country: None,
            timezone: 0,
            banner_url: None,
            homepage_url: None,
            facebook_url: None,
            discord_url: None,
            admins: None,
            owners: None,
            allowed_countries: None,
            seconds_to_shutdown: 7200,
            enabled_scenery: vec!["SCENERY_GROUP_NORMAL".into()],
            disabled_scenery: vec!["SCENERY_GROUP_NORMAL_DISABLE".into()],
        }
    }
}

/// ModNet: when enabled, the SBRW launcher installs its client patches (which
/// turn off the game's UDP encryption) and downloads this server's mods.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ModdingConfig {
    /// `MODDING_ENABLED`. When false, `/Modding/GetModInfo` returns 404.
    pub enabled: bool,
    /// `MODDING_SERVER_ID`. The launcher caches mods per MD5 of this value.
    pub server_id: String,
    /// `MODDING_FEATURES`
    pub features: Vec<String>,
}

impl Default for ModdingConfig {
    fn default() -> Self {
        Self { enabled: true, server_id: "sbrw-rs".into(), features: Vec::new() }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CaptureConfig {
    /// Which protocols to record. Empty disables capture.
    pub streams: Vec<CaptureStream>,
    /// Where captures go. Defaults to `<data_dir>/captures`.
    pub dir: Option<PathBuf>,
}

impl Config {
    /// Load from defaults, the config file (if present) and the environment.
    pub fn load(path: Option<&Path>) -> Result<Self, ConfigError> {
        let path = path.map(Path::to_path_buf).unwrap_or_else(default_config_path);
        Self::from_figment(
            Figment::from(Serialized::defaults(Config::default()))
                .merge(Toml::file(path))
                .merge(Env::prefixed(ENV_PREFIX).split("__")),
        )
    }

    fn from_figment(figment: Figment) -> Result<Self, ConfigError> {
        let mut config: Config = figment.extract().map_err(Box::new)?;
        if let Ok(streams) = std::env::var(CAPTURE_ENV) {
            config.capture.streams = parse_capture_streams(&streams)?;
        }
        for (key, value) in std::env::vars() {
            if let Some(name) = key.strip_prefix(PARAMETER_ENV_PREFIX) {
                config.parameters.insert(name.to_owned(), value);
            }
        }
        Ok(config)
    }

    /// Upstream `parameter` rows this config sets, for the world to apply over
    /// upstream's defaults. A field that is unset (`None`) leaves upstream's
    /// row as it is; `parameters` entries win over everything else.
    pub fn upstream_parameters(&self) -> BTreeMap<String, String> {
        let mut p = BTreeMap::new();
        let mut set = |name: &str, value: String| {
            p.insert(name.to_owned(), value);
        };
        let host = &self.server.public_host;
        set("SERVER_ADDRESS", self.public_http_url());
        set("XMPP_IP", host.clone());
        set("XMPP_PORT", self.xmpp.advertised_port().to_string());
        set("UDP_FREEROAM_IP", host.clone());
        set("UDP_FREEROAM_PORT", self.udp.freeroam.advertised_port().to_string());
        set("UDP_RACE_IP", host.clone());
        set("UDP_RACE_PORT", self.udp.race.advertised_port().to_string());

        let l = &self.launcher;
        set("SERVER_INFO_NAME", l.server_name.clone());
        set("SERVER_INFO_TIMEZONE", l.timezone.to_string());
        set("SERVER_INFO_SHUTDOWN_TIMER", l.seconds_to_shutdown.to_string());
        set("SERVER_INFO_ENABLED_SCENERY", l.enabled_scenery.join(";"));
        set("SERVER_INFO_DISABLED_SCENERY", l.disabled_scenery.join(";"));
        for (name, value) in [
            ("SERVER_INFO_MESSAGE", &l.message),
            ("SERVER_INFO_COUNTRY", &l.country),
            ("SERVER_INFO_BANNER_URL", &l.banner_url),
            ("SERVER_INFO_HOMEPAGE_URL", &l.homepage_url),
            ("SERVER_INFO_FACEBOOK_URL", &l.facebook_url),
            ("SERVER_INFO_DISCORD_URL", &l.discord_url),
            ("SERVER_INFO_ADMINS", &l.admins),
            ("SERVER_INFO_OWNERS", &l.owners),
            ("SERVER_INFO_ALLOWED_COUNTRIES", &l.allowed_countries),
        ] {
            if let Some(value) = value {
                set(name, value.clone());
            }
        }

        set("MODDING_ENABLED", self.modding.enabled.to_string());
        set("MODDING_SERVER_ID", self.modding.server_id.clone());
        set("MODDING_BASE_PATH", self.mods_base_url());
        set("MODDING_FEATURES", self.modding.features.join(";"));

        p.extend(self.parameters.clone());
        p
    }

    /// Base URL players reach this server's HTTP listener on.
    pub fn public_http_url(&self) -> String {
        format!("http://{}:{}", self.server.public_host, self.http.advertised_port())
    }

    /// Base URL of the ModNet file tree (`index.json` and mod packages).
    pub fn mods_base_url(&self) -> String {
        format!("{}/mods", self.public_http_url())
    }

    pub fn mods_dir(&self) -> PathBuf {
        self.server.data_dir.join("mods")
    }

    pub fn capture_dir(&self) -> PathBuf {
        self.capture.dir.clone().unwrap_or_else(|| self.server.data_dir.join("captures"))
    }

    /// Problems worth warning about at startup; none of them are fatal.
    pub fn warnings(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        if self.server.public_host.parse::<IpAddr>().is_err() {
            warnings.push(format!(
                "server.public_host {:?} is not an IP literal; it is unverified whether the game resolves host names for XMPP and UDP",
                self.server.public_host
            ));
        }
        if self.server.public_host.parse::<IpAddr>().is_ok_and(|ip| ip.is_unspecified()) {
            warnings.push("server.public_host is 0.0.0.0; clients cannot connect to that".into());
        }
        warnings
    }
}

/// `$SBRW_CONFIG`, else `sbrw.toml` inside the data directory, which itself may
/// come from `SBRW__SERVER__DATA_DIR`.
fn default_config_path() -> PathBuf {
    if let Ok(path) = std::env::var(CONFIG_PATH_ENV) {
        return PathBuf::from(path);
    }
    let data_dir = std::env::var(format!("{ENV_PREFIX}SERVER__DATA_DIR"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| ServerConfig::default().data_dir);
    data_dir.join("sbrw.toml")
}

fn parse_capture_streams(value: &str) -> Result<Vec<CaptureStream>, ConfigError> {
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.parse().map_err(|_| ConfigError::CaptureStream(s.to_owned())))
        .collect()
}

#[cfg(test)]
// figment's `Jail` closures must return `figment::Error`, which is large.
#[allow(clippy::result_large_err)]
mod tests {
    use super::*;
    use figment::Jail;

    fn load_in_jail() -> Result<Config, figment::Error> {
        Config::load(Some(Path::new("sbrw.toml"))).map_err(|e| figment::Error::from(e.to_string()))
    }

    #[test]
    fn defaults_without_file_or_env() {
        Jail::expect_with(|_| {
            let config = load_in_jail()?;
            assert_eq!(config, Config::default());
            assert_eq!(config.public_http_url(), "http://127.0.0.1:8680");
            assert_eq!(config.mods_base_url(), "http://127.0.0.1:8680/mods");
            Ok(())
        });
    }

    #[test]
    fn file_then_env_override() {
        Jail::expect_with(|jail| {
            jail.create_file(
                "sbrw.toml",
                r#"
                [parameters]
                SESSION_LENGTH_MINUTES = "60"

                [server]
                public_host = "10.0.0.5"

                [http]
                bind = "0.0.0.0:9000"
                public_port = 80

                [launcher]
                server_name = "From file"
                enabled_scenery = ["SCENERY_GROUP_HALLOWEEN"]
                "#,
            )?;
            jail.set_env("SBRW__LAUNCHER__SERVER_NAME", "From env");
            jail.set_env("SBRW__UDP__RACE__BIND", "0.0.0.0:7000");
            jail.set_env("SBRW__UDP__MAX_VISIBLE_PLAYERS", "8");
            jail.set_env("SBRW__UDP__RACE_RELAY__WAIT_FOR_ALL_SECS", "0");
            jail.set_env("SBRW_CAPTURE", "http, udp-race");
            jail.set_env("SBRW_PARAMETER_TICKET_TOKEN", "secret");
            let config = load_in_jail()?;
            assert_eq!(config.launcher.server_name, "From env");
            assert_eq!(config.launcher.enabled_scenery, ["SCENERY_GROUP_HALLOWEEN"]);
            assert_eq!(config.http.bind.port(), 9000);
            assert_eq!(config.public_http_url(), "http://10.0.0.5:80");
            assert_eq!(config.udp.race.bind.port(), 7000);
            assert_eq!(config.udp.freeroam.bind.port(), 9999);
            assert_eq!(config.udp.max_visible_players, 8);
            assert_eq!(config.udp.visibility_radius, 300.0);
            assert_eq!(config.udp.race_relay.wait_for_all_secs, 0);
            assert_eq!(config.udp.race_relay.racer_idle_secs, 10);
            assert_eq!(config.capture.streams, [CaptureStream::Http, CaptureStream::UdpRace]);
            assert_eq!(config.parameters["TICKET_TOKEN"], "secret");
            assert_eq!(config.parameters["SESSION_LENGTH_MINUTES"], "60");
            Ok(())
        });
    }

    #[test]
    fn unknown_keys_are_rejected() {
        Jail::expect_with(|jail| {
            jail.create_file("sbrw.toml", "[launcher]\nserver_nmae = \"typo\"\n")?;
            assert!(load_in_jail().is_err());
            Ok(())
        });
    }

    #[test]
    fn bad_capture_stream_is_rejected() {
        Jail::expect_with(|jail| {
            jail.set_env("SBRW_CAPTURE", "http,smtp");
            assert!(load_in_jail().is_err());
            Ok(())
        });
    }

    #[test]
    fn warns_about_unusable_public_host() {
        let mut config = Config::default();
        assert!(config.warnings().is_empty());
        config.server.public_host = "0.0.0.0".into();
        assert_eq!(config.warnings().len(), 1);
        config.server.public_host = "game.example.com".into();
        assert_eq!(config.warnings().len(), 1);
    }

    #[test]
    fn config_sets_upstream_parameters() {
        let mut config = Config::default();
        config.server.public_host = "203.0.113.7".into();
        config.xmpp.public_port = Some(15222);
        config.launcher.discord_url = Some("https://discord.gg/x".into());
        config.modding.features = vec!["A".into(), "B".into()];
        config.parameters.insert("XMPP_PORT".into(), "5333".into());
        let p = config.upstream_parameters();
        assert_eq!(p["XMPP_IP"], "203.0.113.7");
        assert_eq!(p["UDP_FREEROAM_IP"], "203.0.113.7");
        assert_eq!(p["UDP_FREEROAM_PORT"], "9999");
        assert_eq!(p["UDP_RACE_IP"], "203.0.113.7");
        assert_eq!(p["UDP_RACE_PORT"], "9998");
        assert_eq!(p["SERVER_ADDRESS"], "http://203.0.113.7:8680");
        // An explicit parameter wins over the typed config.
        assert_eq!(p["XMPP_PORT"], "5333");
        assert_eq!(p["SERVER_INFO_NAME"], "sbrw-rs");
        assert_eq!(p["SERVER_INFO_DISCORD_URL"], "https://discord.gg/x");
        assert_eq!(p["SERVER_INFO_ENABLED_SCENERY"], "SCENERY_GROUP_NORMAL");
        // Unset fields keep upstream's rows.
        assert!(!p.contains_key("SERVER_INFO_COUNTRY"));
        assert_eq!(p["MODDING_ENABLED"], "true");
        assert_eq!(p["MODDING_BASE_PATH"], "http://203.0.113.7:8680/mods");
        assert_eq!(p["MODDING_FEATURES"], "A;B");
    }
}
