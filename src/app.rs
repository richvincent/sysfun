//! Application state: current view, latest snapshot, history ring buffers, selections.

use crate::collect::host::HostInfo;
use crate::collect::Snapshot;
use std::collections::VecDeque;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Overview,
    Cpu,
    Mem,
    Disk,
    Net,
    Wifi,
    Power,
    Thermal,
    Procs,
}

impl View {
    pub const ALL: [View; 9] =
        [View::Overview, View::Cpu, View::Mem, View::Disk, View::Net, View::Wifi, View::Power, View::Thermal, View::Procs];

    pub fn title(self) -> &'static str {
        match self {
            View::Overview => "Overview",
            View::Cpu => "CPU",
            View::Mem => "Memory",
            View::Disk => "Disk",
            View::Net => "Network",
            View::Wifi => "Wi-Fi",
            View::Power => "Power",
            View::Thermal => "Thermal",
            View::Procs => "Processes",
        }
    }
    pub fn index(self) -> usize {
        View::ALL.iter().position(|v| *v == self).unwrap_or(0)
    }
    pub fn next(self) -> View {
        View::ALL[(self.index() + 1) % View::ALL.len()]
    }
    pub fn prev(self) -> View {
        View::ALL[(self.index() + View::ALL.len() - 1) % View::ALL.len()]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProcSort {
    Cpu,
    Mem,
    Pid,
    Name,
}

const HIST: usize = 600;

#[derive(Default)]
pub struct History {
    pub cpu: VecDeque<u64>,
    pub mem: VecDeque<u64>,
    pub swap: VecDeque<u64>,
    pub net_rx: VecDeque<u64>,
    pub net_tx: VecDeque<u64>,
    pub disk_r: VecDeque<u64>,
    pub disk_w: VecDeque<u64>,
    pub batt_mw: VecDeque<u64>,
    pub wifi_sig: VecDeque<u64>,
    pub wifi_tx: VecDeque<u64>,
    pub temp: VecDeque<u64>,
    pub load: VecDeque<u64>,
}

fn push(dq: &mut VecDeque<u64>, v: u64) {
    if dq.len() >= HIST {
        dq.pop_front();
    }
    dq.push_back(v);
}

/// Last `n` samples as a slice-able Vec (sparklines render newest at the right).
pub fn tail(dq: &VecDeque<u64>, n: usize) -> Vec<u64> {
    let skip = dq.len().saturating_sub(n);
    dq.iter().skip(skip).copied().collect()
}

impl History {
    pub fn record(&mut self, s: &Snapshot) {
        push(&mut self.cpu, s.cpu.total_pct.round() as u64);
        push(&mut self.mem, s.mem.used_pct().round() as u64);
        push(&mut self.swap, s.mem.swap_pct().round() as u64);
        push(&mut self.net_rx, s.net.total_rx_bps as u64);
        push(&mut self.net_tx, s.net.total_tx_bps as u64);
        push(&mut self.disk_r, s.disk.total_read_bps as u64);
        push(&mut self.disk_w, s.disk.total_write_bps as u64);
        push(&mut self.load, (s.cpu.load.0 * 100.0) as u64);
        if let Some(b) = s.power.batteries.first() {
            push(&mut self.batt_mw, (b.power_w.unwrap_or(0.0) * 1000.0) as u64);
        }
        if let Some(l) = &s.wifi.link {
            if l.connected {
                // -100 dBm -> 0, -20 dBm -> 80
                push(&mut self.wifi_sig, (l.signal_dbm.unwrap_or(-100) + 100).clamp(0, 100) as u64);
                push(&mut self.wifi_tx, (l.tx_mbps * 10.0) as u64);
            }
        }
        if let Some(h) = s.thermal.hottest() {
            push(&mut self.temp, (h.value * 10.0) as u64);
        }
    }
}

pub struct App {
    pub view: View,
    pub snap: Option<Snapshot>,
    pub host: Arc<HostInfo>,
    pub hist: History,
    pub proc_sort: ProcSort,
    pub proc_sel: usize,
    pub wifi_sel: usize,
    pub show_help: bool,
    pub interval_ms: u64,
    pub paused: bool,
    pub last_draw_ms: f64,
    pub frames: u64,
    pub rescan_at: Option<std::time::Instant>,
}

impl App {
    pub fn new(view: View, host: Arc<HostInfo>, interval_ms: u64) -> Self {
        App {
            view,
            snap: None,
            host,
            hist: History::default(),
            proc_sort: ProcSort::Cpu,
            proc_sel: 0,
            wifi_sel: 0,
            show_help: false,
            interval_ms,
            paused: false,
            last_draw_ms: 0.0,
            frames: 0,
            rescan_at: None,
        }
    }

    pub fn accept(&mut self, s: Snapshot) {
        if self.paused {
            return;
        }
        self.hist.record(&s);
        self.snap = Some(s);
    }

    pub fn select_down(&mut self) {
        match self.view {
            View::Procs => self.proc_sel = self.proc_sel.saturating_add(1),
            View::Wifi => self.wifi_sel = self.wifi_sel.saturating_add(1),
            _ => {}
        }
    }
    pub fn select_up(&mut self) {
        match self.view {
            View::Procs => self.proc_sel = self.proc_sel.saturating_sub(1),
            View::Wifi => self.wifi_sel = self.wifi_sel.saturating_sub(1),
            _ => {}
        }
    }
}
