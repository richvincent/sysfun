//! Application state: current view, latest snapshot, history ring buffers, selections, settings.

use crate::collect::host::HostInfo;
use crate::collect::Snapshot;
use crate::config::{Config, ThemeName, INTERVAL_STEPS};
use ratatui::layout::Rect;
use std::collections::VecDeque;
use std::path::PathBuf;
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
    /// Not a content tab: reached with `,` or the gear icon.
    Settings,
}

impl View {
    /// The content views, in their canonical order.
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
            View::Settings => "Settings",
        }
    }
    /// Stable identifier used in the config file.
    pub fn slug(self) -> &'static str {
        match self {
            View::Overview => "overview",
            View::Cpu => "cpu",
            View::Mem => "memory",
            View::Disk => "disk",
            View::Net => "network",
            View::Wifi => "wifi",
            View::Power => "power",
            View::Thermal => "thermal",
            View::Procs => "processes",
            View::Settings => "settings",
        }
    }
    pub fn from_slug(s: &str) -> Option<View> {
        let s = s.trim().to_ascii_lowercase();
        View::ALL.iter().copied().find(|v| v.slug() == s).or(match s.as_str() {
            "mem" | "ram" => Some(View::Mem),
            "net" => Some(View::Net),
            "wlan" => Some(View::Wifi),
            "bat" | "battery" => Some(View::Power),
            "temp" | "sensors" => Some(View::Thermal),
            "ps" | "proc" | "top" => Some(View::Procs),
            _ => None,
        })
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

/// Screen regions recorded during the last draw so mouse events can be hit-tested.
#[derive(Default)]
pub struct HitAreas {
    pub header: Option<Rect>,
    pub tabs: Vec<(Rect, View)>,
    pub gear: Option<Rect>,
    /// Process table: data rows area, index of the first visible row, header row.
    pub proc_rows: Option<Rect>,
    pub proc_start: usize,
    pub proc_header: Option<Rect>,
    /// Wi-Fi table: data rows area and the table's scroll offset.
    pub wifi_rows: Option<Rect>,
    pub wifi_offset: usize,
    /// Settings view: one rect per item, indexed like `SettingsItem::all`.
    pub settings_rows: Vec<Rect>,
}

fn contains(r: Option<Rect>, x: u16, y: u16) -> bool {
    r.map(|r| x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height).unwrap_or(false)
}

/// Rows of the Settings view, in display order: one per tab, then the preferences.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SettingsItem {
    Tab(usize),
    DefaultView,
    Interval,
    Temperature,
    Theme,
    Mouse,
    HiddenNetworks,
}

impl SettingsItem {
    pub const PREFS: [SettingsItem; 6] = [
        SettingsItem::DefaultView,
        SettingsItem::Interval,
        SettingsItem::Temperature,
        SettingsItem::Theme,
        SettingsItem::Mouse,
        SettingsItem::HiddenNetworks,
    ];

    pub fn all(cfg: &Config) -> Vec<SettingsItem> {
        (0..cfg.tabs.len()).map(SettingsItem::Tab).chain(SettingsItem::PREFS.iter().copied()).collect()
    }
}

/// Side effects the main loop must apply after a settings change.
#[derive(Clone, Copy, Debug)]
pub enum Effect {
    Interval(u64),
    Mouse(bool),
}

pub struct App {
    pub view: View,
    /// Visible content tabs in display order (derived from `cfg`).
    pub tabs: Vec<View>,
    pub snap: Option<Snapshot>,
    pub host: Arc<HostInfo>,
    pub hist: History,
    pub proc_sort: ProcSort,
    pub proc_sel: usize,
    pub wifi_sel: usize,
    pub settings_sel: usize,
    pub show_help: bool,
    pub interval_ms: u64,
    pub paused: bool,
    pub last_draw_ms: f64,
    pub frames: u64,
    pub rescan_at: Option<std::time::Instant>,
    pub hit: HitAreas,
    pub cfg: Config,
    pub cfg_path: PathBuf,
    /// Last save/load status shown in the Settings view.
    pub cfg_msg: Option<String>,
    pub effects: Vec<Effect>,
}

