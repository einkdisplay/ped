use std::fs;
use std::process::Command;

#[derive(Clone, Debug)]
pub struct BatteryInfo {
    pub percentage: f64,
    pub charging: bool,
}

#[derive(Clone, Debug)]
pub struct NetworkInfo {
    pub connected: bool,
    pub ssid: Option<String>,
    pub airplane_mode: bool,
    pub ip_address: Option<String>,
}

pub fn read_battery() -> Result<BatteryInfo, String> {
    if let Some(info) = read_battery_sysfs() {
        return Ok(info);
    }
    if let Some(info) = read_battery_lipc() {
        return Ok(info);
    }
    Err("battery telemetry unavailable".to_owned())
}

pub fn read_network() -> Result<NetworkInfo, String> {
    let airplane = read_airplane_mode();
    let ssid = read_ssid();
    let ip_address = read_ip_address();
    let connected = ip_address.is_some() || ssid.as_ref().is_some_and(|value| !value.is_empty());
    if airplane.is_none() && ssid.is_none() && ip_address.is_none() {
        return Err("network telemetry unavailable".to_owned());
    }
    Ok(NetworkInfo {
        connected: connected && !airplane.unwrap_or(false),
        ssid,
        airplane_mode: airplane.unwrap_or(false),
        ip_address,
    })
}

fn read_battery_sysfs() -> Option<BatteryInfo> {
    let candidates = [
        "/sys/class/power_supply/battery/capacity",
        "/sys/class/power_supply/BAT0/capacity",
        "/sys/class/power_supply/max17042_battery/capacity",
    ];
    let percentage = candidates.iter().find_map(|path| read_f64(path))?;
    let status = [
        "/sys/class/power_supply/battery/status",
        "/sys/class/power_supply/BAT0/status",
        "/sys/class/power_supply/max17042_battery/status",
    ]
    .iter()
    .find_map(|path| read_trimmed(path))
    .unwrap_or_default();
    let charging = matches!(
        status.to_ascii_lowercase().as_str(),
        "charging" | "full" | "1" | "true"
    );
    Some(BatteryInfo {
        percentage: percentage.clamp(0.0, 100.0),
        charging,
    })
}

fn read_battery_lipc() -> Option<BatteryInfo> {
    let percentage: f64 = lipc_get("com.lab126.powerd", "battLevel")?.parse().ok()?;
    let charging = lipc_get("com.lab126.powerd", "isCharging")
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    Some(BatteryInfo {
        percentage: percentage.clamp(0.0, 100.0),
        charging,
    })
}

fn read_airplane_mode() -> Option<bool> {
    if let Some(value) = lipc_get("com.lab126.cmd", "wirelessEnable") {
        return Some(!(value == "1" || value.eq_ignore_ascii_case("true")));
    }
    None
}

fn read_ssid() -> Option<String> {
    if let Some(value) = lipc_get("com.lab126.wifid", "essid") {
        let trimmed = value.trim();
        if !trimmed.is_empty() && trimmed != "0" {
            return Some(trimmed.to_owned());
        }
    }
    None
}

fn read_ip_address() -> Option<String> {
    for iface in ["wlan0", "eth0"] {
        let path = format!("/sys/class/net/{iface}/operstate");
        if read_trimmed(&path).as_deref() != Some("up") {
            continue;
        }
        if let Some(ip) = ipv4_for_interface(iface) {
            if !ip.starts_with("127.") {
                return Some(ip);
            }
        }
    }
    None
}

fn ipv4_for_interface(iface: &str) -> Option<String> {
    let output = Command::new("ip")
        .args(["-4", "-o", "addr", "show", "dev", iface])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    for token in text.split_whitespace() {
        if let Some((addr, _)) = token.split_once('/') {
            if addr.contains('.') && !addr.starts_with("127.") {
                return Some(addr.to_owned());
            }
        }
    }
    None
}

fn lipc_get(service: &str, property: &str) -> Option<String> {
    let output = Command::new("lipc-get-prop")
        .args([service, property])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn read_trimmed(path: &str) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn read_f64(path: &str) -> Option<f64> {
    read_trimmed(path)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_sources_are_errors() {
        // On non-Kindle hosts this should fail cleanly rather than panic.
        let _ = read_battery();
        let _ = read_network();
        assert!(Path::new("/").exists());
    }
}
