use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Root Upstart job that owns Xorg/awesome/blanket (`lxinit`).
/// Stopping it cascades to `lab126_gui` and the Java framework.
const KINDLE_X_SERVICE: &str = "x";
/// App/framework umbrella job. Manual `start x` does **not** revive this after
/// boot because `lab126_gui` waits on one-shot `n_ready` + `langpicker_ready`.
const KINDLE_GUI_SERVICE: &str = "lab126_gui";

#[derive(Clone, Debug)]
pub struct LifecycleConfig {
    pub enabled: bool,
    pub marker: PathBuf,
    /// Root service stopped for fb0 takeover. Prefer `x` over `lab126_gui`.
    pub service: String,
    pub stop_timeout: Duration,
    pub watchdog_timeout: Duration,
}

#[derive(Debug, Serialize, Deserialize)]
struct SessionMarker {
    pid: u32,
    service: String,
    started_at: u64,
    heartbeat_at: u64,
    watchdog_timeout_s: u64,
}

pub struct LifecycleGuard {
    config: LifecycleConfig,
    active: bool,
    stop_heartbeat: Option<Arc<AtomicBool>>,
    heartbeat_thread: Option<JoinHandle<()>>,
}

impl LifecycleGuard {
    pub fn acquire(config: LifecycleConfig) -> Result<Self, String> {
        let mut guard = Self {
            config,
            active: false,
            stop_heartbeat: None,
            heartbeat_thread: None,
        };
        if !guard.config.enabled {
            return Ok(guard);
        }
        guard.validate_environment()?;
        guard.write_marker(true)?;
        if let Err(error) = guard.stop_service() {
            let _ = guard.restore_service();
            let _ = guard.remove_marker();
            return Err(error);
        }
        guard.active = true;
        guard.start_heartbeat();
        Ok(guard)
    }

    fn validate_environment(&self) -> Result<(), String> {
        if !Path::new("/dev/fb0").exists() {
            return Err("Kindle framebuffer /dev/fb0 is unavailable".to_owned());
        }
        if self.config.service.trim().is_empty() {
            return Err("Kindle UI service name is empty".to_owned());
        }
        if let Some(parent) = self.config.marker.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("cannot create lifecycle marker directory: {error}"))?;
        }
        Ok(())
    }

    fn write_marker(&self, initial: bool) -> Result<(), String> {
        let now = unix_now();
        let existing = if !initial && self.config.marker.exists() {
            fs::read_to_string(&self.config.marker)
                .ok()
                .and_then(|text| serde_json::from_str::<SessionMarker>(&text).ok())
        } else {
            None
        };
        let marker = SessionMarker {
            pid: std::process::id(),
            service: self.config.service.clone(),
            started_at: existing.map(|marker| marker.started_at).unwrap_or(now),
            heartbeat_at: now,
            watchdog_timeout_s: self.config.watchdog_timeout.as_secs().max(1),
        };
        let encoded = serde_json::to_vec_pretty(&marker)
            .map_err(|error| format!("cannot encode lifecycle marker: {error}"))?;
        let temporary = self.config.marker.with_extension("tmp");
        fs::write(&temporary, encoded)
            .map_err(|error| format!("cannot write lifecycle marker: {error}"))?;
        fs::rename(&temporary, &self.config.marker)
            .map_err(|error| format!("cannot install lifecycle marker: {error}"))
    }

    fn start_heartbeat(&mut self) {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = stop.clone();
        let marker = self.config.marker.clone();
        let service = self.config.service.clone();
        let timeout_s = self.config.watchdog_timeout.as_secs().max(1);
        let interval = Duration::from_secs((timeout_s / 3).max(1));
        let thread = thread::Builder::new()
            .name("ped-heartbeat".to_owned())
            .spawn(move || {
                while !stop_thread.load(Ordering::Acquire) {
                    let now = unix_now();
                    let payload = SessionMarker {
                        pid: std::process::id(),
                        service: service.clone(),
                        started_at: now,
                        heartbeat_at: now,
                        watchdog_timeout_s: timeout_s,
                    };
                    if let Ok(encoded) = serde_json::to_vec_pretty(&payload) {
                        let temporary = marker.with_extension("tmp");
                        let _ = fs::write(&temporary, encoded)
                            .and_then(|_| fs::rename(&temporary, &marker));
                    }
                    thread::sleep(interval);
                }
            })
            .ok();
        self.stop_heartbeat = Some(stop);
        self.heartbeat_thread = thread;
    }

    fn stop_service(&self) -> Result<(), String> {
        // Prefer stopping the configured root job. Default is `x`, which also
        // tears down lab126_gui / framework / pillow via Upstart edges. Stopping
        // only lab126_gui leaves Xorg+awesome+blanket alive (title bar clock,
        // touch routing, etc.).
        ensure_service_stopped(&self.config.service, self.config.stop_timeout)
    }

    fn restore_service(&self) -> Result<(), String> {
        restore_kindle_ui(self.config.stop_timeout)
    }

    fn remove_marker(&self) -> Result<(), String> {
        match fs::remove_file(&self.config.marker) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("cannot remove lifecycle marker: {error}")),
        }
    }

    pub fn release(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        if let Some(stop) = self.stop_heartbeat.take() {
            stop.store(true, Ordering::Release);
        }
        if let Some(thread) = self.heartbeat_thread.take() {
            let _ = thread.join();
        }
        if !self.active {
            return;
        }
        self.active = false;
        if let Err(error) = self.restore_service() {
            eprintln!("ped: lifecycle recovery failed: {error}");
            // Keep marker so an external watchdog can still attempt recovery.
            return;
        }
        if let Err(error) = self.remove_marker() {
            eprintln!("ped: lifecycle marker cleanup failed: {error}");
        }
    }

    pub fn recover_stale_marker(config: &LifecycleConfig) -> Result<(), String> {
        if !config.marker.exists() {
            return Ok(());
        }
        let contents = fs::read_to_string(&config.marker).unwrap_or_default();
        let marker: Option<SessionMarker> = serde_json::from_str(&contents).ok();
        let stale = match &marker {
            Some(marker) => {
                let age = unix_now().saturating_sub(marker.heartbeat_at);
                let timeout = marker.watchdog_timeout_s.max(1);
                age > timeout || !process_exists(marker.pid)
            }
            None => true,
        };
        if !stale && config.enabled {
            // Another PED instance appears alive; refuse to clobber it.
            return Err(format!(
                "active PED lifecycle marker already present at {}",
                config.marker.display()
            ));
        }
        let _ = restore_kindle_ui(config.stop_timeout);
        match fs::remove_file(&config.marker) {
            Ok(()) | Err(_) => Ok(()),
        }
    }
}

