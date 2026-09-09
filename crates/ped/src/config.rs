use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use url::Url;

const DEFAULT_PAGE: &str = "data:text/html,<body style='font-family:sans-serif'><h1>PED</h1><p>Configure a page in ped.toml.</p></body>";

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct Config {
    pub page: PageConfig,
    pub display: DisplayConfig,
    pub control: ControlConfig,
    pub lifecycle: LifecycleConfig,
    pub static_server: StaticServerConfig,
    #[serde(skip)]
    pub source_path: Option<PathBuf>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct PageConfig {
    pub url: String,
    pub script: Vec<PathBuf>,
    pub trusted_origins: Vec<String>,
    pub allowed_origins: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct DisplayConfig {
    pub auto_refresh: bool,
    pub refresh_interval_ms: u64,
    pub waveform: String,
    pub periodic_full_refresh: bool,
    pub full_refresh_interval: u32,
    pub wait_for_complete: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct ControlConfig {
    pub socket: PathBuf,
    pub max_script_bytes: usize,
    pub max_event_detail_bytes: usize,
    pub max_request_bytes: usize,
    pub queue_capacity: usize,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct LifecycleConfig {
    pub enabled: bool,
    pub marker: PathBuf,
    pub service: String,
    pub stop_timeout_ms: u64,
    pub watchdog_timeout_s: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct StaticServerConfig {
    pub enabled: bool,
    pub bind: String,
    pub port: u16,
    pub prefix: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            page: PageConfig::default(),
            display: DisplayConfig::default(),
            control: ControlConfig::default(),
            lifecycle: LifecycleConfig::default(),
            static_server: StaticServerConfig::default(),
            source_path: None,
        }
    }
}

impl Default for PageConfig {
    fn default() -> Self {
        Self {
            url: DEFAULT_PAGE.to_owned(),
            script: Vec::new(),
            trusted_origins: Vec::new(),
            allowed_origins: Vec::new(),
        }
    }
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            auto_refresh: true,
            refresh_interval_ms: 2500,
            waveform: "quality".to_owned(),
            periodic_full_refresh: true,
            full_refresh_interval: 20,
            wait_for_complete: false,
        }
    }
}

impl Default for ControlConfig {
    fn default() -> Self {
        Self {
            socket: PathBuf::from("/tmp/ped.sock"),
            max_script_bytes: 64 * 1024,
            max_event_detail_bytes: 32 * 1024,
            max_request_bytes: 128 * 1024,
            queue_capacity: 32,
        }
    }
}

impl Default for LifecycleConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            marker: PathBuf::from("/var/run/ped/session.json"),
            service: "lab126_gui".to_owned(),
            stop_timeout_ms: 3000,
            watchdog_timeout_s: 30,
        }
    }
}

impl Default for StaticServerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            bind: "127.0.0.1".to_owned(),
            port: 0,
            prefix: "/".to_owned(),
        }
    }
}

