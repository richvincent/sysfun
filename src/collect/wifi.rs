//! Wi-Fi link, station, scan and PHY details via nl80211 (`iw`) and /proc/net/wireless.

use crate::util::{first_float, first_int, kv, read, run, which};
use std::time::Instant;

#[derive(Clone, Debug, Default)]
pub struct Bss {
    pub ssid: String,
    pub bssid: String,
    pub freq: u32,
    pub chan: u32,
    pub signal: f64,
    pub security: String,
    pub width: String,
    pub gen: String,
    pub stations: Option<u32>,
    pub util_pct: Option<f64>,
    pub associated: bool,
    pub last_seen_ms: Option<u64>,
    pub country: Option<String>,
    pub has_ht: bool,
    pub has_vht: bool,
    pub has_he: bool,
    pub has_eht: bool,
    pub wps: bool,
    pub beacon_int: Option<u32>,
    pub tx_power: Option<i32>,
}

#[derive(Clone, Debug, Default)]
pub struct WifiLink {
    pub iface: String,
    pub mac: String,
    pub phy: String,
    pub mode: String,
    pub driver: Option<String>,
    pub connected: bool,
    pub ssid: String,
    pub bssid: String,
    pub freq: u32,
    pub chan: u32,
    pub width: String,
    pub center1: Option<u32>,
    pub signal_dbm: Option<i32>,
    pub signal_chains: Vec<i32>,
    pub signal_avg: Option<i32>,
    pub noise_dbm: Option<i32>,
    pub link_quality: Option<(u32, u32)>,
    pub rx_bitrate: String,
    pub tx_bitrate: String,
    pub rx_mbps: f64,
    pub tx_mbps: f64,
    pub expected_mbps: Option<f64>,
    pub txpower_dbm: Option<f64>,
    pub connected_secs: Option<u64>,
    pub inactive_ms: Option<u64>,
    pub tx_failed: Option<u64>,
    pub tx_retries: Option<u64>,
    pub rx_drop_misc: Option<u64>,
    pub beacon_loss: Option<u64>,
    pub beacon_rx: Option<u64>,
    pub rx_bytes: Option<u64>,
    pub tx_bytes: Option<u64>,
    pub rx_packets: Option<u64>,
    pub tx_packets: Option<u64>,
    pub dtim: Option<u32>,
    pub beacon_int: Option<u32>,
    pub authorized: bool,
    pub authenticated: bool,
    pub associated: bool,
    pub wmm: bool,
    pub mfp: bool,
    pub power_save: Option<bool>,
    pub disc_retry: Option<u64>,
    pub disc_misc: Option<u64>,
    pub disc_crypt: Option<u64>,
    pub missed_beacon: Option<u64>,
    pub security: String,
    pub last_ack_signal: Option<i32>,
    pub avg_ack_signal: Option<i32>,
}

