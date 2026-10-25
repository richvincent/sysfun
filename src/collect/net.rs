//! Network interfaces, addresses, routes, DNS and socket statistics.

use crate::util::{read, read_u64};
use std::collections::HashMap;
use std::ffi::CStr;
use std::net::{Ipv4Addr, Ipv6Addr};

#[derive(Clone, Debug, Default)]
pub struct Iface {
    pub name: String,
    pub kind: String,
    pub state: String,
    pub carrier: bool,
    pub mac: String,
    pub mtu: u32,
    pub speed_mbps: Option<u32>,
    pub duplex: Option<String>,
    pub driver: Option<String>,
    pub ipv4: Vec<String>,
    pub ipv6: Vec<String>,
    pub rx_bps: f64,
    pub tx_bps: f64,
    pub rx_pps: f64,
    pub tx_pps: f64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub rx_errors: u64,
    pub tx_errors: u64,
    pub rx_dropped: u64,
    pub tx_dropped: u64,
    pub is_default: bool,
}

#[derive(Clone, Debug, Default)]
pub struct SockStat {
    pub used: u64,
    pub tcp_inuse: u64,
    pub tcp_orphan: u64,
    pub tcp_tw: u64,
    pub tcp_alloc: u64,
    pub udp_inuse: u64,
}

#[derive(Clone, Debug, Default)]
pub struct TcpStats {
    pub cur_estab: u64,
    pub retrans_per_s: f64,
    pub in_segs_per_s: f64,
    pub out_segs_per_s: f64,
    pub in_errs: u64,
    pub active_opens: u64,
    pub passive_opens: u64,
}

#[derive(Clone, Debug, Default)]
pub struct NetSnap {
    pub ifaces: Vec<Iface>,
    pub default_v4: Option<(String, String)>,
    pub default_v6: Option<(String, String)>,
    pub dns: Vec<String>,
    pub dns_upstream: Vec<String>,
    pub search: Vec<String>,
    pub sockets: Option<SockStat>,
    pub tcp: Option<TcpStats>,
    pub total_rx_bps: f64,
    pub total_tx_bps: f64,
}

#[derive(Clone, Copy, Default)]
struct Raw {
    rx_bytes: u64,
    tx_bytes: u64,
    rx_packets: u64,
    tx_packets: u64,
}

pub struct NetSampler {
    prev: HashMap<String, Raw>,
    prev_tcp: (u64, u64, u64),
}

impl NetSampler {
    pub fn new() -> Self {
        let mut s = NetSampler { prev: HashMap::new(), prev_tcp: (0, 0, 0) };
        for n in iface_names() {
            s.prev.insert(n.clone(), raw_stats(&n));
        }
        if let Some(t) = snmp_tcp() {
            s.prev_tcp = (t.0, t.1, t.2);
        }
        s
    }

