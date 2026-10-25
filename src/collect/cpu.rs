//! CPU usage, frequency, load, scheduler stats and pressure.

use crate::util::{read, read_u64};

#[derive(Clone, Copy, Default, Debug)]
pub struct CoreStat {
    pub user: u64,
    pub nice: u64,
    pub system: u64,
    pub idle: u64,
    pub iowait: u64,
    pub irq: u64,
    pub softirq: u64,
    pub steal: u64,
}

impl CoreStat {
    fn parse(fields: &[&str]) -> Self {
        let g = |i: usize| fields.get(i).and_then(|v| v.parse().ok()).unwrap_or(0);
        CoreStat { user: g(0), nice: g(1), system: g(2), idle: g(3), iowait: g(4), irq: g(5), softirq: g(6), steal: g(7) }
    }
    pub fn total(&self) -> u64 {
        self.user + self.nice + self.system + self.idle + self.iowait + self.irq + self.softirq + self.steal
    }
    pub fn busy(&self) -> u64 {
        self.total() - self.idle - self.iowait
    }
}

#[derive(Clone, Debug, Default)]
pub struct Psi {
    pub some10: f64,
    pub some60: f64,
    pub some300: f64,
    pub full10: f64,
    pub full60: f64,
    pub full300: f64,
}

pub fn read_psi(kind: &str) -> Option<Psi> {
    let s = read(format!("/proc/pressure/{}", kind))?;
    let mut p = Psi::default();
    for line in s.lines() {
        let mut it = line.split_whitespace();
        let tag = it.next()?;
        let mut vals = [0.0; 3];
        for kv in it {
            if let Some((k, v)) = kv.split_once('=') {
                let v: f64 = v.parse().unwrap_or(0.0);
                match k {
                    "avg10" => vals[0] = v,
                    "avg60" => vals[1] = v,
                    "avg300" => vals[2] = v,
                    _ => {}
                }
            }
        }
        if tag == "some" {
            p.some10 = vals[0];
            p.some60 = vals[1];
            p.some300 = vals[2];
        } else if tag == "full" {
            p.full10 = vals[0];
            p.full60 = vals[1];
            p.full300 = vals[2];
        }
    }
    Some(p)
}

#[derive(Clone, Debug, Default)]
pub struct CpuSnap {
    pub total_pct: f64,
    pub user_pct: f64,
    pub system_pct: f64,
    pub iowait_pct: f64,
    pub irq_pct: f64,
    pub steal_pct: f64,
    pub per_core: Vec<f64>,
    pub freq_mhz: Vec<Option<u32>>,
    pub max_mhz: Vec<Option<u32>>,
    pub min_mhz: Vec<Option<u32>>,
    pub load: (f64, f64, f64),
    pub running: u32,
    pub blocked: u32,
    pub forks_total: u64,
    pub ctxt_per_s: f64,
    pub intr_per_s: f64,
    pub forks_per_s: f64,
    pub governor: Option<String>,
    pub epp: Option<String>,
    pub driver: Option<String>,
    pub psi: Option<Psi>,
    pub threads_total: u32,
}

pub struct CpuSampler {
    prev_total: CoreStat,
    prev_cores: Vec<CoreStat>,
    prev_ctxt: u64,
    prev_intr: u64,
    prev_forks: u64,
    ncpu: usize,
}

impl CpuSampler {
    pub fn new() -> Self {
        let (t, c, ctxt, intr, forks, _, _) = parse_stat();
        CpuSampler { prev_total: t, prev_cores: c.clone(), prev_ctxt: ctxt, prev_intr: intr, prev_forks: forks, ncpu: c.len() }
    }