impl Config {
    pub fn load(path: Option<&Path>) -> Result<Self, String> {
        let Some(path) = path else {
            return Ok(Self::default());
        };

        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|error| format!("cannot resolve config path {}: {error}", path.display()))?
                .join(path)
        };
        let path = fs::canonicalize(&absolute).unwrap_or(absolute);

        let source = fs::read_to_string(&path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        let mut config: Self =
            toml::from_str(&source).map_err(|error| format!("invalid TOML: {error}"))?;
        config.resolve_paths(&path);
        config.source_path = Some(path);
        config.validate()?;
        Ok(config)
    }

    fn resolve_paths(&mut self, config_path: &Path) {
        let base = config_path.parent().unwrap_or_else(|| Path::new("."));
        for script in &mut self.page.script {
            if script.is_relative() {
                *script = base.join(&*script);
            }
        }
        if !self.page.url.contains("://") && !self.page.url.starts_with("data:") {
            let raw_path = PathBuf::from(&self.page.url);
            let page_path = if raw_path.is_absolute() {
                raw_path
            } else {
                base.join(raw_path)
            };
            let page_path = fs::canonicalize(&page_path).unwrap_or(page_path);
            match Url::from_file_path(&page_path) {
                Ok(page_url) => self.page.url = page_url.to_string(),
                Err(()) => {
                    // Keep the absolute path text so validate() can report a clear error.
                    self.page.url = page_path.display().to_string();
                }
            }
        }
    }

    fn validate(&self) -> Result<(), String> {
        if self.display.refresh_interval_ms == 0 {
            return Err("display.refresh_interval_ms must be greater than zero".to_owned());
        }
        if self.control.max_script_bytes == 0 {
            return Err("control.max_script_bytes must be greater than zero".to_owned());
        }
        if self.control.max_event_detail_bytes == 0 {
            return Err("control.max_event_detail_bytes must be greater than zero".to_owned());
        }
        if self.control.max_request_bytes < self.control.max_script_bytes
            || self.control.max_request_bytes < self.control.max_event_detail_bytes
        {
            return Err(
                "control.max_request_bytes must cover script and event detail limits".to_owned(),
            );
        }
        if self.control.queue_capacity == 0 {
            return Err("control.queue_capacity must be greater than zero".to_owned());
        }
        if self.display.full_refresh_interval == 0 {
            return Err("display.full_refresh_interval must be greater than zero".to_owned());
        }
        if self.static_server.enabled
            && (!self.static_server.bind.starts_with("127.")
                && self.static_server.bind != "localhost")
        {
            return Err("static_server.bind must remain loopback-only".to_owned());
        }
        let page_url =
            Url::parse(&self.page.url).map_err(|error| format!("invalid page.url: {error}"))?;
        for origin in self
            .page
            .trusted_origins
            .iter()
            .chain(self.page.allowed_origins.iter())
        {
            let parsed =
                Url::parse(origin).map_err(|error| format!("invalid origin {origin}: {error}"))?;
            if parsed.host_str().is_none()
                || (!parsed.path().is_empty() && parsed.path() != "/")
                || parsed.query().is_some()
                || parsed.fragment().is_some()
            {
                return Err(format!(
                    "origin must contain only scheme and host: {origin}"
                ));
            }
        }
        if page_url.scheme() == "file" && !self.static_server.enabled {
            return Err("file:// pages require static_server.enabled".to_owned());
        }
        Ok(())
    }

    pub fn page_url(&self) -> Result<Url, String> {
        Url::parse(&self.page.url).map_err(|error| format!("invalid page.url: {error}"))
    }

    pub fn is_url_allowed(&self, candidate: &Url, initial: &Url) -> bool {
        if self.page.allowed_origins.is_empty() {
            return candidate.origin() == initial.origin();
        }
        self.page
            .allowed_origins
            .iter()
            .any(|origin| origin_matches(origin, candidate))
    }

    pub fn is_origin_trusted(&self, candidate: &Url) -> bool {
        if self.page.trusted_origins.is_empty() {
            return matches!(candidate.scheme(), "http" | "https" | "file")
                && candidate
                    .host_str()
                    .is_none_or(|host| host == "127.0.0.1" || host == "localhost");
        }
        self.page.trusted_origins.iter().any(|origin| origin_matches(origin, candidate))
    }

    pub fn apply_runtime_reload(&mut self, next: Config) -> Result<ReloadReport, String> {
        let mut report = ReloadReport::default();
        if self.control.socket != next.control.socket
            || self.static_server.bind != next.static_server.bind
            || self.static_server.port != next.static_server.port
            || self.static_server.enabled != next.static_server.enabled
            || self.static_server.prefix != next.static_server.prefix
            || self.page.url != next.page.url
            || self.page.script != next.page.script
            || self.lifecycle.enabled != next.lifecycle.enabled
            || self.lifecycle.marker != next.lifecycle.marker
            || self.lifecycle.service != next.lifecycle.service
            || self.control.queue_capacity != next.control.queue_capacity
        {
            report.restart_required = true;
            report.messages.push(
                "socket/static-server/page/lifecycle/queue changes require restart".to_owned(),
            );
            return Ok(report);
        }

        self.display = next.display;
        self.page.trusted_origins = next.page.trusted_origins;
        self.page.allowed_origins = next.page.allowed_origins;
        self.control.max_script_bytes = next.control.max_script_bytes;
        self.control.max_event_detail_bytes = next.control.max_event_detail_bytes;
        self.control.max_request_bytes = next.control.max_request_bytes;
        self.lifecycle.stop_timeout_ms = next.lifecycle.stop_timeout_ms;
        self.lifecycle.watchdog_timeout_s = next.lifecycle.watchdog_timeout_s;
        report.applied = true;
        report
            .messages
            .push("reloaded display/control/origin runtime settings".to_owned());
        Ok(report)
    }
}

#[derive(Clone, Debug, Default)]
pub struct ReloadReport {
    pub applied: bool,
    pub restart_required: bool,
    pub messages: Vec<String>,
}


fn origin_matches(configured: &str, candidate: &Url) -> bool {
    let Ok(allowed) = Url::parse(configured) else {
        return false;
    };
    if allowed.scheme() != candidate.scheme() {
        return false;
    }
    if allowed.host_str() != candidate.host_str() {
        return false;
    }
    match (allowed.port(), candidate.port()) {
        (None, _) => true,
        (Some(expected), Some(actual)) => expected == actual,
        (Some(expected), None) => candidate.port_or_known_default() == Some(expected),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        let config = Config::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn invalid_loopback_binding_is_rejected() {
        let mut config = Config::default();
        config.static_server.bind = "0.0.0.0".to_owned();
        assert!(config.validate().is_err());
    }

    #[test]
    fn url_policy_defaults_to_initial_origin() {
        let config = Config::default();
        let initial = Url::parse("http://127.0.0.1:1234/index.html").unwrap();
        assert!(config.is_url_allowed(&initial, &initial));
        assert!(!config.is_url_allowed(&Url::parse("http://example.com/").unwrap(), &initial));
    }
}
