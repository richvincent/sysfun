//! Batteries, AC/USB supplies and platform power settings.

use crate::util::{read, read_f64, read_u64};
use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
pub struct Battery {
    pub name: String,
    pub present: bool,
    pub status: String,
    pub capacity_pct: Option<f64>,
    pub capacity_level: Option<String>,
    pub health: Option<String>,
    pub cycles: Option<u32>,
    pub voltage_v: Option<f64>,
    pub voltage_min_design_v: Option<f64>,
    pub voltage_max_design_v: Option<f64>,
    pub current_a: Option<f64>,
    pub power_w: Option<f64>,
    pub energy_now_wh: Option<f64>,
    pub energy_full_wh: Option<f64>,
    pub energy_full_design_wh: Option<f64>,
    pub charge_now_ah: Option<f64>,
    pub charge_full_ah: Option<f64>,
    pub charge_full_design_ah: Option<f64>,
    pub health_pct: Option<f64>,
    pub time_to_empty_s: Option<u64>,
    pub time_to_full_s: Option<u64>,
    pub time_estimated: bool,
    pub temp_c: Option<f64>,
    pub model: Option<String>,
    pub manufacturer: Option<String>,
    pub serial: Option<String>,
    pub technology: Option<String>,
    pub manufactured: Option<String>,
    pub start_threshold: Option<u32>,
    pub end_threshold: Option<u32>,
    pub charge_behaviour: Option<String>,
    pub charge_term_current_a: Option<f64>,
    pub const_charge_current_max_a: Option<f64>,
    pub const_charge_voltage_v: Option<f64>,
}

impl Battery {
    pub fn is_charging(&self) -> bool {
        self.status.eq_ignore_ascii_case("charging")
    }
    pub fn is_discharging(&self) -> bool {
        self.status.eq_ignore_ascii_case("discharging")
    }
}

#[derive(Clone, Debug, Default)]
pub struct Supply {
    pub name: String,
    pub kind: String,
    pub online: bool,
    pub voltage_v: Option<f64>,
    pub current_a: Option<f64>,
    pub power_w: Option<f64>,
    pub input_power_limit_w: Option<f64>,
    pub usb_type: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct PowerSnap {
    pub batteries: Vec<Battery>,
    pub supplies: Vec<Supply>,
    pub on_ac: bool,
    pub governor: Option<String>,
    pub epp: Option<String>,
    pub platform_profile: Option<String>,
    pub backlight_pct: Option<f64>,
    pub rails: Vec<(String, f64)>,
    pub system_power_w: Option<f64>,
    pub lid_open: Option<bool>,
}

fn uevent(path: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    if let Some(s) = read(format!("{}/uevent", path)) {
        for l in s.lines() {
            if let Some((k, v)) = l.split_once('=') {
                m.insert(k.trim_start_matches("POWER_SUPPLY_").to_ascii_lowercase(), v.to_string());
            }
        }
    }
    m
}

fn micro(m: &HashMap<String, String>, k: &str) -> Option<f64> {
    m.get(k)?.parse::<f64>().ok().map(|v| v / 1_000_000.0)
}

pub struct PowerSampler {
    rails: Vec<(String, std::path::PathBuf)>,
    tick: u64,
}

impl PowerSampler {
    pub fn new() -> Self {
        PowerSampler { rails: discover_rails(), tick: 0 }
    }
    pub fn sample(&mut self) -> PowerSnap {
        self.tick += 1;
        if self.tick.is_multiple_of(60) {
            self.rails = discover_rails();
        }
        let mut snap = sample_supplies();
        for (label, path) in &self.rails {
            let Some(uw) = read_u64(path) else { continue };
            let w = uw as f64 / 1_000_000.0;
            let l = label.to_ascii_lowercase();
            if l.contains("total system") || l.contains("package") || l.contains("psys") {
                snap.system_power_w = Some(w);
            }
            snap.rails.push((label.clone(), w));
        }
        snap
    }
}

/// hwmon power*_input files that are not part of a battery chip.
fn discover_rails() -> Vec<(String, std::path::PathBuf)> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir("/sys/class/hwmon") else { return out };
    let mut dirs: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    dirs.sort();
    for p in dirs {
        let chip = read(p.join("name")).unwrap_or_default();
        if chip.contains("battery") || chip.contains("_bat") {
            continue;
        }
        let Ok(files) = std::fs::read_dir(&p) else { continue };
        let mut names: Vec<String> = files
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|f| f.starts_with("power") && f.ends_with("_input"))
            .collect();
        names.sort();
        for f in names {
            let stem = f.trim_end_matches("_input");
            let label = read(p.join(format!("{}_label", stem))).unwrap_or_else(|| format!("{} {}", chip, stem));
            out.push((label, p.join(&f)));
        }
    }
    out
}