impl WifiLink {
    pub fn band(&self) -> &'static str {
        band_of(self.freq)
    }
    pub fn snr(&self) -> Option<i32> {
        match (self.signal_dbm, self.noise_dbm) {
            (Some(s), Some(n)) if n > -200 && n < 0 => Some(s - n),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct PhyCaps {
    pub bands: Vec<String>,
    pub ht: bool,
    pub vht: bool,
    pub he: bool,
    pub eht: bool,
    pub modes: Vec<String>,
    pub ciphers: usize,
    pub antennas: Option<String>,
    pub max_scan_ssids: Option<u32>,
    pub channels: usize,
}

#[derive(Clone, Debug, Default)]
pub struct WifiSnap {
    pub ifaces: Vec<String>,
    pub link: Option<WifiLink>,
    pub scan: Vec<Bss>,
    pub phy: Option<PhyCaps>,
    pub regdom: Option<String>,
    pub scan_age_s: Option<f64>,
    pub iw_available: bool,
    pub chan_load: Vec<(u32, usize)>,
}

pub struct WifiSampler {
    iw_ok: bool,
    nmcli_ok: bool,
    force_scan_ticks: u32,
    info_cache: Option<(String, WifiLink)>,
    phy: Option<PhyCaps>,
    regdom: Option<String>,
    scan: Vec<Bss>,
    last_scan: Option<Instant>,
    tick: u64,
}

pub fn band_of(freq: u32) -> &'static str {
    match freq {
        2400..=2500 => "2.4 GHz",
        5150..=5925 => "5 GHz",
        5926..=7125 => "6 GHz",
        57000..=71000 => "60 GHz",
        0 => "",
        _ => "?",
    }
}

pub fn chan_of(freq: u32) -> u32 {
    match freq {
        2484 => 14,
        2412..=2472 => (freq - 2407) / 5,
        5150..=5925 => (freq - 5000) / 5,
        5955..=7115 => (freq - 5950) / 5,
        _ => 0,
    }
}

pub fn wifi_ifaces() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir("/sys/class/net")
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| {
                    std::path::Path::new(&format!("/sys/class/net/{}/phy80211", n)).exists()
                        || std::path::Path::new(&format!("/sys/class/net/{}/wireless", n)).exists()
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

impl WifiSampler {
    pub fn new() -> Self {
        WifiSampler {
            iw_ok: which("iw"),
            nmcli_ok: which("nmcli"),
            force_scan_ticks: 0,
            info_cache: None,
            phy: None,
            regdom: None,
            scan: Vec::new(),
            last_scan: None,
            tick: 0,
        }
    }

    /// Ask NetworkManager (or the kernel, if we are root) for a fresh scan; results land on later ticks.
    pub fn rescan(&mut self) {
        let iface = wifi_ifaces().into_iter().next();
        let mut asked = false;
        if self.nmcli_ok {
            asked = run("nmcli", &["dev", "wifi", "rescan"]).is_some();
        }
        if !asked && self.iw_ok {
            if let Some(i) = iface {
                let _ = run("iw", &["dev", &i, "scan", "trigger"]);
            }
        }
        // Re-read the caches on each of the next few ticks while results trickle in.
        self.force_scan_ticks = 6;
    }

    pub fn sample(&mut self) -> WifiSnap {
        self.tick += 1;
        let ifaces = wifi_ifaces();
        let mut snap = WifiSnap { ifaces: ifaces.clone(), iw_available: self.iw_ok, ..Default::default() };
        let Some(iface) = ifaces.first().cloned() else { return snap };
        let mut link = WifiLink { iface: iface.clone(), ..Default::default() };
        link.mac = read(format!("/sys/class/net/{}/address", iface)).unwrap_or_default();
        link.driver = std::fs::read_link(format!("/sys/class/net/{}/device/driver", iface))
            .ok()
            .and_then(|p| p.file_name().map(|f| f.to_string_lossy().into_owned()));
        link.phy = read(format!("/sys/class/net/{}/phy80211/name", iface)).unwrap_or_default();
        parse_proc_wireless(&iface, &mut link);

        if self.iw_ok {
            if let Some(out) = run("iw", &["dev", &iface, "link"]) {
                parse_link(&out, &mut link);
            }
            // `iw info` (channel/width/txpower/mode) and power-save state change rarely: refresh every 10 ticks,
            // or immediately when the BSSID changes.
            let bssid_changed = self.info_cache.as_ref().map(|(b, _)| *b != link.bssid).unwrap_or(true);
            if bssid_changed || self.tick % 10 == 1 || self.info_cache.is_none() {
                let mut cached = WifiLink::default();
                if let Some(out) = run("iw", &["dev", &iface, "info"]) {
                    parse_info(&out, &mut cached);
                }
                if let Some(out) = run("iw", &["dev", &iface, "get", "power_save"]) {
                    cached.power_save = Some(out.to_ascii_lowercase().contains("on"));
                }
                self.info_cache = Some((link.bssid.clone(), cached));
            }
            if let Some((_, c)) = &self.info_cache {
                link.chan = c.chan;
                link.width = c.width.clone();
                link.center1 = c.center1;
                link.txpower_dbm = c.txpower_dbm;
                link.mode = c.mode.clone();
                link.power_save = c.power_save;
                if link.freq == 0 {
                    link.freq = c.freq;
                }
                if link.phy.is_empty() {
                    link.phy = c.phy.clone();
                }
            }
            if link.connected {
                if let Some(out) = run("iw", &["dev", &iface, "station", "dump"]) {
                    parse_station(&out, &mut link);
                }
            }
            let need_scan = self.force_scan_ticks > 0 || self.last_scan.map(|t| t.elapsed().as_secs_f64() > 5.0).unwrap_or(true);
            self.force_scan_ticks = self.force_scan_ticks.saturating_sub(1);
            if need_scan {
                if let Some(out) = run("iw", &["dev", &iface, "scan", "dump"]) {
                    let mut scan = parse_scan(&out);
                    if self.nmcli_ok {
                        if let Some(nm) = run(
                            "nmcli",
                            &[
                                "-t",
                                "-f",
                                "ACTIVE,SSID,BSSID,MODE,CHAN,FREQ,RATE,SIGNAL,SECURITY,WPA-FLAGS,RSN-FLAGS",
                                "dev",
                                "wifi",
                                "list",
                                "--rescan",
                                "no",
                            ],
                        ) {
                            for b in parse_nmcli(&nm) {
                                if !scan.iter().any(|x| x.bssid.eq_ignore_ascii_case(&b.bssid)) {
                                    scan.push(b);
                                }
                            }
                        }
                    }
                    self.scan = scan;
                    self.last_scan = Some(Instant::now());
                }
            }
            if self.phy.is_none() {
                let phy = if link.phy.is_empty() { "phy0".to_string() } else { link.phy.clone() };
                if let Some(out) = run("iw", &["phy", &phy, "info"]) {
                    self.phy = Some(parse_phy(&phy, &out));
                }
            }
            if self.regdom.is_none() || self.tick.is_multiple_of(60) {
                self.regdom = run("iw", &["reg", "get"])
                    .and_then(|o| o.lines().find(|l| l.starts_with("country")).map(|l| l.trim_end_matches(':').to_string()));
            }
        }
        if link.chan == 0 && link.freq > 0 {
            link.chan = chan_of(link.freq);
        }
        // Security + BSS load for the associated network come from the scan cache.
        let mut scan = self.scan.clone();
        for b in scan.iter_mut() {
            b.associated = link.connected && b.bssid.eq_ignore_ascii_case(&link.bssid);
            if b.associated {
                link.security = b.security.clone();
                if link.width.is_empty() {
                    link.width = b.width.clone();
                }
            }
        }
        scan.sort_by(|a, b| b.associated.cmp(&a.associated).then(b.signal.partial_cmp(&a.signal).unwrap_or(std::cmp::Ordering::Equal)));
        let mut load: Vec<(u32, usize)> = Vec::new();
        for b in &scan {
            if let Some(e) = load.iter_mut().find(|(c, _)| *c == b.chan) {
                e.1 += 1;
            } else {
                load.push((b.chan, 1));
            }
        }
        load.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        snap.chan_load = load;
        snap.scan_age_s = self.last_scan.map(|t| t.elapsed().as_secs_f64());
        snap.scan = scan;
        snap.phy = self.phy.clone();
        snap.regdom = self.regdom.clone();
        snap.link = Some(link);
        snap
    }
}

fn parse_proc_wireless(iface: &str, link: &mut WifiLink) {
    let Some(s) = read("/proc/net/wireless") else { return };
    for l in s.lines().skip(2) {
        let f: Vec<&str> = l.split_whitespace().collect();
        if f.len() < 11 || f[0].trim_end_matches(':') != iface {
            continue;
        }
        let num = |i: usize| f.get(i).map(|v| v.trim_end_matches('.')).and_then(|v| v.parse::<i64>().ok());
        if let Some(q) = num(2) {
            link.link_quality = Some((q.max(0) as u32, 70));
        }
        if link.signal_dbm.is_none() {
            link.signal_dbm = num(3).map(|v| v as i32);
        }
        if let Some(n) = num(4) {
            if n > -200 && n < 0 {
                link.noise_dbm = Some(n as i32);
            }
        }
        link.disc_crypt = num(6).map(|v| v as u64);
        link.disc_retry = num(8).map(|v| v as u64);
        link.disc_misc = num(9).map(|v| v as u64);
        link.missed_beacon = num(10).map(|v| v as u64);
    }
}

fn bitrate_mbps(s: &str) -> f64 {
    first_float(s).unwrap_or(0.0)
}

fn parse_link(out: &str, link: &mut WifiLink) {
    for line in out.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("Connected to ") {
            link.connected = true;
            link.bssid = rest.split_whitespace().next().unwrap_or("").to_string();
            continue;
        }
        if t.starts_with("Not connected") {
            link.connected = false;
            continue;
        }
        let Some((k, v)) = kv(t) else { continue };
        match k.as_str() {
            "ssid" => link.ssid = v,
            "freq" => link.freq = first_float(&v).unwrap_or(0.0) as u32,
            "signal" => link.signal_dbm = first_int(&v).map(|x| x as i32),
            "rx bitrate" => {
                link.rx_mbps = bitrate_mbps(&v);
                link.rx_bitrate = v;
            }
            "tx bitrate" => {
                link.tx_mbps = bitrate_mbps(&v);
                link.tx_bitrate = v;
            }
            "dtim period" => link.dtim = first_int(&v).map(|x| x as u32),
            "beacon int" => link.beacon_int = first_int(&v).map(|x| x as u32),
            "rx" => {
                link.rx_bytes = first_int(&v).map(|x| x as u64);
                link.rx_packets = v.split('(').nth(1).and_then(first_int).map(|x| x as u64);
            }
            "tx" => {
                link.tx_bytes = first_int(&v).map(|x| x as u64);
                link.tx_packets = v.split('(').nth(1).and_then(first_int).map(|x| x as u64);
            }
            _ => {}
        }
    }
}

fn parse_info(out: &str, link: &mut WifiLink) {
    for line in out.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("channel ") {
            // channel 40 (5200 MHz), width: 20 MHz, center1: 5200 MHz
            link.chan = first_int(rest).unwrap_or(0) as u32;
            if link.freq == 0 {
                link.freq = rest.split('(').nth(1).and_then(first_int).unwrap_or(0) as u32;
            }
            if let Some(w) = rest.split("width:").nth(1) {
                link.width = w.split(',').next().unwrap_or("").trim().to_string();
            }
            if let Some(c) = rest.split("center1:").nth(1) {
                link.center1 = first_int(c).map(|x| x as u32);
            }
        } else if let Some(rest) = t.strip_prefix("txpower ") {
            link.txpower_dbm = first_float(rest);
        } else if let Some(rest) = t.strip_prefix("type ") {
            link.mode = rest.trim().to_string();
        } else if let Some(rest) = t.strip_prefix("wiphy ") {
            if link.phy.is_empty() {
                link.phy = format!("phy{}", rest.trim());
            }
        }
    }
}