impl App {
    pub fn new(view: View, host: Arc<HostInfo>, interval_ms: u64, cfg: Config, cfg_path: PathBuf, cfg_msg: Option<String>) -> Self {
        let tabs = cfg.visible_tabs();
        App {
            view,
            tabs,
            snap: None,
            host,
            hist: History::default(),
            proc_sort: ProcSort::Cpu,
            proc_sel: 0,
            wifi_sel: 0,
            settings_sel: 0,
            show_help: false,
            interval_ms,
            paused: false,
            last_draw_ms: 0.0,
            frames: 0,
            rescan_at: None,
            hit: HitAreas::default(),
            cfg,
            cfg_path,
            cfg_msg,
            effects: Vec::new(),
        }
    }

    pub fn accept(&mut self, s: Snapshot) {
        if self.paused {
            return;
        }
        self.hist.record(&s);
        self.snap = Some(s);
    }

    pub fn mono(&self) -> bool {
        self.cfg.theme == ThemeName::Mono
    }

    // ── view navigation ──────────────────────────────────────────────

    pub fn next_view(&mut self) {
        let i = self.tabs.iter().position(|v| *v == self.view);
        self.view = match i {
            Some(i) if i + 1 < self.tabs.len() => self.tabs[i + 1],
            Some(_) => View::Settings,
            None => self.tabs.first().copied().unwrap_or(View::Settings),
        };
    }

    pub fn prev_view(&mut self) {
        let i = self.tabs.iter().position(|v| *v == self.view);
        self.view = match i {
            Some(0) => View::Settings,
            Some(i) => self.tabs[i - 1],
            None => self.tabs.last().copied().unwrap_or(View::Settings),
        };
    }

    pub fn jump(&mut self, digit: usize) {
        if let Some(v) = self.tabs.get(digit.wrapping_sub(1)) {
            self.view = *v;
        }
    }

    pub fn select_down(&mut self) {
        match self.view {
            View::Procs => self.proc_sel = self.proc_sel.saturating_add(1),
            View::Wifi => self.wifi_sel = self.wifi_sel.saturating_add(1),
            View::Settings => self.settings_sel = (self.settings_sel + 1).min(SettingsItem::all(&self.cfg).len() - 1),
            _ => {}
        }
    }
    pub fn select_up(&mut self) {
        match self.view {
            View::Procs => self.proc_sel = self.proc_sel.saturating_sub(1),
            View::Wifi => self.wifi_sel = self.wifi_sel.saturating_sub(1),
            View::Settings => self.settings_sel = self.settings_sel.saturating_sub(1),
            _ => {}
        }
    }

    // ── settings ─────────────────────────────────────────────────────

    pub fn settings_item(&self) -> SettingsItem {
        let all = SettingsItem::all(&self.cfg);
        all[self.settings_sel.min(all.len() - 1)]
    }

    /// Space / Enter / click on the selected row: toggle a tab or advance a preference.
    pub fn settings_toggle(&mut self) {
        match self.settings_item() {
            SettingsItem::Tab(i) => {
                let visible = self.cfg.tabs.iter().filter(|(_, on)| *on).count();
                if let Some((_, on)) = self.cfg.tabs.get_mut(i) {
                    if *on && visible <= 1 {
                        self.cfg_msg = Some("at least one tab must stay visible".into());
                        return;
                    }
                    *on = !*on;
                }
                self.save_cfg();
            }
            _ => self.settings_cycle(1),
        }
    }

