use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
pub struct LifecycleConfig {
    pub enabled: bool,
    pub marker: PathBuf,
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
        run_service_command("stop", &self.config.service)?;
        let deadline = Instant::now() + self.config.stop_timeout;
        while Instant::now() < deadline {
            if !service_is_running(&self.config.service) {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(100));
        }
        Err(format!(
            "Kindle UI service {} did not stop before timeout",
            self.config.service
        ))
    }

    fn restore_service(&self) -> Result<(), String> {
        run_service_command("start", &self.config.service)?;
        let deadline = Instant::now() + self.config.stop_timeout;
        while Instant::now() < deadline {
            if service_is_running(&self.config.service) {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(100));
        }
        Err(format!(
            "Kindle UI service {} did not restart before timeout",
            self.config.service
        ))
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
        let _ = run_service_command("start", &config.service);
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

fn run_service_command(action: &str, service: &str) -> Result<(), String> {
    let status = Command::new(format!("/sbin/{action}"))
        .arg(service)
        .status()
        .map_err(|error| format!("cannot run /sbin/{action} {service}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("/sbin/{action} {service} failed with {status}"))
    }
}

fn service_is_running(service: &str) -> bool {
    Command::new("/sbin/status")
        .arg(service)
        .output()
        .map(|output| {
            let text = String::from_utf8_lossy(&output.stdout);
            output.status.success() && text.contains("start/running")
        })
        .unwrap_or(false)
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
        let guard = LifecycleGuard::acquire(LifecycleConfig {
            enabled: false,
            marker: PathBuf::from("/tmp/ped-test-marker"),
            service: "lab126_gui".to_owned(),
            stop_timeout: Duration::from_millis(1),
            watchdog_timeout: Duration::from_secs(30),
        })
        .unwrap();
        assert!(true); // disabled acquire is a noop
    }
}