fn sample_supplies() -> PowerSnap {
    let mut snap = PowerSnap::default();
    let Ok(rd) = std::fs::read_dir("/sys/class/power_supply") else { return snap };
    let mut names: Vec<String> = rd.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    for name in names {
        let path = format!("/sys/class/power_supply/{}", name);
        let m = uevent(&path);
        let kind = m.get("type").cloned().unwrap_or_default();
        if kind == "Battery" {
            // Skip peripherals like mice/headsets (scope=Device) in the main list.
            if m.get("scope").map(|s| s == "Device").unwrap_or(false) {
                continue;
            }
            let status = m.get("status").cloned().unwrap_or_else(|| "Unknown".into());
            let voltage_v = micro(&m, "voltage_now");
            let current_a = micro(&m, "current_now").map(|v| v.abs());
            let mut power_w = micro(&m, "power_now").map(|v| v.abs());
            if power_w.is_none() {
                if let (Some(v), Some(i)) = (voltage_v, current_a) {
                    power_w = Some(v * i);
                }
            }
            let mut energy_now_wh = micro(&m, "energy_now");
            let mut energy_full_wh = micro(&m, "energy_full");
            let mut energy_full_design_wh = micro(&m, "energy_full_design");
            let charge_now_ah = micro(&m, "charge_now");
            let charge_full_ah = micro(&m, "charge_full");
            let charge_full_design_ah = micro(&m, "charge_full_design");
            let vref = micro(&m, "voltage_min_design").or(voltage_v).unwrap_or(0.0);
            if energy_now_wh.is_none() {
                energy_now_wh = charge_now_ah.map(|c| c * vref);
            }
            if energy_full_wh.is_none() {
                energy_full_wh = charge_full_ah.map(|c| c * vref);
            }
            if energy_full_design_wh.is_none() {
                energy_full_design_wh = charge_full_design_ah.map(|c| c * vref);
            }
            let health_pct = match (charge_full_ah, charge_full_design_ah, energy_full_wh, energy_full_design_wh) {
                (Some(f), Some(d), _, _) if d > 0.0 => Some(f / d * 100.0),
                (_, _, Some(f), Some(d)) if d > 0.0 => Some(f / d * 100.0),
                _ => None,
            };
            let mut capacity_pct = m.get("capacity").and_then(|v| v.parse().ok());
            if capacity_pct.is_none() {
                if let (Some(n), Some(f)) = (energy_now_wh, energy_full_wh) {
                    if f > 0.0 {
                        capacity_pct = Some(n / f * 100.0);
                    }
                }
            }
            let mut time_to_empty_s = m.get("time_to_empty_now").and_then(|v| v.parse::<u64>().ok()).filter(|v| *v > 0);
            let mut time_to_full_s = m.get("time_to_full_now").and_then(|v| v.parse::<u64>().ok()).filter(|v| *v > 0);
            let mut estimated = false;
            if let Some(p) = power_w.filter(|p| *p > 0.5) {
                if time_to_empty_s.is_none() && status.eq_ignore_ascii_case("discharging") {
                    if let Some(e) = energy_now_wh {
                        time_to_empty_s = Some((e / p * 3600.0) as u64);
                        estimated = true;
                    }
                }
                if time_to_full_s.is_none() && status.eq_ignore_ascii_case("charging") {
                    if let (Some(e), Some(f)) = (energy_now_wh, energy_full_wh) {
                        time_to_full_s = Some(((f - e).max(0.0) / p * 3600.0) as u64);
                        estimated = true;
                    }
                }
            }
            let manufactured = match (m.get("manufacture_year"), m.get("manufacture_month"), m.get("manufacture_day")) {
                (Some(y), Some(mo), Some(d)) => Some(format!("{}-{:0>2}-{:0>2}", y, mo, d)),
                (Some(y), Some(mo), None) => Some(format!("{}-{:0>2}", y, mo)),
                _ => None,
            };
            snap.batteries.push(Battery {
                name: name.clone(),
                present: m.get("present").map(|v| v == "1").unwrap_or(true),
                status,
                capacity_pct,
                capacity_level: m.get("capacity_level").cloned(),
                health: m.get("health").cloned(),
                cycles: m.get("cycle_count").and_then(|v| v.parse().ok()),
                voltage_v,
                voltage_min_design_v: micro(&m, "voltage_min_design"),
                voltage_max_design_v: micro(&m, "voltage_max_design"),
                current_a,
                power_w,
                energy_now_wh,
                energy_full_wh,
                energy_full_design_wh,
                charge_now_ah,
                charge_full_ah,
                charge_full_design_ah,
                health_pct,
                time_to_empty_s,
                time_to_full_s,
                time_estimated: estimated,
                temp_c: m.get("temp").and_then(|v| v.parse::<f64>().ok()).map(|v| v / 10.0),
                model: m.get("model_name").cloned(),
                manufacturer: m.get("manufacturer").cloned(),
                serial: m.get("serial_number").cloned(),
                technology: m.get("technology").cloned(),
                manufactured,
                start_threshold: m.get("charge_control_start_threshold").and_then(|v| v.parse().ok()),
                end_threshold: m.get("charge_control_end_threshold").and_then(|v| v.parse().ok()),
                charge_behaviour: m.get("charge_behaviour").cloned(),
                charge_term_current_a: micro(&m, "charge_term_current"),
                const_charge_current_max_a: micro(&m, "constant_charge_current_max"),
                const_charge_voltage_v: micro(&m, "constant_charge_voltage"),
            });
        } else {
            let online = m.get("online").map(|v| v == "1").unwrap_or(false);
            if online && kind != "Battery" {
                snap.on_ac = true;
            }
            let voltage_v = micro(&m, "voltage_now").filter(|v| *v > 0.0);
            let current_a = micro(&m, "current_now").filter(|v| *v > 0.0);
            let power_w = micro(&m, "power_now").or(match (voltage_v, current_a) {
                (Some(v), Some(i)) => Some(v * i),
                _ => None,
            });
            snap.supplies.push(Supply {
                name: name.clone(),
                kind: kind.clone(),
                online,
                voltage_v,
                current_a,
                power_w,
                input_power_limit_w: micro(&m, "input_power_limit").filter(|v| *v > 0.0),
                usb_type: m.get("usb_type").and_then(|t| {
                    t.split_whitespace().find(|x| x.starts_with('[')).map(|x| x.trim_matches(|c| c == '[' || c == ']').to_string())
                }),
            });
        }
    }
    if snap
        .batteries
        .iter()
        .any(|b| b.is_charging() || b.status.eq_ignore_ascii_case("full") || b.status.eq_ignore_ascii_case("not charging"))
    {
        snap.on_ac = true;
    }
    snap.governor = read("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor");
    snap.epp = read("/sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference");
    snap.platform_profile = read("/sys/firmware/acpi/platform_profile");
    if let Ok(rd) = std::fs::read_dir("/sys/class/backlight") {
        for e in rd.flatten() {
            let p = e.path();
            if let (Some(b), Some(m)) = (read_f64(p.join("brightness")), read_f64(p.join("max_brightness"))) {
                if m > 0.0 {
                    snap.backlight_pct = Some(b / m * 100.0);
                }
            }
        }
    }
    snap.lid_open =
        read("/proc/acpi/button/lid/LID0/state").or_else(|| read("/proc/acpi/button/lid/LID/state")).map(|s| s.contains("open"));
    snap
}