fn parse_station(out: &str, link: &mut WifiLink) {
    for line in out.lines() {
        let t = line.trim();
        let Some((k, v)) = kv(t) else { continue };
        let yes = v.eq_ignore_ascii_case("yes");
        let u = || first_int(&v).map(|x| x.max(0) as u64);
        match k.as_str() {
            "authorized" => link.authorized = yes,
            "authenticated" => link.authenticated = yes,
            "associated" => link.associated = yes,
            "wmm/wme" => link.wmm = yes,
            "mfp" => link.mfp = yes,
            "inactive time" => link.inactive_ms = u(),
            "rx bytes" => link.rx_bytes = u(),
            "tx bytes" => link.tx_bytes = u(),
            "rx packets" => link.rx_packets = u(),
            "tx packets" => link.tx_packets = u(),
            "tx failed" => link.tx_failed = u(),
            "tx retries" => link.tx_retries = u(),
            "rx drop misc" => link.rx_drop_misc = u(),
            "beacon loss" => link.beacon_loss = u(),
            "beacon rx" => link.beacon_rx = u(),
            "connected time" => link.connected_secs = u(),
            "expected throughput" => link.expected_mbps = first_float(&v),
            "last ack signal" => link.last_ack_signal = first_int(&v).map(|x| x as i32),
            "avg ack signal" => link.avg_ack_signal = first_int(&v).map(|x| x as i32),
            "signal" => {
                link.signal_dbm = first_int(&v).map(|x| x as i32);
                link.signal_chains = bracket_ints(&v);
            }
            "signal avg" => link.signal_avg = first_int(&v).map(|x| x as i32),
            "tx bitrate" => {
                link.tx_mbps = bitrate_mbps(&v);
                link.tx_bitrate = v;
            }
            "rx bitrate" => {
                link.rx_mbps = bitrate_mbps(&v);
                link.rx_bitrate = v;
            }
            "dtim period" => link.dtim = first_int(&v).map(|x| x as u32),
            "beacon interval" => link.beacon_int = first_int(&v).map(|x| x as u32),
            _ => {}
        }
    }
}

