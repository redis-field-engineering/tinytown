/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Configuration management for tinytown.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use url::Url;

use crate::agent::AgentCli;
use crate::error::{Error, Result};
use crate::global_config::{GlobalConfig, normalize_builtin_cli_reference};

/// Default Redis socket path within a town (under .tt/).
pub const DEFAULT_SOCKET_NAME: &str = ".tt/redis.sock";

/// Default config file name.
pub const CONFIG_FILE: &str = "tinytown.toml";

/// Town configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Town name
    pub name: String,

    /// Town root directory
    #[serde(skip)]
    pub root: PathBuf,

    /// Redis configuration
    #[serde(default)]
    pub redis: RedisConfig,

    /// Townhall daemon configuration
    #[serde(default)]
    pub townhall: TownhallConfig,

    /// Agent worker runtime configuration
    #[serde(default)]
    pub agent: AgentConfig,

    /// Available agent CLIs (e.g., claude, auggie, codex)
    #[serde(default)]
    pub agent_clis: HashMap<String, AgentCli>,

    /// Default CLI to use when spawning agents
    #[serde(default = "default_cli")]
    pub default_cli: String,

    /// CLI to use for the interactive conductor
    #[serde(default)]
    pub conductor_cli: Option<String>,

    /// Maximum concurrent agents
    #[serde(default = "default_max_agents")]
    pub max_agents: usize,

    /// Whether using central Redis (managed in ~/.tt/) vs per-town Redis
    /// This is set at creation time based on GlobalConfig and not re-read
    #[serde(default)]
    pub use_central_redis: bool,

    /// Use Redis Streams (Docket pattern) for task dispatch instead of Lists.
    ///
    /// When enabled, backlog operations use XADD/XREADGROUP/XACK instead of
    /// RPUSH/BLPOP, providing at-least-once delivery, crash recovery via
    /// XPENDING, and consumer-group distribution across workers.
    ///
    /// Default: false (List-based backlog for backward compatibility).
    #[serde(default)]
    pub use_streams: bool,
}

/// Townhall daemon configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TownhallConfig {
    /// Bind address for REST API
    #[serde(default = "default_bind")]
    pub bind: String,

    /// REST API port
    #[serde(default = "default_rest_port")]
    pub rest_port: u16,

    /// Request timeout in milliseconds
    #[serde(default = "default_timeout_ms")]
    pub request_timeout_ms: u64,

    /// Authentication configuration
    #[serde(default)]
    pub auth: AuthConfig,

    /// TLS configuration for server certificates
    #[serde(default)]
    pub tls: TlsConfig,

    /// Mutual TLS configuration for client certificate auth
    #[serde(default)]
    pub mtls: MtlsConfig,
}

/// Agent worker runtime configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Seconds an idle worker should wait before draining and exiting.
    #[serde(default = "default_agent_idle_timeout_secs")]
    pub idle_timeout_secs: u64,
}

/// Authentication mode for townhall.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    /// No authentication required (only safe on loopback)
    #[default]
    None,
    /// API key authentication via Bearer token or X-API-Key header
    ApiKey,
    /// OIDC JWT authentication
    Oidc,
}

/// Authentication configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    /// Authentication mode
    #[serde(default)]
    pub mode: AuthMode,

    // --- API Key mode settings ---
    /// Argon2id hash of the API key (not the raw key!)
    #[serde(default)]
    pub api_key_hash: Option<String>,

    /// Scopes granted to API key authentication (defaults to all scopes if empty)
    #[serde(default)]
    pub api_key_scopes: Vec<Scope>,

    // --- OIDC mode settings ---
    /// OIDC issuer URL (e.g., "https://issuer.example.com")
    #[serde(default)]
    pub issuer: Option<String>,

    /// Expected audience claim
    #[serde(default)]
    pub audience: Option<String>,

    /// JWKS URL for key validation
    #[serde(default)]
    pub jwks_url: Option<String>,

    /// Required scopes for access
    #[serde(default)]
    pub required_scopes: Vec<String>,

    /// Clock skew tolerance in seconds for JWT validation
    #[serde(default = "default_clock_skew")]
    pub clock_skew_seconds: u64,
}