    pub fn sample(&mut self, dt: f64) -> CpuSnap {
        let (t, cores, ctxt, intr, forks, running, blocked) = parse_stat();
        let dtot = t.total().saturating_sub(self.prev_total.total()).max(1) as f64;
        let d = |a: u64, b: u64| a.saturating_sub(b) as f64 / dtot * 100.0;
        let total_pct = d(t.busy(), self.prev_total.busy());
        let user_pct = d(t.user + t.nice, self.prev_total.user + self.prev_total.nice);
        let system_pct = d(t.system, self.prev_total.system);
        let iowait_pct = d(t.iowait, self.prev_total.iowait);
        let irq_pct = d(t.irq + t.softirq, self.prev_total.irq + self.prev_total.softirq);
        let steal_pct = d(t.steal, self.prev_total.steal);
        let per_core: Vec<f64> = cores
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let p = self.prev_cores.get(i).copied().unwrap_or_default();
                let dt_ = c.total().saturating_sub(p.total()).max(1) as f64;
                c.busy().saturating_sub(p.busy()) as f64 / dt_ * 100.0
            })
            .collect();
        let dt = dt.max(0.001);
        let ctxt_per_s = ctxt.saturating_sub(self.prev_ctxt) as f64 / dt;
        let intr_per_s = intr.saturating_sub(self.prev_intr) as f64 / dt;
        let forks_per_s = forks.saturating_sub(self.prev_forks) as f64 / dt;
        self.prev_total = t;
        self.prev_cores = cores;
        self.prev_ctxt = ctxt;
        self.prev_intr = intr;
        self.prev_forks = forks;
        self.ncpu = self.prev_cores.len();

        let n = self.ncpu;
        let mut freq_mhz = Vec::with_capacity(n);
        let mut max_mhz = Vec::with_capacity(n);
        let mut min_mhz = Vec::with_capacity(n);
        for i in 0..n {
            let base = format!("/sys/devices/system/cpu/cpu{}/cpufreq", i);
            let cur = read_u64(format!("{}/scaling_cur_freq", base)).or_else(|| read_u64(format!("{}/cpuinfo_cur_freq", base)));
            freq_mhz.push(cur.map(|k| (k / 1000) as u32));
            max_mhz.push(read_u64(format!("{}/cpuinfo_max_freq", base)).map(|k| (k / 1000) as u32));
            min_mhz.push(read_u64(format!("{}/cpuinfo_min_freq", base)).map(|k| (k / 1000) as u32));
        }
        if freq_mhz.iter().all(|f| f.is_none()) {
            // x86 fallback: /proc/cpuinfo "cpu MHz"
            if let Some(info) = read("/proc/cpuinfo") {
                let mut i = 0;
                for l in info.lines().filter(|l| l.starts_with("cpu MHz")) {
                    if let Some(v) = l.split(':').nth(1).and_then(|v| v.trim().parse::<f64>().ok()) {
                        if i < freq_mhz.len() {
                            freq_mhz[i] = Some(v as u32);
                        }
                        i += 1;
                    }
                }
            }
        }
        let load = read("/proc/loadavg")
            .map(|s| {
                let v: Vec<f64> = s.split_whitespace().take(3).filter_map(|x| x.parse().ok()).collect();
                (v.first().copied().unwrap_or(0.0), v.get(1).copied().unwrap_or(0.0), v.get(2).copied().unwrap_or(0.0))
            })
            .unwrap_or_default();
        let threads_total = read("/proc/loadavg")
            .and_then(|s| s.split_whitespace().nth(3).and_then(|x| x.split('/').nth(1)).and_then(|x| x.parse().ok()))
            .unwrap_or(0);
        let governor = read("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor");
        let epp = read("/sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference");
        let driver = read("/sys/devices/system/cpu/cpu0/cpufreq/scaling_driver");
        CpuSnap {
            total_pct,
            user_pct,
            system_pct,
            iowait_pct,
            irq_pct,
            steal_pct,
            per_core,
            freq_mhz,
            max_mhz,
            min_mhz,
            load,
            running,
            blocked,
            forks_total: forks,
            ctxt_per_s,
            intr_per_s,
            forks_per_s,
            governor,
            epp,
            driver,
            psi: read_psi("cpu"),
            threads_total,
        }
    }
}

fn parse_stat() -> (CoreStat, Vec<CoreStat>, u64, u64, u64, u32, u32) {
    let s = read("/proc/stat").unwrap_or_default();
    let mut total = CoreStat::default();
    let mut cores = Vec::new();
    let (mut ctxt, mut intr, mut forks, mut running, mut blocked) = (0u64, 0u64, 0u64, 0u32, 0u32);
    for line in s.lines() {
        let mut it = line.split_whitespace();
        let Some(key) = it.next() else { continue };
        let rest: Vec<&str> = it.collect();
        if key == "cpu" {
            total = CoreStat::parse(&rest);
        } else if let Some(stripped) = key.strip_prefix("cpu") {
            if stripped.chars().all(|c| c.is_ascii_digit()) {
                cores.push(CoreStat::parse(&rest));
            }
        } else if key == "ctxt" {
            ctxt = rest.first().and_then(|v| v.parse().ok()).unwrap_or(0);
        } else if key == "intr" {
            intr = rest.first().and_then(|v| v.parse().ok()).unwrap_or(0);
        } else if key == "processes" {
            forks = rest.first().and_then(|v| v.parse().ok()).unwrap_or(0);
        } else if key == "procs_running" {
            running = rest.first().and_then(|v| v.parse().ok()).unwrap_or(0);
        } else if key == "procs_blocked" {
            blocked = rest.first().and_then(|v| v.parse().ok()).unwrap_or(0);
        }
    }
    (total, cores, ctxt, intr, forks, running, blocked)
}