fn bracket_ints(v: &str) -> Vec<i32> {
    let Some(start) = v.find('[') else { return vec![] };
    let Some(end) = v[start..].find(']') else { return vec![] };
    v[start + 1..start + end].split(',').filter_map(|x| x.trim().parse().ok()).collect()
}

fn parse_scan(out: &str) -> Vec<Bss> {
    let mut list: Vec<Bss> = Vec::new();
    let mut cur: Option<Bss> = None;
    let mut rsn_auth: Vec<String> = Vec::new();
    let mut wpa = false;
    let mut rsn = false;
    let mut privacy = false;
    let mut section = String::new();
    let mut ht_width = String::new();
    let mut vht_width = String::new();
    let finish = |b: Option<Bss>, list: &mut Vec<Bss>, rsn: bool, wpa: bool, privacy: bool, auth: &[String], ht_w: &str, vht_w: &str| {
        if let Some(mut b) = b {
            b.security = security_label(rsn, wpa, privacy, auth);
            if b.chan == 0 {
                b.chan = chan_of(b.freq);
            }
            b.gen = gen_label(&b);
            b.width = if !vht_w.is_empty() {
                vht_w.to_string()
            } else if !ht_w.is_empty() {
                ht_w.to_string()
            } else {
                "20 MHz".into()
            };
            if let Some(e) = list.iter_mut().find(|x| x.bssid == b.bssid) {
                *e = b;
            } else {
                list.push(b);
            }
        }
    };
    for line in out.lines() {
        if let Some(rest) = line.strip_prefix("BSS ") {
            finish(cur.take(), &mut list, rsn, wpa, privacy, &rsn_auth, &ht_width, &vht_width);
            rsn_auth.clear();
            wpa = false;
            rsn = false;
            privacy = false;
            section.clear();
            ht_width.clear();
            vht_width.clear();
            let bssid = rest.split(['(', ' ']).next().unwrap_or("").to_string();
            cur = Some(Bss { bssid, associated: rest.contains("associated"), ..Default::default() });
            continue;
        }
        let Some(b) = cur.as_mut() else { continue };
        let t = line.trim();
        if !line.starts_with("\t\t") && !line.starts_with("    ") {
            // top-level attribute of the BSS: track which section we're in
            section = t.split(':').next().unwrap_or("").to_string();
            if let Some(v) = t.strip_prefix("SSID: ") {
                b.ssid = v.to_string();
            } else if t == "SSID:" {
                b.ssid.clear();
            } else if let Some(v) = t.strip_prefix("freq: ") {
                b.freq = first_float(v).unwrap_or(0.0) as u32;
            } else if let Some(v) = t.strip_prefix("signal: ") {
                b.signal = first_float(v).unwrap_or(0.0);
            } else if let Some(v) = t.strip_prefix("last seen: ") {
                if v.contains("ms ago") {
                    b.last_seen_ms = first_int(v).map(|x| x as u64);
                }
            } else if let Some(v) = t.strip_prefix("DS Parameter set: channel ") {
                b.chan = first_int(v).unwrap_or(0) as u32;
            } else if let Some(v) = t.strip_prefix("beacon interval: ") {
                b.beacon_int = first_int(v).map(|x| x as u32);
            } else if let Some(v) = t.strip_prefix("capability: ") {
                privacy = v.contains("Privacy");
            } else if let Some(v) = t.strip_prefix("Country: ") {
                b.country = v.split_whitespace().next().map(|s| s.to_string());
            } else if t.starts_with("HT capabilities") {
                b.has_ht = true;
            } else if t.starts_with("VHT capabilities") {
                b.has_vht = true;
            } else if t.starts_with("HE capabilities") {
                b.has_he = true;
            } else if t.starts_with("EHT capabilities") {
                b.has_eht = true;
            } else if t.starts_with("RSN:") {
                rsn = true;
            } else if t.starts_with("WPA:") {
                wpa = true;
            } else if t.starts_with("WPS:") {
                b.wps = true;
            } else if let Some(v) = t.strip_prefix("TPC report: TX power: ") {
                b.tx_power = first_int(v).map(|x| x as i32);
            }
            continue;
        }
        // nested attributes
        let t = t.trim_start_matches('*').trim();
        if let Some(v) = t.strip_prefix("station count: ") {
            b.stations = first_int(v).map(|x| x as u32);
        } else if let Some(v) = t.strip_prefix("channel utilisation: ") {
            if let Some(n) = first_int(v) {
                b.util_pct = Some(n as f64 / 255.0 * 100.0);
            }
        } else if let Some(v) = t.strip_prefix("Authentication suites: ") {
            if section.starts_with("RSN") || section.starts_with("WPA") {
                rsn_auth.extend(v.split_whitespace().map(|s| s.to_string()));
            }
        } else if let Some(v) = t.strip_prefix("STA channel width: ") {
            // HT operation: "20 MHz" or "any"
            ht_width = if v.starts_with("any") { "40 MHz".into() } else { v.trim().to_string() };
        } else if let Some(v) = t.strip_prefix("channel width: ") {
            // VHT/HE operation: "1 (80 MHz)", "0 (20 or 40 MHz)" (0 defers to HT)
            if !v.starts_with('0') {
                if let Some(w) = v.split('(').nth(1) {
                    vht_width = w.trim_end_matches(')').trim().to_string();
                }
            }
        } else if let Some(v) = t.strip_prefix("secondary channel offset: ") {
            if ht_width.is_empty() {
                ht_width = if v.starts_with("no") { "20 MHz".into() } else { "40 MHz".into() };
            }
        } else if let Some(v) = t.strip_prefix("primary channel: ") {
            if b.chan == 0 {
                b.chan = first_int(v).unwrap_or(0) as u32;
            }
        }
    }
    finish(cur.take(), &mut list, rsn, wpa, privacy, &rsn_auth, &ht_width, &vht_width);
    list
}