impl Drop for LifecycleGuard {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn restore_kindle_ui(timeout: Duration) -> Result<(), String> {
    // Full UI bring-up is two-step on modern FW:
    //   1) start x            -> Xorg / awesome / blanket / winmgr
    //   2) start lab126_gui   -> framework / pillow / chrome / apps
    // `start x` alone never re-emits boot-only `n_ready`, so lab126_gui stays down
    // and the user can sit on the tree progress bar with no framework.
    ensure_service_started(KINDLE_X_SERVICE, timeout)?;
    ensure_service_started(KINDLE_GUI_SERVICE, timeout)?;
    // Framework/CVM can take tens of seconds after lab126_gui is "running".
    // Wait best-effort so splash teardown has a chance to finish; do not fail
    // restore if lipc is slow (marker cleanup still happens).
    let _ = wait_for_framework_started(timeout.saturating_mul(4).max(Duration::from_secs(90)));
    let _ = start_home_app();
    Ok(())
}

fn ensure_service_stopped(service: &str, timeout: Duration) -> Result<(), String> {
    if !service_is_running(service) {
        return Ok(());
    }
    match run_service_command("stop", service) {
        Ok(()) => {}
        Err(error) if !service_is_running(service) => {
            // Already stopped races as "Unknown instance".
            let _ = error;
        }
        Err(error) => return Err(error),
    }
    wait_until(timeout, || !service_is_running(service))
        .map_err(|_| format!("Kindle UI service {service} did not stop before timeout"))
}

fn ensure_service_started(service: &str, timeout: Duration) -> Result<(), String> {
    if service_is_running(service) {
        return Ok(());
    }
    match run_service_command("start", service) {
        Ok(()) => {}
        Err(error) if service_is_running(service) => {
            // Already running races as "Job is already running".
            let _ = error;
        }
        Err(error) => return Err(error),
    }
    wait_until(timeout, || service_is_running(service))
        .map_err(|_| format!("Kindle UI service {service} did not restart before timeout"))
}

fn run_service_command(action: &str, service: &str) -> Result<(), String> {
    let output = Command::new(format!("/sbin/{action}"))
        .arg(service)
        .output()
        .map_err(|error| format!("cannot run /sbin/{action} {service}: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail = format!("{stdout}{stderr}").trim().to_owned();
    if action == "start" && detail.contains("already running") {
        return Ok(());
    }
    if action == "stop"
        && (detail.contains("Unknown instance") || detail.contains("already been stopped"))
    {
        return Ok(());
    }
    if detail.is_empty() {
        Err(format!(
            "/sbin/{action} {service} failed with {}",
            output.status
        ))
    } else {
        Err(format!(
            "/sbin/{action} {service} failed with {}: {detail}",
            output.status
        ))
    }
}

fn service_is_running(service: &str) -> bool {
    Command::new("/sbin/status")
        .arg(service)
        .output()
        .map(|output| {
            let text = String::from_utf8_lossy(&output.stdout);
            text.contains("start/running")
        })
        .unwrap_or(false)
}

fn wait_until(timeout: Duration, mut predicate: impl FnMut() -> bool) -> Result<(), ()> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if predicate() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    if predicate() { Ok(()) } else { Err(()) }
}

fn wait_for_framework_started(timeout: Duration) -> bool {
    wait_until(timeout, framework_is_started).is_ok()
}

fn framework_is_started() -> bool {
    // lipc-get-prop prints the value on stdout when successful.
    Command::new("lipc-get-prop")
        .args(["-eiq", "com.lab126.kaf", "frameworkStarted"])
        .output()
        .map(|output| {
            output.status.success() && String::from_utf8_lossy(&output.stdout).trim().eq("1")
        })
        .unwrap_or(false)
}

fn start_home_app() -> Result<(), String> {
    let status = Command::new("lipc-set-prop")
        .args([
            "com.lab126.appmgrd",
            "start",
            "app://com.lab126.booklet.home",
        ])
        .status()
        .map_err(|error| format!("cannot request home app: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("lipc home start failed with {status}"))
    }
}

fn process_exists(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_lifecycle_is_a_noop() {
        let _guard = LifecycleGuard::acquire(LifecycleConfig {
            enabled: false,
            marker: PathBuf::from("/tmp/ped-test-marker"),
            service: KINDLE_X_SERVICE.to_owned(),
            stop_timeout: Duration::from_millis(1),
            watchdog_timeout: Duration::from_secs(30),
        })
        .unwrap();
    }
}
