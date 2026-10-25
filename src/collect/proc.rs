//! Per-process CPU and memory from /proc/<pid>/stat.

use crate::util::{now_epoch, read};
use std::collections::HashMap;
use std::os::unix::fs::MetadataExt;

#[derive(Clone, Debug, Default)]
pub struct Proc {
    pub pid: i32,
    pub name: String,
    pub cmd: String,
    pub state: char,
    pub cpu_pct: f64,
    pub rss: u64,
    pub mem_pct: f64,
    pub threads: u32,
    pub user: String,
    pub age_s: u64,
    pub cpu_time_s: f64,
    pub nice: i32,
    pub start: u64,
}

#[derive(Clone, Debug, Default)]
pub struct ProcSnap {
    pub procs: Vec<Proc>,
    pub total: usize,
    pub threads: usize,
    pub running: usize,
    pub sleeping: usize,
    pub stopped: usize,
    pub zombie: usize,
}

pub struct ProcSampler {
    prev: HashMap<i32, (u64, u64)>,
    cmd_cache: HashMap<(i32, u64), String>,
    clk_tck: f64,
    page_size: u64,
    users: HashMap<u32, String>,
    boot_epoch: u64,
}

impl ProcSampler {
    pub fn new(boot_epoch: u64) -> Self {
        let clk_tck = unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as f64;
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as u64;
        let mut users = HashMap::new();
        if let Some(pw) = read("/etc/passwd") {
            for l in pw.lines() {
                let f: Vec<&str> = l.split(':').collect();
                if f.len() > 2 {
                    if let Ok(uid) = f[2].parse::<u32>() {
                        users.insert(uid, f[0].to_string());
                    }
                }
            }
        }
        let mut s = ProcSampler {
            prev: HashMap::new(),
            cmd_cache: HashMap::new(),
            clk_tck: if clk_tck > 0.0 { clk_tck } else { 100.0 },
            page_size,
            users,
            boot_epoch,
        };
        let _ = s.sample(1.0, 1);
        s
    }

    pub fn sample(&mut self, dt: f64, mem_total: u64) -> ProcSnap {
        let dt = dt.max(0.001);
        let mut snap = ProcSnap::default();
        let Ok(rd) = std::fs::read_dir("/proc") else { return snap };
        let mut next: HashMap<i32, (u64, u64)> = HashMap::with_capacity(self.prev.len() + 16);
        let now = now_epoch();
        let mut procs: Vec<Proc> = Vec::with_capacity(self.prev.len() + 16);
        for e in rd.flatten() {
            let fname = e.file_name();
            let Some(pid) = fname.to_str().and_then(|s| s.parse::<i32>().ok()) else { continue };
            let Ok(stat) = std::fs::read_to_string(format!("/proc/{}/stat", pid)) else { continue };
            let Some(lp) = stat.find('(') else { continue };
            let Some(rp) = stat.rfind(')') else { continue };
            let name = stat[lp + 1..rp].to_string();
            let rest: Vec<&str> = stat[rp + 1..].split_whitespace().collect();
            if rest.len() < 22 {
                continue;
            }
            let g = |i: usize| rest.get(i).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
            let state = rest[0].chars().next().unwrap_or('?');
            let utime = g(11);
            let stime = g(12);
            let nice = rest.get(16).and_then(|v| v.parse::<i32>().ok()).unwrap_or(0);
            let threads = g(17) as u32;
            let starttime = g(19);
            let rss = g(21) * self.page_size;
            let ticks = utime + stime;
            let _ = starttime;
            next.insert(pid, (ticks, starttime));
            let cpu_pct = match self.prev.get(&pid) {
                Some(&(pt, pst)) if pst == starttime => (ticks.saturating_sub(pt) as f64 / self.clk_tck) / dt * 100.0,
                _ => 0.0,
            };
            let uid = e.metadata().map(|m| m.uid()).unwrap_or(u32::MAX);
            let user = self.users.get(&uid).cloned().unwrap_or_else(|| if uid == u32::MAX { "?".into() } else { uid.to_string() });
            let start_epoch = self.boot_epoch + (starttime as f64 / self.clk_tck) as u64;
            snap.total += 1;
            snap.threads += threads as usize;
            match state {
                'R' => snap.running += 1,
                'S' | 'D' | 'I' => snap.sleeping += 1,
                'T' | 't' => snap.stopped += 1,
                'Z' => snap.zombie += 1,
                _ => {}
            }
            procs.push(Proc {
                pid,
                name,
                cmd: String::new(),
                state,
                cpu_pct,
                rss,
                mem_pct: if mem_total > 0 { rss as f64 / mem_total as f64 * 100.0 } else { 0.0 },
                threads,
                user,
                age_s: now.saturating_sub(start_epoch),
                cpu_time_s: ticks as f64 / self.clk_tck,
                nice,
                start: starttime,
            });
        }
        self.prev = next;
        // Read cmdline only for the processes likely to be displayed.
        procs.sort_by(|a, b| b.cpu_pct.partial_cmp(&a.cpu_pct).unwrap_or(std::cmp::Ordering::Equal));
        let mut want: Vec<i32> = procs.iter().take(80).map(|p| p.pid).collect();
        let mut by_mem: Vec<&Proc> = procs.iter().collect();
        by_mem.sort_by_key(|a| std::cmp::Reverse(a.rss));
        want.extend(by_mem.iter().take(80).map(|p| p.pid));
        let mut fresh: HashMap<(i32, u64), String> = HashMap::with_capacity(want.len());
        for p in procs.iter_mut() {
            if want.contains(&p.pid) {
                let key = (p.pid, p.start);
                let cmd = match self.cmd_cache.remove(&key) {
                    Some(c) => c,
                    None => match std::fs::read(format!("/proc/{}/cmdline", p.pid)) {
                        Ok(c) => {
                            let s: String = c
                                .split(|b| *b == 0)
                                .filter(|a| !a.is_empty())
                                .map(|a| String::from_utf8_lossy(a).into_owned())
                                .collect::<Vec<_>>()
                                .join(" ");
                            if s.is_empty() {
                                format!("[{}]", p.name)
                            } else {
                                s
                            }
                        }
                        Err(_) => format!("[{}]", p.name),
                    },
                };
                p.cmd = cmd.clone();
                fresh.insert(key, cmd);
            } else {
                p.cmd = p.name.clone();
            }
        }
        self.cmd_cache = fresh;
        snap.procs = procs;
        snap
    }
}