/// NetworkManager keeps its own (usually much larger) scan cache; merge it in without rescanning.
fn parse_nmcli(out: &str) -> Vec<Bss> {
    let mut list = Vec::new();
    for line in out.lines() {
        // Fields are ':'-separated with literal colons escaped as "\:".
        let mut fields: Vec<String> = Vec::new();
        let mut cur = String::new();
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\\' {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            } else if c == ':' {
                fields.push(std::mem::take(&mut cur));
            } else {
                cur.push(c);
            }
        }
        fields.push(cur);
        if fields.len() < 11 {
            continue;
        }
        let freq = first_int(&fields[5]).unwrap_or(0) as u32;
        let rate = first_int(&fields[6]).unwrap_or(0);
        let pct = first_int(&fields[7]).unwrap_or(0) as f64;
        let sec = fields[8].as_str();
        let rsn = fields[10].to_ascii_lowercase();
        let security = if sec.is_empty() {
            "Open".to_string()
        } else if sec.contains("WEP") {
            "WEP".to_string()
        } else {
            let mut parts: Vec<&str> = Vec::new();
            if sec.contains("WPA3") || rsn.contains("sae") {
                parts.push("WPA3");
            }
            if sec.contains("WPA2") {
                parts.push("WPA2");
            }
            if sec.contains("WPA1") {
                parts.push("WPA");
            }
            if sec.contains("802.1X") {
                parts.push("Enterprise");
            }
            if parts.is_empty() {
                parts.push(sec);
            }
            parts.join("/")
        };
        let band = band_of(freq);
        // Max supported rate hints at the generation when iw has no IEs for this BSS.
        let gen = if rate >= 1200 {
            "~Wi-Fi 6"
        } else if band != "2.4 GHz" && rate >= 400 {
            "~Wi-Fi 5"
        } else if rate >= 72 {
            "~Wi-Fi 4"
        } else {
            "—"
        };
        let mut b = Bss {
            ssid: fields[1].clone(),
            bssid: fields[2].to_ascii_lowercase(),
            freq,
            chan: first_int(&fields[4]).unwrap_or(0) as u32,
            signal: (pct / 2.0 - 100.0).clamp(-100.0, -20.0),
            security,
            width: "—".into(),
            gen: gen.into(),
            associated: fields[0] == "yes",
            ..Default::default()
        };
        if b.chan == 0 {
            b.chan = chan_of(freq);
        }
        list.push(b);
    }
    list
}

