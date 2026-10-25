//! Data collection: a background sampler produces `Snapshot`s at a fixed cadence.

pub mod cpu;
pub mod disk;
pub mod host;
pub mod mem;
pub mod net;
pub mod power;
pub mod proc;
pub mod thermal;
pub mod wifi;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub uptime: u64,
    pub cpu: cpu::CpuSnap,
    pub mem: mem::MemSnap,
    pub disk: disk::DiskSnap,
    pub net: net::NetSnap,
    pub wifi: wifi::WifiSnap,
    pub power: power::PowerSnap,
    pub thermal: thermal::ThermalSnap,
    pub procs: proc::ProcSnap,
    pub sample_ms: f64,
    /// Per-collector cost in milliseconds, for the footer and `SYSFUN_TIMING=1`.
    pub timings: Vec<(&'static str, f64)>,
}

/// Wi-Fi needs `iw` subprocesses (~10 ms each), so in the live TUI it runs on its own
/// thread at its own cadence and the main sampler just clones the latest result.
enum WifiSource {
    Sync(Box<wifi::WifiSampler>),
    Threaded { latest: Arc<Mutex<wifi::WifiSnap>>, stop: Arc<AtomicBool>, rescan: Arc<AtomicBool> },
}

pub struct Sampler {
    pub host: Arc<host::HostInfo>,
    cpu: cpu::CpuSampler,
    disk: disk::DiskSampler,
    net: net::NetSampler,
    wifi: WifiSource,
    procs: proc::ProcSampler,
    power: power::PowerSampler,
    thermal: thermal::ThermalSampler,
    last: Instant,
}

impl Drop for Sampler {
    fn drop(&mut self) {
        if let WifiSource::Threaded { stop, .. } = &self.wifi {
            stop.store(true, Ordering::Relaxed);
        }
    }
}

impl Sampler {
    pub fn new(threaded_wifi: bool) -> Self {
        let host = Arc::new(host::HostInfo::collect());
        let wifi = if threaded_wifi {
            let mut ws = wifi::WifiSampler::new();
            let latest = Arc::new(Mutex::new(ws.sample()));
            let stop = Arc::new(AtomicBool::new(false));
            let rescan = Arc::new(AtomicBool::new(false));
            let (l2, s2, r2) = (latest.clone(), stop.clone(), rescan.clone());
            std::thread::Builder::new()
                .name("sysfun-wifi".into())
                .spawn(move || {
                    while !s2.load(Ordering::Relaxed) {
                        // Sleep in short slices so a rescan request is picked up promptly.
                        for _ in 0..10 {
                            std::thread::sleep(Duration::from_millis(100));
                            if s2.load(Ordering::Relaxed) || r2.load(Ordering::Relaxed) {
                                break;
                            }
                        }
                        if s2.load(Ordering::Relaxed) {
                            break;
                        }
                        if r2.swap(false, Ordering::Relaxed) {
                            ws.rescan();
                        }
                        let snap = ws.sample();
                        if let Ok(mut g) = l2.lock() {
                            *g = snap;
                        }
                    }
                })
                .expect("spawn wifi thread");
            WifiSource::Threaded { latest, stop, rescan }
        } else {
            WifiSource::Sync(Box::new(wifi::WifiSampler::new()))
        };
        Sampler {
            cpu: cpu::CpuSampler::new(),
            disk: disk::DiskSampler::new(),
            net: net::NetSampler::new(),
            wifi,
            procs: proc::ProcSampler::new(host.boot_epoch),
            power: power::PowerSampler::new(),
            thermal: thermal::ThermalSampler::new(),
            host,
            last: Instant::now(),
        }
    }

    pub fn rescan_wifi(&mut self) {
        match &mut self.wifi {
            WifiSource::Sync(ws) => ws.rescan(),
            WifiSource::Threaded { rescan, .. } => rescan.store(true, Ordering::Relaxed),
        }
    }

    pub fn sample(&mut self) -> Snapshot {
        let t0 = Instant::now();
        let dt = t0.duration_since(self.last).as_secs_f64();
        self.last = t0;
        let mut timings = Vec::with_capacity(8);
        let mut timed = |name: &'static str, f: &mut dyn FnMut()| {
            let t = Instant::now();
            f();
            timings.push((name, t.elapsed().as_secs_f64() * 1000.0));
        };
        let mut cpu = Default::default();
        timed("cpu", &mut || cpu = self.cpu.sample(dt));
        let mut mem = Default::default();
        timed("mem", &mut || mem = mem::sample());
        let mut disk = Default::default();
        timed("disk", &mut || disk = self.disk.sample(dt));
        let mut net = Default::default();
        timed("net", &mut || net = self.net.sample(dt));
        let mut wifi = Default::default();
        timed("wifi", &mut || {
            wifi = match &mut self.wifi {
                WifiSource::Sync(ws) => ws.sample(),
                WifiSource::Threaded { latest, .. } => latest.lock().map(|g| g.clone()).unwrap_or_default(),
            }
        });
        let mut power = Default::default();
        timed("power", &mut || power = self.power.sample());
        let mut thermal = Default::default();
        timed("thermal", &mut || thermal = self.thermal.sample());
        let mem_total = mem.total;
        let mut procs = Default::default();
        timed("procs", &mut || procs = self.procs.sample(dt, mem_total));
        if std::env::var_os("SYSFUN_TIMING").is_some() {
            let parts: Vec<String> = timings.iter().map(|(n, ms)| format!("{} {:.1}ms", n, ms)).collect();
            eprintln!("sysfun timing: {}", parts.join(" · "));
        }
        Snapshot {
            uptime: self.host.uptime_secs(),
            cpu,
            mem,
            disk,
            net,
            wifi,
            power,
            thermal,
            procs,
            sample_ms: t0.elapsed().as_secs_f64() * 1000.0,
            timings,
        }
    }
}

pub enum Cmd {
    Interval(Duration),
    Refresh,
    Rescan,
    Quit,
}

/// Spawn the sampler thread. The first snapshot arrives after ~250 ms so rates are meaningful.
pub fn spawn(interval: Duration) -> (Arc<host::HostInfo>, Receiver<Snapshot>, Sender<Cmd>) {
    let (tx, rx) = mpsc::channel::<Snapshot>();
    let (ctx, crx) = mpsc::channel::<Cmd>();
    let mut sampler = Sampler::new(true);
    let host = sampler.host.clone();
    std::thread::Builder::new()
        .name("sysfun-sampler".into())
        .spawn(move || {
            let mut interval = interval;
            let mut wait = Duration::from_millis(250);
            loop {
                match crx.recv_timeout(wait) {
                    Ok(Cmd::Quit) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Ok(Cmd::Interval(d)) => {
                        interval = d;
                        continue;
                    }
                    Ok(Cmd::Rescan) => {
                        sampler.rescan_wifi();
                        continue;
                    }
                    Ok(Cmd::Refresh) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
                let snap = sampler.sample();
                let cost = Duration::from_secs_f64(snap.sample_ms / 1000.0);
                if tx.send(snap).is_err() {
                    break;
                }
                wait = interval.saturating_sub(cost).max(Duration::from_millis(50));
            }
        })
        .expect("spawn sampler thread");
    (host, rx, ctx)
}