    /// ← / → on a preference row.
    pub fn settings_cycle(&mut self, dir: i32) {
        let fwd = dir > 0;
        match self.settings_item() {
            SettingsItem::Tab(_) => {
                // Left/right on a tab row moves it, which feels natural in a horizontal tab bar.
                self.settings_reorder(dir);
                return;
            }
            SettingsItem::DefaultView => {
                let order: Vec<View> = self.cfg.tabs.iter().map(|(v, _)| *v).collect();
                let i = order.iter().position(|v| *v == self.cfg.default_view).unwrap_or(0);
                let n = order.len();
                self.cfg.default_view = order[if fwd { (i + 1) % n } else { (i + n - 1) % n }];
            }
            SettingsItem::Interval => {
                let i = INTERVAL_STEPS.iter().position(|v| *v >= self.cfg.interval_ms).unwrap_or(INTERVAL_STEPS.len() - 1);
                let n = INTERVAL_STEPS.len();
                let j = if fwd { (i + 1).min(n - 1) } else { i.saturating_sub(1) };
                self.cfg.interval_ms = INTERVAL_STEPS[j];
                self.interval_ms = self.cfg.interval_ms;
                self.effects.push(Effect::Interval(self.cfg.interval_ms));
            }
            SettingsItem::Temperature => self.cfg.fahrenheit = !self.cfg.fahrenheit,
            SettingsItem::Theme => {
                self.cfg.theme = match self.cfg.theme {
                    ThemeName::Default => ThemeName::Mono,
                    ThemeName::Mono => ThemeName::Default,
                }
            }
            SettingsItem::Mouse => {
                self.cfg.mouse = !self.cfg.mouse;
                self.effects.push(Effect::Mouse(self.cfg.mouse));
            }
            SettingsItem::HiddenNetworks => self.cfg.show_hidden_networks = !self.cfg.show_hidden_networks,
        }
        self.save_cfg();
    }

    /// Move the selected tab up (dir < 0) or down (dir > 0) in the order.
    pub fn settings_reorder(&mut self, dir: i32) {
        if let SettingsItem::Tab(i) = self.settings_item() {
            let j = if dir < 0 { i.checked_sub(1) } else { Some(i + 1).filter(|j| *j < self.cfg.tabs.len()) };
            if let Some(j) = j {
                self.cfg.tabs.swap(i, j);
                self.settings_sel = j;
                self.save_cfg();
            }
        }
    }

    pub fn settings_reset(&mut self) {
        let keep_interval = self.cfg.interval_ms;
        self.cfg = Config::default();
        if keep_interval != self.cfg.interval_ms {
            self.interval_ms = self.cfg.interval_ms;
            self.effects.push(Effect::Interval(self.cfg.interval_ms));
        }
        self.effects.push(Effect::Mouse(self.cfg.mouse));
        self.settings_sel = 0;
        self.save_cfg();
    }

    fn save_cfg(&mut self) {
        self.cfg.normalize();
        self.tabs = self.cfg.visible_tabs();
        self.cfg_msg = Some(match self.cfg.save(&self.cfg_path) {
            Ok(()) => format!("saved to {}", self.cfg_path.display()),
            Err(e) => format!("could not save {}: {}", self.cfg_path.display(), e),
        });
    }

    // ── mouse ────────────────────────────────────────────────────────

    /// Left click at terminal cell (x, y).
    pub fn click(&mut self, x: u16, y: u16) {
        if self.show_help {
            self.show_help = false;
            return;
        }
        if contains(self.hit.gear, x, y) {
            self.view = View::Settings;
            return;
        }
        if let Some((_, v)) = self.hit.tabs.iter().find(|(r, _)| contains(Some(*r), x, y)) {
            self.view = *v;
            return;
        }
        match self.view {
            View::Procs => {
                if contains(self.hit.proc_header, x, y) {
                    if let Some(sort) = crate::ui::procs::header_sort(x.saturating_sub(self.hit.proc_header.unwrap().x)) {
                        self.proc_sort = sort;
                        self.proc_sel = 0;
                    }
                } else if let Some(r) = self.hit.proc_rows.filter(|r| contains(Some(*r), x, y)) {
                    self.proc_sel = self.hit.proc_start + (y - r.y) as usize;
                }
            }
            View::Wifi => {
                if let Some(r) = self.hit.wifi_rows.filter(|r| contains(Some(*r), x, y)) {
                    self.wifi_sel = self.hit.wifi_offset + (y - r.y) as usize;
                }
            }
            View::Settings => {
                if let Some(i) = self.hit.settings_rows.iter().position(|r| contains(Some(*r), x, y)) {
                    if self.settings_sel == i {
                        self.settings_toggle(); // second click on the same row acts on it
                    } else {
                        self.settings_sel = i;
                    }
                }
            }
            _ => {}
        }
    }

    /// Mouse wheel: over the tab bar it changes view, elsewhere it moves the selection.
    pub fn scroll(&mut self, down: bool, x: u16, y: u16) {
        if contains(self.hit.header, x, y) {
            if down {
                self.next_view();
            } else {
                self.prev_view();
            }
        } else if down {
            self.select_down();
        } else {
            self.select_up();
        }
    }
}