fn security_label(rsn: bool, wpa: bool, privacy: bool, auth: &[String]) -> String {
    let has = |s: &str| auth.iter().any(|a| a.contains(s));
    if rsn || wpa {
        let mut parts: Vec<&str> = Vec::new();
        if has("OWE") {
            return "Enhanced Open (OWE)".into();
        }
        if has("SAE") {
            parts.push("WPA3");
        }
        if has("PSK") {
            parts.push(if rsn { "WPA2" } else { "WPA" });
        }
        if has("802.1X") || has("EAP") {
            parts.push("Enterprise");
        }
        if parts.is_empty() {
            parts.push(if rsn { "WPA2" } else { "WPA" });
        }
        parts.dedup();
        parts.join("/")
    } else if privacy {
        "WEP".into()
    } else {
        "Open".into()
    }
}

fn gen_label(b: &Bss) -> String {
    let band = band_of(b.freq);
    if b.has_eht {
        "Wi-Fi 7".into()
    } else if b.has_he {
        if band == "6 GHz" {
            "Wi-Fi 6E".into()
        } else {
            "Wi-Fi 6".into()
        }
    } else if b.has_vht {
        "Wi-Fi 5".into()
    } else if b.has_ht {
        "Wi-Fi 4".into()
    } else if band == "2.4 GHz" {
        "802.11b/g".into()
    } else {
        "802.11a".into()
    }
}

