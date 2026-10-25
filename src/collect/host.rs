//! Static host facts gathered once at startup.

use crate::util::read;

#[derive(Clone, Debug, Default)]
pub struct HostInfo {
    pub hostname: String,
    pub user: String,
    pub os: String,
    pub kernel: String,
    pub arch: String,
    pub model: String,
    pub cpu_model: String,
    pub desktop: String,
    pub boot_epoch: u64,
}

impl HostInfo {
    pub fn collect() -> Self {
        let hostname = read("/proc/sys/kernel/hostname").unwrap_or_default();
        let user = std::env::var("USER").unwrap_or_default();
        let os = read("/etc/os-release")
            .and_then(|s| {
                s.lines()
                    .find(|l| l.starts_with("PRETTY_NAME="))
                    .map(|l| l.trim_start_matches("PRETTY_NAME=").trim_matches('"').to_string())
            })
            .unwrap_or_else(|| "Linux".into());
        let kernel = read("/proc/sys/kernel/osrelease").unwrap_or_default();
        let arch = std::env::consts::ARCH.to_string();
        let model = read("/proc/device-tree/model")
            .map(|s| s.trim_end_matches('\0').to_string())
            .or_else(|| {
                let v = read("/sys/class/dmi/id/sys_vendor").unwrap_or_default();
                let p = read("/sys/class/dmi/id/product_name").unwrap_or_default();
                let joined = format!("{} {}", v, p).trim().to_string();
                if joined.is_empty() {
                    None
                } else {
                    Some(joined)
                }
            })
            .unwrap_or_else(|| "Unknown machine".into());
        let cpu_model = cpu_model_name();
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").or_else(|_| std::env::var("DESKTOP_SESSION")).unwrap_or_default();
        let boot_epoch = read("/proc/stat")
            .and_then(|s| s.lines().find(|l| l.starts_with("btime")).and_then(|l| l.split_whitespace().nth(1)).and_then(|v| v.parse().ok()))
            .unwrap_or(0);
        HostInfo { hostname, user, os, kernel, arch, model, cpu_model, desktop, boot_epoch }
    }

    pub fn uptime_secs(&self) -> u64 {
        crate::util::read("/proc/uptime")
            .and_then(|s| s.split_whitespace().next().and_then(|v| v.parse::<f64>().ok()))
            .map(|v| v as u64)
            .unwrap_or(0)
    }
}

fn cpu_model_name() -> String {
    let info = read("/proc/cpuinfo").unwrap_or_default();
    if let Some(l) = info.lines().find(|l| l.starts_with("model name")) {
        if let Some((_, v)) = l.split_once(':') {
            return v.trim().to_string();
        }
    }
    if let Some(l) = info.lines().find(|l| l.starts_with("Hardware")) {
        if let Some((_, v)) = l.split_once(':') {
            return v.trim().to_string();
        }
    }
    // ARM: use device-tree compatible strings of the cpu nodes (e.g. apple,icestorm).
    let mut kinds: Vec<String> = Vec::new();
    let mut i = 0;
    while let Some(c) = read(format!("/sys/devices/system/cpu/cpu{}/of_node/compatible", i)) {
        let c = c.trim_end_matches('\0').to_string();
        if !kinds.contains(&c) {
            kinds.push(c);
        }
        i += 1;
        if i > 512 {
            break;
        }
    }
    if !kinds.is_empty() {
        let vendor = kinds[0].split(',').next().unwrap_or("").to_string();
        let names: Vec<String> = kinds
            .iter()
            .map(|k| {
                let n = k.split(',').nth(1).unwrap_or(k);
                let mut cs = n.chars();
                match cs.next() {
                    Some(f) => f.to_uppercase().collect::<String>() + cs.as_str(),
                    None => String::new(),
                }
            })
            .collect();
        let mut v = vendor.chars();
        let vendor = match v.next() {
            Some(f) => f.to_uppercase().collect::<String>() + v.as_str(),
            None => String::new(),
        };
        let per: Vec<String> = names
            .iter()
            .map(|n| {
                let count = (0..512)
                    .filter(|i| {
                        read(format!("/sys/devices/system/cpu/cpu{}/of_node/compatible", i))
                            .map(|c| c.trim_end_matches('\0').ends_with(&n.to_lowercase()))
                            .unwrap_or(false)
                    })
                    .count();
                format!("{}×{}", count, n)
            })
            .collect();
        return format!("{} ({})", vendor, per.join(" + "));
    }
    let impl_id = info.lines().find(|l| l.starts_with("CPU implementer")).and_then(|l| l.split(':').nth(1)).map(|s| s.trim().to_string());
    match impl_id.as_deref() {
        Some("0x41") => "ARM".into(),
        Some("0x61") => "Apple".into(),
        Some("0x51") => "Qualcomm".into(),
        Some(x) => format!("ARM implementer {}", x),
        None => "Unknown CPU".into(),
    }
}