fn default_clock_skew() -> u64 {
    60
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            mode: AuthMode::None,
            api_key_hash: None,
            api_key_scopes: Vec::new(),
            issuer: None,
            audience: None,
            jwks_url: None,
            required_scopes: Vec::new(),
            clock_skew_seconds: default_clock_skew(),
        }
    }
}

/// Authorization scopes for townhall endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// Read town/agent/task status
    #[serde(rename = "town.read")]
    TownRead,
    /// Write operations: assign tasks, send messages, claim backlog
    #[serde(rename = "town.write")]
    TownWrite,
    /// Agent lifecycle: spawn, kill, restart, prune, recover
    #[serde(rename = "agent.manage")]
    AgentManage,
    /// Administrative operations
    #[serde(rename = "admin")]
    Admin,
}

impl Scope {
    /// Parse scope from string representation.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "town.read" | "town:read" => Some(Scope::TownRead),
            "town.write" | "town:write" => Some(Scope::TownWrite),
            "agent.manage" | "agent:manage" => Some(Scope::AgentManage),
            "admin" => Some(Scope::Admin),
            _ => None,
        }
    }

    /// Get string representation.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Scope::TownRead => "town.read",
            Scope::TownWrite => "town.write",
            Scope::AgentManage => "agent.manage",
            Scope::Admin => "admin",
        }
    }
}

impl std::fmt::Display for Scope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// TLS configuration for server certificates.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TlsConfig {
    /// Whether TLS is enabled
    #[serde(default)]
    pub enabled: bool,

    /// Path to TLS certificate file (PEM)
    #[serde(default)]
    pub cert_file: Option<String>,

    /// Path to TLS private key file (PEM)
    #[serde(default)]
    pub key_file: Option<String>,
}

impl TlsConfig {
    /// Validate TLS configuration. Returns error message if invalid.
    pub fn validate(&self) -> Option<&'static str> {
        if !self.enabled {
            return None;
        }
        if self.cert_file.is_none() {
            return Some("TLS enabled but cert_file is not configured");
        }
        if self.key_file.is_none() {
            return Some("TLS enabled but key_file is not configured");
        }
        // Check files exist
        if let Some(cert) = &self.cert_file
            && !std::path::Path::new(cert).exists()
        {
            return Some("TLS cert_file does not exist");
        }
        if let Some(key) = &self.key_file
            && !std::path::Path::new(key).exists()
        {
            return Some("TLS key_file does not exist");
        }
        None
    }
}

/// Mutual TLS (mTLS) configuration for client certificate auth.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MtlsConfig {
    /// Whether mTLS is enabled
    #[serde(default)]
    pub enabled: bool,

    /// Whether mTLS is required (vs optional)
    #[serde(default)]
    pub required: bool,

    /// Path to CA certificate for client verification
    #[serde(default)]
    pub ca_file: Option<String>,
}

impl MtlsConfig {
    /// Validate mTLS configuration. Returns error message if invalid.
    pub fn validate(&self) -> Option<&'static str> {
        if !self.enabled && !self.required {
            return None;
        }
        if self.required && self.ca_file.is_none() {
            return Some("mTLS required but ca_file is not configured");
        }
        if let Some(ca) = &self.ca_file
            && !std::path::Path::new(ca).exists()
        {
            return Some("mTLS ca_file does not exist");
        }
        None
    }
}

fn default_rest_port() -> u16 {
    8080
}

fn default_timeout_ms() -> u64 {
    30000
}

fn default_agent_idle_timeout_secs() -> u64 {
    300
}

impl Default for TownhallConfig {
    fn default() -> Self {
        Self {
            bind: default_bind(),
            rest_port: default_rest_port(),
            request_timeout_ms: default_timeout_ms(),
            auth: AuthConfig::default(),
            tls: TlsConfig::default(),
            mtls: MtlsConfig::default(),
        }
    }
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            idle_timeout_secs: default_agent_idle_timeout_secs(),
        }
    }
}

fn default_cli() -> String {
    "claude".to_string()
}

fn default_max_agents() -> usize {
    10
}