    pub fn sample(&mut self, dt: f64) -> NetSnap {
        let dt = dt.max(0.001);
        let addrs = getifaddrs();
        let default_v4 = default_route_v4();
        let default_v6 = default_route_v6();
        let mut ifaces = Vec::new();
        let mut names = iface_names();
        names.sort_by_key(|n| (n == "lo", n.clone()));
        for name in names {
            let base = format!("/sys/class/net/{}", name);
            let raw = raw_stats(&name);
            let prev = self.prev.get(&name).copied().unwrap_or(raw);
            self.prev.insert(name.clone(), raw);
            let is_wireless = std::path::Path::new(&format!("{}/wireless", base)).exists()
                || std::path::Path::new(&format!("{}/phy80211", base)).exists();
            let has_device = std::path::Path::new(&format!("{}/device", base)).exists();
            let kind = if name == "lo" {
                "loopback"
            } else if is_wireless {
                "wi-fi"
            } else if std::path::Path::new(&format!("{}/bridge", base)).exists() {
                "bridge"
            } else if std::path::Path::new(&format!("{}/tun_flags", base)).exists()
                || name.starts_with("tailscale")
                || name.starts_with("wg")
                || name.starts_with("tun")
            {
                "vpn/tunnel"
            } else if name.starts_with("docker") || name.starts_with("veth") || name.starts_with("virbr") || name.starts_with("br-") {
                "virtual"
            } else if has_device {
                if read(format!("{}/type", base)).as_deref() == Some("1") {
                    "ethernet"
                } else {
                    "device"
                }
            } else {
                "virtual"
            };
            let driver = std::fs::read_link(format!("{}/device/driver", base))
                .ok()
                .and_then(|p| p.file_name().map(|f| f.to_string_lossy().into_owned()));
            let (v4, v6) = addrs.get(&name).cloned().unwrap_or_default();
            let speed = read_u64(format!("{}/speed", base)).filter(|v| *v > 0 && *v < 1_000_000).map(|v| v as u32);
            ifaces.push(Iface {
                name: name.clone(),
                kind: kind.to_string(),
                state: read(format!("{}/operstate", base)).unwrap_or_else(|| "unknown".into()),
                carrier: read_u64(format!("{}/carrier", base)).unwrap_or(0) == 1,
                mac: read(format!("{}/address", base)).unwrap_or_default(),
                mtu: read_u64(format!("{}/mtu", base)).unwrap_or(0) as u32,
                speed_mbps: speed,
                duplex: read(format!("{}/duplex", base)).filter(|d| d != "unknown"),
                driver,
                ipv4: v4,
                ipv6: v6,
                rx_bps: raw.rx_bytes.saturating_sub(prev.rx_bytes) as f64 / dt,
                tx_bps: raw.tx_bytes.saturating_sub(prev.tx_bytes) as f64 / dt,
                rx_pps: raw.rx_packets.saturating_sub(prev.rx_packets) as f64 / dt,
                tx_pps: raw.tx_packets.saturating_sub(prev.tx_packets) as f64 / dt,
                rx_bytes: raw.rx_bytes,
                tx_bytes: raw.tx_bytes,
                rx_packets: raw.rx_packets,
                tx_packets: raw.tx_packets,
                rx_errors: read_u64(format!("{}/statistics/rx_errors", base)).unwrap_or(0),
                tx_errors: read_u64(format!("{}/statistics/tx_errors", base)).unwrap_or(0),
                rx_dropped: read_u64(format!("{}/statistics/rx_dropped", base)).unwrap_or(0),
                tx_dropped: read_u64(format!("{}/statistics/tx_dropped", base)).unwrap_or(0),
                is_default: default_v4.as_ref().map(|(i, _)| i == &name).unwrap_or(false),
            });
        }
        // Default interface first, then up interfaces, loopback last.
        ifaces.sort_by_key(|i| (!i.is_default, i.kind == "loopback", i.state != "up", i.name.clone()));
        let total_rx_bps = ifaces.iter().filter(|i| i.kind != "loopback").map(|i| i.rx_bps).sum();
        let total_tx_bps = ifaces.iter().filter(|i| i.kind != "loopback").map(|i| i.tx_bps).sum();
        let (dns, dns_upstream, search) = resolv();
        let tcp = snmp_tcp().map(|t| {
            let r = TcpStats {
                cur_estab: t.3,
                in_segs_per_s: t.0.saturating_sub(self.prev_tcp.0) as f64 / dt,
                out_segs_per_s: t.1.saturating_sub(self.prev_tcp.1) as f64 / dt,
                retrans_per_s: t.2.saturating_sub(self.prev_tcp.2) as f64 / dt,
                in_errs: t.4,
                active_opens: t.5,
                passive_opens: t.6,
            };
            self.prev_tcp = (t.0, t.1, t.2);
            r
        });
        NetSnap { ifaces, default_v4, default_v6, dns, dns_upstream, search, sockets: sockstat(), tcp, total_rx_bps, total_tx_bps }
    }
}

fn iface_names() -> Vec<String> {
    std::fs::read_dir("/sys/class/net")
        .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default()
}

fn raw_stats(name: &str) -> Raw {
    let b = format!("/sys/class/net/{}/statistics", name);
    Raw {
        rx_bytes: read_u64(format!("{}/rx_bytes", b)).unwrap_or(0),
        tx_bytes: read_u64(format!("{}/tx_bytes", b)).unwrap_or(0),
        rx_packets: read_u64(format!("{}/rx_packets", b)).unwrap_or(0),
        tx_packets: read_u64(format!("{}/tx_packets", b)).unwrap_or(0),
    }
}

/// Interface -> (ipv4 list with prefix, ipv6 list with prefix).
fn getifaddrs() -> HashMap<String, (Vec<String>, Vec<String>)> {
    let mut out: HashMap<String, (Vec<String>, Vec<String>)> = HashMap::new();
    let mut ifap: *mut libc::ifaddrs = std::ptr::null_mut();
    if unsafe { libc::getifaddrs(&mut ifap) } != 0 {
        return out;
    }
    let mut p = ifap;
    while !p.is_null() {
        let ifa = unsafe { &*p };
        if !ifa.ifa_addr.is_null() {
            let name = unsafe { CStr::from_ptr(ifa.ifa_name) }.to_string_lossy().into_owned();
            let fam = unsafe { (*ifa.ifa_addr).sa_family } as i32;
            let entry = out.entry(name).or_default();
            if fam == libc::AF_INET {
                let sin = unsafe { &*(ifa.ifa_addr as *const libc::sockaddr_in) };
                let ip = Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
                let prefix = if ifa.ifa_netmask.is_null() {
                    32
                } else {
                    let m = unsafe { &*(ifa.ifa_netmask as *const libc::sockaddr_in) };
                    u32::from_be(m.sin_addr.s_addr).count_ones()
                };
                entry.0.push(format!("{}/{}", ip, prefix));
            } else if fam == libc::AF_INET6 {
                let sin6 = unsafe { &*(ifa.ifa_addr as *const libc::sockaddr_in6) };
                let ip = Ipv6Addr::from(sin6.sin6_addr.s6_addr);
                let prefix = if ifa.ifa_netmask.is_null() {
                    128
                } else {
                    let m = unsafe { &*(ifa.ifa_netmask as *const libc::sockaddr_in6) };
                    m.sin6_addr.s6_addr.iter().map(|b| b.count_ones()).sum::<u32>()
                };
                entry.1.push(format!("{}/{}", ip, prefix));
            }
        }
        p = ifa.ifa_next;
    }
    unsafe { libc::freeifaddrs(ifap) };
    // Global IPv6 before link-local.
    for v in out.values_mut() {
        v.1.sort_by_key(|a| a.starts_with("fe80"));
    }
    out
}

