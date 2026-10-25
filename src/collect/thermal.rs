//! hwmon sensors and thermal zones.

use crate::util::{read, read_f64};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Temp,
    Fan,
    Volt,
    Curr,
    Power,
}

impl Kind {
    pub fn unit(self) -> &'static str {
        match self {
            Kind::Temp => "°C",
            Kind::Fan => "RPM",
            Kind::Volt => "V",
            Kind::Curr => "A",
            Kind::Power => "W",
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Kind::Temp => "Temperature",
            Kind::Fan => "Fan",
            Kind::Volt => "Voltage",
            Kind::Curr => "Current",
            Kind::Power => "Power",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Sensor {
    pub chip: String,
    pub label: String,
    pub kind: Kind,
    pub value: f64,
    pub max: Option<f64>,
    pub crit: Option<f64>,
}

#[derive(Clone, Debug, Default)]
pub struct ThermalSnap {
    pub sensors: Vec<Sensor>,
    pub zones: Vec<(String, f64)>,
    pub cooling: Vec<(String, u64, u64)>,
}

impl ThermalSnap {
    pub fn hottest(&self) -> Option<&Sensor> {
        self.sensors
            .iter()
            .filter(|s| s.kind == Kind::Temp)
            .max_by(|a, b| a.value.partial_cmp(&b.value).unwrap_or(std::cmp::Ordering::Equal))
    }
    pub fn temps(&self) -> impl Iterator<Item = &Sensor> {
        self.sensors.iter().filter(|s| s.kind == Kind::Temp)
    }
    pub fn fans(&self) -> impl Iterator<Item = &Sensor> {
        self.sensors.iter().filter(|s| s.kind == Kind::Fan)
    }
}

/// Static sensor metadata (paths, labels, limits) discovered once; only values are re-read per tick.
struct SensorDef {
    chip: String,
    label: String,
    kind: Kind,
    div: f64,
    input: std::path::PathBuf,
    max: Option<f64>,
    crit: Option<f64>,
}

pub struct ThermalSampler {
    defs: Vec<SensorDef>,
    tick: u64,
}

impl ThermalSampler {
    pub fn new() -> Self {
        ThermalSampler { defs: discover(), tick: 0 }
    }

    pub fn sample(&mut self) -> ThermalSnap {
        self.tick += 1;
        if self.tick.is_multiple_of(60) {
            self.defs = discover(); // pick up hot-plugged USB sensors etc.
        }
        let mut snap = ThermalSnap::default();
        for d in &self.defs {
            let Some(raw) = read_f64(&d.input) else { continue };
            snap.sensors.push(Sensor {
                chip: d.chip.clone(),
                label: d.label.clone(),
                kind: d.kind,
                value: raw / d.div,
                max: d.max,
                crit: d.crit,
            });
        }
        zones(&mut snap);
        snap
    }
}

fn discover() -> Vec<SensorDef> {
    let mut defs = Vec::new();
    if let Ok(rd) = std::fs::read_dir("/sys/class/hwmon") {
        let mut dirs: Vec<_> = rd.flatten().map(|e| e.path()).collect();
        dirs.sort();
        let mut seen: Vec<String> = Vec::new();
        for p in dirs {
            let mut chip =
                read(p.join("name")).unwrap_or_else(|| p.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default());
            let dups = seen.iter().filter(|c| **c == chip).count();
            seen.push(chip.clone());
            if dups > 0 {
                chip = format!("{} #{}", chip, dups + 1);
            }
            let Ok(files) = std::fs::read_dir(&p) else { continue };
            let mut inputs: Vec<String> =
                files.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|f| f.ends_with("_input")).collect();
            inputs.sort();
            for f in inputs {
                let stem = f.trim_end_matches("_input");
                let (kind, div) = if stem.starts_with("temp") {
                    (Kind::Temp, 1000.0)
                } else if stem.starts_with("fan") {
                    (Kind::Fan, 1.0)
                } else if stem.starts_with("in") {
                    (Kind::Volt, 1000.0)
                } else if stem.starts_with("curr") {
                    (Kind::Curr, 1000.0)
                } else if stem.starts_with("power") {
                    (Kind::Power, 1_000_000.0)
                } else {
                    continue;
                };
                if read_f64(p.join(&f)).is_none() {
                    continue;
                }
                let label = read(p.join(format!("{}_label", stem))).unwrap_or_else(|| format!("{} {}", chip, stem));
                let max = read_f64(p.join(format!("{}_max", stem))).map(|v| v / div);
                let crit = read_f64(p.join(format!("{}_crit", stem))).map(|v| v / div);
                defs.push(SensorDef { chip: chip.clone(), label, kind, div, input: p.join(&f), max, crit });
            }
        }
    }
    defs
}

fn zones(snap: &mut ThermalSnap) {
    if let Ok(rd) = std::fs::read_dir("/sys/class/thermal") {
        let mut dirs: Vec<_> = rd.flatten().map(|e| e.path()).collect();
        dirs.sort();
        for p in dirs {
            let name = p.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
            if name.starts_with("thermal_zone") {
                if let (Some(t), Some(v)) = (read(p.join("type")), read_f64(p.join("temp"))) {
                    snap.zones.push((t, v / 1000.0));
                }
            } else if name.starts_with("cooling_device") {
                if let (Some(t), Some(c), Some(m)) = (read(p.join("type")), read_f64(p.join("cur_state")), read_f64(p.join("max_state"))) {
                    snap.cooling.push((t, c as u64, m as u64));
                }
            }
        }
    }
}