fn inferred_cli_alias(cli: &str) -> Option<&'static str> {
    let trimmed = cli.trim();
    let first = trimmed.split_whitespace().next()?;

    match first {
        "claude" => Some("claude"),
        "auggie" => Some("auggie"),
        "aider" => Some("aider"),
        "gemini" => Some("gemini"),
        "cursor" => Some("cursor"),
        "gh" if trimmed.starts_with("gh copilot") => Some("copilot"),
        "codex" => {
            if trimmed.contains("gpt-5.4-mini") {
                Some("codex-mini")
            } else {
                Some("codex")
            }
        }
        _ => None,
    }
}

fn normalize_cli_reference(cli: &str, agent_clis: &HashMap<String, AgentCli>) -> String {
    let trimmed = cli.trim();
    if trimmed.is_empty() {
        return default_cli();
    }

    if agent_clis.contains_key(trimmed) {
        return trimmed.to_string();
    }

    if let Some((name, _)) = agent_clis
        .iter()
        .find(|(_, config)| config.command.trim() == trimmed)
    {
        return name.clone();
    }

    if let Some(alias) = inferred_cli_alias(trimmed)
        && agent_clis.contains_key(alias)
    {
        return alias.to_string();
    }

    trimmed.to_string()
}

/// Redis connection configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedisConfig {
    /// Explicit Redis connection URL (e.g. redis:// or rediss://)
    #[serde(default)]
    pub url: Option<String>,

    /// Use Unix socket (faster) vs TCP
    #[serde(default = "default_true")]
    pub use_socket: bool,

    /// Socket path (relative to town root)
    #[serde(default = "default_socket_path")]
    pub socket_path: String,

    /// TCP host (if not using socket)
    #[serde(default = "default_host")]
    pub host: String,

    /// TCP port (if not using socket)
    #[serde(default = "default_port")]
    pub port: u16,

    /// Enable AOF persistence (state survives restart)
    #[serde(default)]
    pub persist: bool,

    /// AOF file path (relative to town root)
    #[serde(default = "default_aof_path")]
    pub aof_path: String,

    // Security fields for TCP mode
    /// Redis password (AUTH command)
    #[serde(default)]
    pub password: Option<String>,

    /// Enable TLS encryption
    #[serde(default)]
    pub tls_enabled: bool,

    /// Path to TLS certificate file (PEM)
    #[serde(default)]
    pub tls_cert: Option<String>,

    /// Path to TLS private key file (PEM)
    #[serde(default)]
    pub tls_key: Option<String>,

    /// Path to CA certificate for verification
    #[serde(default)]
    pub tls_ca_cert: Option<String>,

    /// Bind address for managed Redis (0.0.0.0 for remote, 127.0.0.1 for local only)
    #[serde(default = "default_bind")]
    pub bind: String,
}

fn default_aof_path() -> String {
    ".tt/redis.aof".to_string()
}

fn default_true() -> bool {
    true
}