fn parse_phy(name: &str, out: &str) -> PhyCaps {
    let _ = name;
    let mut p = PhyCaps::default();
    let mut in_modes = false;
    let mut in_ciphers = false;
    let mut in_freqs = false;
    for line in out.lines() {
        let t = line.trim();
        let depth = line.len() - line.trim_start().len();
        if t.starts_with("Supported interface modes") {
            in_modes = true;
            in_ciphers = false;
            continue;
        }
        if t.starts_with("Supported Ciphers") {
            in_ciphers = true;
            in_modes = false;
            continue;
        }
        if t.starts_with("Frequencies:") {
            in_freqs = true;
            continue;
        }
        if t.starts_with('*') {
            let v = t.trim_start_matches('*').trim();
            if in_modes {
                p.modes.push(v.to_string());
                continue;
            }
            if in_ciphers {
                p.ciphers += 1;
                continue;
            }
            if in_freqs {
                if let Some(f) = first_float(v) {
                    if !v.contains("disabled") {
                        p.channels += 1;
                    }
                    let b = band_of(f as u32);
                    if !b.is_empty() && b != "?" && !p.bands.iter().any(|x| x == b) {
                        p.bands.push(b.to_string());
                    }
                }
                continue;
            }
        } else if depth <= 2 || !t.starts_with('*') {
            if in_modes && depth <= 1 {
                in_modes = false;
            }
            if in_ciphers && depth <= 1 {
                in_ciphers = false;
            }
            if in_freqs && !t.starts_with('*') && depth <= 2 {
                in_freqs = false;
            }
        }
        if t.contains("HT20/HT40") || t.starts_with("HT TX/RX MCS") {
            p.ht = true;
        }
        if t.starts_with("VHT Capabilities") || t.starts_with("VHT RX MCS") {
            p.vht = true;
        }
        if t.starts_with("HE Iftypes") || t.starts_with("HE PHY Capabilities") {
            p.he = true;
        }
        if t.starts_with("EHT Iftypes") || t.starts_with("EHT PHY Capabilities") {
            p.eht = true;
        }
        if let Some(v) = t.strip_prefix("Available Antennas: ") {
            p.antennas = Some(v.to_string());
        }
        if let Some(v) = t.strip_prefix("max # scan SSIDs: ") {
            p.max_scan_ssids = v.trim().parse().ok();
        }
    }
    p.bands.sort();
    p
}
