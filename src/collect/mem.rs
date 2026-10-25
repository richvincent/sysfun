//! Memory, swap and zram.

use super::cpu::{read_psi, Psi};
use crate::util::{read, read_u64};
use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
pub struct Zram {
    pub dev: String,
    pub disksize: u64,
    pub orig: u64,
    pub compr: u64,
    pub mem_used: u64,
    pub algo: String,
}

#[derive(Clone, Debug, Default)]
pub struct MemSnap {
    pub total: u64,
    pub free: u64,
    pub available: u64,
    pub used: u64,
    pub buffers: u64,
    pub cached: u64,
    pub shared: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    pub swap_cached: u64,
    pub dirty: u64,
    pub writeback: u64,
    pub anon: u64,
    pub mapped: u64,
    pub slab: u64,
    pub sreclaim: u64,
    pub sunreclaim: u64,
    pub kernel_stack: u64,
    pub page_tables: u64,
    pub commit_limit: u64,
    pub committed: u64,
    pub hugepages_total: u64,
    pub hugepage_size: u64,
    pub zswap: u64,
    pub zswapped: u64,
    pub active: u64,
    pub inactive: u64,
    pub zram: Vec<Zram>,
    pub psi: Option<Psi>,
}

impl MemSnap {
    pub fn used_pct(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.used as f64 / self.total as f64 * 100.0
        }
    }
    pub fn swap_pct(&self) -> f64 {
        if self.swap_total == 0 {
            0.0
        } else {
            self.swap_used as f64 / self.swap_total as f64 * 100.0
        }
    }
}

pub fn sample() -> MemSnap {
    let s = read("/proc/meminfo").unwrap_or_default();
    let mut m: HashMap<&str, u64> = HashMap::with_capacity(64);
    for line in s.lines() {
        if let Some((k, v)) = line.split_once(':') {
            let mut it = v.split_whitespace();
            let n: u64 = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
            let mult = if it.next() == Some("kB") { 1024 } else { 1 };
            m.insert(k.trim(), n * mult);
        }
    }
    let g = |k: &str| m.get(k).copied().unwrap_or(0);
    let total = g("MemTotal");
    let available = g("MemAvailable");
    let swap_total = g("SwapTotal");
    let swap_free = g("SwapFree");
    let mut zram = Vec::new();
    for i in 0..8 {
        let base = format!("/sys/block/zram{}", i);
        let Some(mm) = read(format!("{}/mm_stat", base)) else { break };
        let v: Vec<u64> = mm.split_whitespace().filter_map(|x| x.parse().ok()).collect();
        let algo = read(format!("{}/comp_algorithm", base))
            .and_then(|a| a.split_whitespace().find(|t| t.starts_with('[')).map(|t| t.trim_matches(|c| c == '[' || c == ']').to_string()))
            .unwrap_or_default();
        zram.push(Zram {
            dev: format!("zram{}", i),
            disksize: read_u64(format!("{}/disksize", base)).unwrap_or(0),
            orig: v.first().copied().unwrap_or(0),
            compr: v.get(1).copied().unwrap_or(0),
            mem_used: v.get(2).copied().unwrap_or(0),
            algo,
        });
    }
    MemSnap {
        total,
        free: g("MemFree"),
        available,
        used: total.saturating_sub(available),
        buffers: g("Buffers"),
        cached: g("Cached"),
        shared: g("Shmem"),
        swap_total,
        swap_used: swap_total.saturating_sub(swap_free),
        swap_cached: g("SwapCached"),
        dirty: g("Dirty"),
        writeback: g("Writeback"),
        anon: g("AnonPages"),
        mapped: g("Mapped"),
        slab: g("Slab"),
        sreclaim: g("SReclaimable"),
        sunreclaim: g("SUnreclaim"),
        kernel_stack: g("KernelStack"),
        page_tables: g("PageTables"),
        commit_limit: g("CommitLimit"),
        committed: g("Committed_AS"),
        hugepages_total: g("HugePages_Total"),
        hugepage_size: g("Hugepagesize"),
        zswap: g("Zswap"),
        zswapped: g("Zswapped"),
        active: g("Active"),
        inactive: g("Inactive"),
        zram,
        psi: read_psi("memory"),
    }
}