fn default_socket_path() -> String {
    DEFAULT_SOCKET_NAME.to_string()
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> u16 {
    6379
}

fn default_bind() -> String {
    "127.0.0.1".to_string()
}

impl Default for RedisConfig {
    fn default() -> Self {
        Self {
            url: None,
            use_socket: true,
            socket_path: DEFAULT_SOCKET_NAME.to_string(),
            host: "127.0.0.1".to_string(),
            port: 6379,
            persist: false,
            aof_path: default_aof_path(),
            password: None,
            tls_enabled: false,
            tls_cert: None,
            tls_key: None,
            tls_ca_cert: None,
            bind: default_bind(),
        }
    }
}

impl Config {
    /// Create a new configuration with defaults from GlobalConfig.
    ///
    /// This loads the global config from ~/.tt/config.toml and uses its settings
    /// for default_cli and Redis configuration. If no global config exists,
    /// sensible defaults are used (central Redis on port 16379 with auto-generated password).
    ///
    /// Environment variable overrides:
    /// - `TT_USE_SOCKET=1` - Force Unix socket mode (useful for tests)
    #[must_use]
    pub fn new(name: impl Into<String>, root: impl Into<PathBuf>) -> Self {
        // Load global config for defaults, initializing with password if needed
        // This ensures central Redis always has a password even when called as library
        // If load fails (corrupted config, I/O error), fall back to socket mode for safety
        let (global, global_load_failed) = match GlobalConfig::load_or_init() {
            Ok(g) => (g, false),
            Err(_) => {
                // Fall back to default but mark that we should use socket mode
                // This prevents starting passwordless central Redis on config errors
                (GlobalConfig::default(), true)
            }
        };

        // Check for test/override mode - force Unix socket if TT_USE_SOCKET=1
        // Also force socket if global config failed to load (safety fallback)
        let force_socket = global_load_failed
            || std::env::var("TT_USE_SOCKET")
                .map(|v| v == "1" || v.to_lowercase() == "true")
                .unwrap_or(false);

        let mut agent_clis = HashMap::new();

        // Built-in CLI presets with correct non-interactive flags

        // Claude Code: --print for non-interactive, --dangerously-skip-permissions for full access
        agent_clis.insert(
            "claude".to_string(),
            AgentCli::new("claude", "claude --print --dangerously-skip-permissions"),
        );

        // Auggie (Augment CLI): --print for non-interactive
        agent_clis.insert(
            "auggie".to_string(),
            AgentCli::new("auggie", "auggie --print"),
        );

        // Codex: exec for non-interactive, --dangerously-bypass-approvals-and-sandbox for full access
        agent_clis.insert(
            "codex".to_string(),
            AgentCli::new(
                "codex",
                "codex exec --dangerously-bypass-approvals-and-sandbox",
            ),
        );
        agent_clis.insert(
            "codex-mini".to_string(),
            AgentCli::new(
                "codex-mini",
                "codex exec --dangerously-bypass-approvals-and-sandbox -m gpt-5.4-mini -c model_reasoning_effort=\"medium\"",
            ),
        );

        // Aider: --yes for auto-confirm, --no-auto-commits to not auto-commit
        agent_clis.insert(
            "aider".to_string(),
            AgentCli::new("aider", "aider --yes --no-auto-commits --message"),
        );

        // These may need updates when their CLIs are available/verified
        agent_clis.insert("gemini".to_string(), AgentCli::new("gemini", "gemini"));
        agent_clis.insert(
            "copilot".to_string(),
            AgentCli::new("copilot", "gh copilot"),
        );
        agent_clis.insert("cursor".to_string(), AgentCli::new("cursor", "cursor"));
        for (name, command) in &global.agent_clis {
            agent_clis.insert(name.clone(), AgentCli::new(name, command));
        }

        let normalized_default_cli = normalize_cli_reference(&global.default_cli, &agent_clis);
        let conductor_cli = global.conductor_cli.clone();

        // Build Redis config from global settings
        // By default, use central TCP Redis (not per-town Unix sockets)
        // Unless TT_USE_SOCKET=1 is set (for tests/isolation)
        let (redis, use_central_redis) = if force_socket {
            // Force Unix socket mode (for tests or explicit isolation)
            (RedisConfig::default(), false)
        } else if global.redis.use_central {
            (
                RedisConfig {
                    url: None,
                    use_socket: false,
                    socket_path: DEFAULT_SOCKET_NAME.to_string(),
                    host: global.redis.host.clone(),
                    port: global.redis.port,
                    persist: false,
                    aof_path: default_aof_path(),
                    password: global.redis.password.clone(),
                    tls_enabled: false,
                    tls_cert: None,
                    tls_key: None,
                    tls_ca_cert: None,
                    bind: "127.0.0.1".to_string(),
                },
                true,
            )
        } else {
            // Fall back to per-town Unix socket
            (RedisConfig::default(), false)
        };

        Self {
            name: name.into(),
            root: root.into(),
            redis,
            agent_clis,
            default_cli: normalized_default_cli,
            conductor_cli,
            max_agents: 10,
            use_central_redis,
            use_streams: false,
            townhall: TownhallConfig::default(),
            agent: AgentConfig::default(),
        }
    }

    /// Load configuration from a town directory.
    pub fn load(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref();
        let config_path = root.join(CONFIG_FILE);

        if !config_path.exists() {
            return Err(Error::NotInitialized(root.display().to_string()));
        }

        let content = std::fs::read_to_string(&config_path)?;
        let mut config: Config = toml::from_str(&content).map_err(|e| {
            Error::Io(std::io::Error::other(format!(
                "Invalid tinytown.toml: {}",
                e
            )))
        })?;
        config.root = root.to_path_buf();
        config.normalize_cli_references();

        Ok(config)
    }

    /// Get the CLI to use for the interactive conductor.
    #[must_use]
    pub fn conductor_cli_name(&self) -> &str {
        self.conductor_cli
            .as_deref()
            .filter(|value| !value.is_empty())
            .unwrap_or(&self.default_cli)
    }

    /// Resolve a configured CLI reference to a stable CLI name when possible.
    #[must_use]
    pub fn resolve_cli_name(&self, cli: &str) -> String {
        normalize_cli_reference(cli, &self.agent_clis)
    }

    /// Resolve a configured CLI reference to the command Tinytown should execute.
    #[must_use]
    pub fn resolve_cli_command(&self, cli: &str) -> String {
        let resolved = self.resolve_cli_name(cli);
        self.agent_clis
            .get(&resolved)
            .map(|cfg| cfg.command.clone())
            .unwrap_or_else(|| cli.trim().to_string())
    }

    /// Save configuration to the town directory.
    pub fn save(&self) -> Result<()> {
        let config_path = self.root.join(CONFIG_FILE);
        let content = toml::to_string_pretty(self).map_err(|e| {
            Error::Io(std::io::Error::other(format!(
                "Failed to serialize config: {}",
                e
            )))
        })?;
        std::fs::write(&config_path, content)?;
        Ok(())
    }

    fn normalize_cli_references(&mut self) {
        if let Some(normalized) = normalize_builtin_cli_reference(&self.default_cli)
            && self.default_cli != normalized
        {
            self.default_cli = normalized.to_string();
        }

        if let Some(current) = self.conductor_cli.as_deref()
            && let Some(normalized) = normalize_builtin_cli_reference(current)
            && current != normalized
        {
            self.conductor_cli = Some(normalized.to_string());
        }
    }

    /// Get the Redis socket path.
    #[must_use]
    pub fn socket_path(&self) -> PathBuf {
        // Ensure we have an absolute path for Redis
        let base = if self.root.is_absolute() {
            self.root.clone()
        } else {
            std::env::current_dir().unwrap_or_default().join(&self.root)
        };
        base.join(&self.redis.socket_path)
    }

    /// Get Redis connection URL.
    ///
    /// ⚠️ WARNING: When password is set, this URL contains credentials.
    /// Do NOT log the full URL. Use `redis_url_redacted()` for logging.
    #[must_use]
    pub fn redis_url(&self) -> String {
        if let Some(url) = self.explicit_redis_url() {
            url
        } else if self.redis.use_socket {
            format!("unix://{}", self.socket_path().display())
        } else {
            // Use rediss:// scheme for TLS, redis:// for plain TCP
            let scheme = if self.redis.tls_enabled {
                "rediss"
            } else {
                "redis"
            };

            // Check env var override for password (TINYTOWN_REDIS_PASSWORD takes precedence)
            let password = std::env::var("TINYTOWN_REDIS_PASSWORD")
                .ok()
                .or_else(|| self.redis.password.clone());

            // Include password in URL if configured
            match password {
                Some(pass) => {
                    format!(
                        "{}://:{}@{}:{}",
                        scheme, pass, self.redis.host, self.redis.port
                    )
                }
                None => format!("{}://{}:{}", scheme, self.redis.host, self.redis.port),
            }
        }
    }

    /// Get the Redis password, checking env var first.
    #[must_use]
    pub fn redis_password(&self) -> Option<String> {
        std::env::var("TINYTOWN_REDIS_PASSWORD")
            .ok()
            .or_else(|| self.redis.password.clone())
    }

    /// Get an explicit Redis URL override from the environment or config.
    #[must_use]
    pub fn explicit_redis_url(&self) -> Option<String> {
        std::env::var("REDIS_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                self.redis
                    .url
                    .clone()
                    .filter(|value| !value.trim().is_empty())
            })
    }

    /// Get a redacted version of the Redis URL safe for logging.
    /// Masks the password with **** if one is configured.
    #[must_use]
    pub fn redis_url_redacted(&self) -> String {
        if let Some(url) = self.explicit_redis_url() {
            Self::redact_redis_url(&url)
        } else if self.redis.use_socket {
            format!("unix://{}", self.socket_path().display())
        } else {
            let scheme = if self.redis.tls_enabled {
                "rediss"
            } else {
                "redis"
            };

            // Check if any password is set (env var or config)
            let has_password =
                std::env::var("TINYTOWN_REDIS_PASSWORD").is_ok() || self.redis.password.is_some();

            if has_password {
                format!("{}://:****@{}:{}", scheme, self.redis.host, self.redis.port)
            } else {
                format!("{}://{}:{}", scheme, self.redis.host, self.redis.port)
            }
        }
    }

    /// Check if Tinytown should treat Redis as externally managed.
    ///
    /// Any explicit URL override is treated as external, even if it points to
    /// localhost, because Tinytown should connect to it directly rather than
    /// trying to start its own redis-server.
    #[must_use]
    pub fn is_remote_redis(&self) -> bool {
        if self.explicit_redis_url().is_some() {
            return true;
        }

        !self.redis.use_socket && !Self::is_loopback_host(&self.redis.host)
    }

    /// Check if using central Redis (TCP on localhost with global config port).
    /// Central Redis is managed globally in ~/.tt/ rather than per-town.
    /// This uses the flag set at config creation time, avoiding re-reading global config.
    #[must_use]
    pub fn is_central_redis(&self) -> bool {
        self.use_central_redis
    }

    fn is_loopback_host(host: &str) -> bool {
        host == "localhost" || host == "::1" || host.starts_with("127.")
    }

    fn redact_redis_url(url: &str) -> String {
        let Ok(mut parsed) = Url::parse(url) else {
            return Self::redact_redis_url_fallback(url);
        };

        if parsed.password().is_some() {
            let _ = parsed.set_password(Some("****"));
        }

        parsed.to_string()
    }

    fn redact_redis_url_fallback(url: &str) -> String {
        let Some(scheme_idx) = url.find("://") else {
            return url.to_string();
        };

        let authority_start = scheme_idx + 3;
        let authority_end = url[authority_start..]
            .find(['/', '?', '#'])
            .map_or(url.len(), |idx| authority_start + idx);
        let authority = &url[authority_start..authority_end];

        let Some(at_idx) = authority.rfind('@') else {
            return url.to_string();
        };

        let userinfo = &authority[..at_idx];
        let redacted_userinfo = match userinfo.split_once(':') {
            Some(("", _)) => ":****".to_string(),
            Some((username, _)) => format!("{username}:****"),
            None => userinfo.to_string(),
        };

        format!(
            "{}{}{}{}",
            &url[..authority_start],
            redacted_userinfo,
            &authority[at_idx..],
            &url[authority_end..]
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn normalize_cli_reference_maps_interactive_codex_to_agent_preset() {
        let mut agent_clis = HashMap::new();
        agent_clis.insert(
            "codex".to_string(),
            AgentCli::new(
                "codex",
                "codex exec --dangerously-bypass-approvals-and-sandbox",
            ),
        );
        agent_clis.insert(
            "codex-mini".to_string(),
            AgentCli::new(
                "codex-mini",
                "codex exec --dangerously-bypass-approvals-and-sandbox -m gpt-5.4-mini -c model_reasoning_effort=\"medium\"",
            ),
        );

        assert_eq!(
            normalize_cli_reference(
                "codex --dangerously-bypass-approvals-and-sandbox",
                &agent_clis
            ),
            "codex"
        );
        assert_eq!(
            normalize_cli_reference(
                "codex --dangerously-bypass-approvals-and-sandbox -m gpt-5.4-mini",
                &agent_clis
            ),
            "codex-mini"
        );
    }

    #[test]
    fn load_normalizes_legacy_cli_references() -> std::result::Result<(), Box<dyn std::error::Error>>
    {
        let temp_dir = TempDir::new()?;
        let config_path = temp_dir.path().join("tinytown.toml");

        std::fs::write(
            &config_path,
            r#"
name = "legacy-cli-town"
default_cli = "codex --dangerously-bypass-approvals-and-sandbox"
conductor_cli = "codex exec --dangerously-bypass-approvals-and-sandbox -m gpt-5.4-mini -c model_reasoning_effort=\"medium\""
"#,
        )?;

        let config = Config::load(temp_dir.path())?;
        assert_eq!(config.default_cli, "codex");
        assert_eq!(config.conductor_cli.as_deref(), Some("codex-mini"));

        Ok(())
    }
}