fn default_route_v4() -> Option<(String, String)> {
    let s = read("/proc/net/route")?;
    let mut best: Option<(u64, String, String)> = None;
    for line in s.lines().skip(1) {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 8 || f[1] != "00000000" {
            continue;
        }
        let flags = u32::from_str_radix(f[3], 16).unwrap_or(0);
        if flags & 0x2 == 0 {
            continue;
        }
        let gw = u32::from_str_radix(f[2], 16).ok()?;
        let gw = Ipv4Addr::from(gw.swap_bytes()).to_string();
        let metric: u64 = f[6].parse().unwrap_or(0);
        if best.as_ref().map(|b| metric < b.0).unwrap_or(true) {
            best = Some((metric, f[0].to_string(), gw));
        }
    }
    best.map(|b| (b.1, b.2))
}

fn default_route_v6() -> Option<(String, String)> {
    let s = read("/proc/net/ipv6_route")?;
    for line in s.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 10 {
            continue;
        }
        if f[0] == "00000000000000000000000000000000" && f[1] == "00" && f[4] != "00000000000000000000000000000000" {
            let mut bytes = [0u8; 16];
            for i in 0..16 {
                bytes[i] = u8::from_str_radix(&f[4][i * 2..i * 2 + 2], 16).unwrap_or(0);
            }
            let gw = Ipv6Addr::from(bytes).to_string();
            return Some((f[9].to_string(), gw));
        }
    }
    None
}

fn resolv() -> (Vec<String>, Vec<String>, Vec<String>) {
    let parse = |p: &str| -> (Vec<String>, Vec<String>) {
        let mut ns = Vec::new();
        let mut search = Vec::new();
        if let Some(s) = read(p) {
            for l in s.lines() {
                let l = l.trim();
                if let Some(v) = l.strip_prefix("nameserver ") {
                    ns.push(v.trim().to_string());
                } else if let Some(v) = l.strip_prefix("search ") {
                    search.extend(v.split_whitespace().map(|x| x.to_string()));
                }
            }
        }
        (ns, search)
    };
    let (dns, search) = parse("/etc/resolv.conf");
    let upstream = if dns.iter().any(|d| d.starts_with("127.0.0.53") || d == "127.0.0.1") {
        let (u, _) = parse("/run/systemd/resolve/resolv.conf");
        u
    } else {
        Vec::new()
    };
    (dns, upstream, search)
}

fn sockstat() -> Option<SockStat> {
    let s = read("/proc/net/sockstat")?;
    let mut st = SockStat::default();
    for l in s.lines() {
        let f: Vec<&str> = l.split_whitespace().collect();
        let val = |k: &str| f.iter().position(|x| *x == k).and_then(|i| f.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(0u64);
        match f.first().copied() {
            Some("sockets:") => st.used = val("used"),
            Some("TCP:") => {
                st.tcp_inuse = val("inuse");
                st.tcp_orphan = val("orphan");
                st.tcp_tw = val("tw");
                st.tcp_alloc = val("alloc");
            }
            Some("UDP:") => st.udp_inuse = val("inuse"),
            _ => {}
        }
    }
    Some(st)
}

/// (InSegs, OutSegs, RetransSegs, CurrEstab, InErrs, ActiveOpens, PassiveOpens)
fn snmp_tcp() -> Option<(u64, u64, u64, u64, u64, u64, u64)> {
    let s = read("/proc/net/snmp")?;
    let mut lines = s.lines().filter(|l| l.starts_with("Tcp:"));
    let hdr: Vec<&str> = lines.next()?.split_whitespace().collect();
    let val: Vec<&str> = lines.next()?.split_whitespace().collect();
    let g =
        |k: &str| hdr.iter().position(|h| *h == k).and_then(|i| val.get(i)).and_then(|v| v.parse::<i64>().ok()).unwrap_or(0).max(0) as u64;
    Some((g("InSegs"), g("OutSegs"), g("RetransSegs"), g("CurrEstab"), g("InErrs"), g("ActiveOpens"), g("PassiveOpens")))
}
