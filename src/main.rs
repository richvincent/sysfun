//! sysfun — fast, elegant system information in your terminal.

mod app;
mod collect;
mod ui;
mod util;

use app::{App, ProcSort, View};
use clap::{ArgGroup, Parser};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use std::time::{Duration, Instant};

#[derive(Parser, Debug)]
#[command(
    name = "sysfun",
    version,
    about = "Fast, elegant system information in your terminal",
    long_about = "sysfun shows everything relevant about this machine in a live TUI.\n\
                  Every section starts with a plain-language summary and gets progressively\n\
                  more detailed the further down you read.",
    after_help = "Keys: Tab/←→ switch view · 1-9 jump · ↑↓ select · c/m/p/n sort processes · s rescan Wi-Fi · space pause · q quit"
)]
#[command(group(ArgGroup::new("view").args(["bat", "wifi", "disk", "cpu", "mem", "net", "temp", "ps", "overview"])))]
struct Cli {
    /// Battery and power: charge, time left, wattage, health, chargers
    #[arg(long, visible_alias = "battery", visible_alias = "power")]
    bat: bool,
    /// Wi-Fi: friendly status at the top, RF engineering detail at the bottom
    #[arg(long, visible_alias = "wlan")]
    wifi: bool,
    /// Storage: filesystems, block devices, live I/O
    #[arg(long, visible_alias = "storage")]
    disk: bool,
    /// Processor: per-core load, frequencies, scheduler, pressure
    #[arg(long)]
    cpu: bool,
    /// Memory: RAM, swap, zram, kernel accounting
    #[arg(long, visible_alias = "memory", visible_alias = "ram")]
    mem: bool,
    /// Network: interfaces, addresses, routes, DNS, sockets
    #[arg(long, visible_alias = "network")]
    net: bool,
    /// Sensors: temperatures, fans, voltages, power rails
    #[arg(long, visible_alias = "thermal", visible_alias = "sensors")]
    temp: bool,
    /// Processes: top consumers by CPU or memory
    #[arg(long, visible_alias = "proc", visible_alias = "top")]
    ps: bool,
    /// Everything at a glance (default)
    #[arg(long)]
    overview: bool,
    /// Refresh interval in milliseconds
    #[arg(short, long, default_value_t = 1000, value_name = "MS")]
    interval: u64,
    /// Print one frame as plain text and exit (no TUI)
    #[arg(long)]
    once: bool,
    /// Width used by --once (defaults to the terminal width or 120)
    #[arg(long, value_name = "COLS")]
    width: Option<u16>,
    /// Height used by --once (defaults to the terminal height or 45)
    #[arg(long, value_name = "ROWS")]
    height: Option<u16>,
}

impl Cli {
    fn view(&self) -> View {
        if self.bat {
            View::Power
        } else if self.wifi {
            View::Wifi
        } else if self.disk {
            View::Disk
        } else if self.cpu {
            View::Cpu
        } else if self.mem {
            View::Mem
        } else if self.net {
            View::Net
        } else if self.temp {
            View::Thermal
        } else if self.ps {
            View::Procs
        } else {
            View::Overview
        }
    }
}

fn main() {
    let cli = Cli::parse();
    let interval = cli.interval.clamp(100, 60_000);
    if cli.once {
        once(&cli, interval);
        return;
    }
    if let Err(e) = run(cli.view(), interval) {
        eprintln!("sysfun: {}", e);
        std::process::exit(1);
    }
}

fn once(cli: &Cli, interval: u64) {
    use ratatui::backend::TestBackend;
    let (w, h) = crossterm::terminal::size().unwrap_or((120, 45));
    let w = cli.width.unwrap_or(w).max(40);
    let h = cli.height.unwrap_or(h).max(12);
    let mut sampler = collect::Sampler::new(false);
    std::thread::sleep(Duration::from_millis(300));
    let snap = sampler.sample();
    let mut app = App::new(cli.view(), sampler.host.clone(), interval);
    app.accept(snap);
    let mut term = ratatui::Terminal::new(TestBackend::new(w, h)).expect("test backend");
    term.draw(|f| ui::draw(f, &mut app)).expect("draw");
    let buf = term.backend().buffer().clone();
    let mut out = String::with_capacity((w as usize + 1) * h as usize);
    for y in 0..buf.area.height {
        let mut line = String::with_capacity(w as usize);
        for x in 0..buf.area.width {
            line.push_str(buf[(x, y)].symbol());
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    print!("{}", out);
}

fn run(view: View, interval_ms: u64) -> std::io::Result<()> {
    let (host, rx, ctx) = collect::spawn(Duration::from_millis(interval_ms));
    let mut app = App::new(view, host, interval_ms);
    let mut terminal = ratatui::init();
    let result = (|| -> std::io::Result<()> {
        let mut dirty = true;
        loop {
            // Drain all pending snapshots; keep only the newest.
            let mut newest = None;
            while let Ok(s) = rx.try_recv() {
                newest = Some(s);
            }
            if let Some(s) = newest {
                app.accept(s);
                dirty = true;
            }
            if dirty {
                let t = Instant::now();
                terminal.draw(|f| ui::draw(f, &mut app))?;
                app.last_draw_ms = t.elapsed().as_secs_f64() * 1000.0;
                app.frames += 1;
                dirty = false;
            }
            if event::poll(Duration::from_millis(40))? {
                match event::read()? {
                    Event::Key(k) if k.kind == KeyEventKind::Press => {
                        dirty = true;
                        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
                        match k.code {
                            KeyCode::Char('q') | KeyCode::Esc => {
                                if app.show_help {
                                    app.show_help = false;
                                } else {
                                    break;
                                }
                            }
                            KeyCode::Char('c') if ctrl => break,
                            KeyCode::Char('?') => app.show_help = !app.show_help,
                            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => app.view = app.view.next(),
                            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => app.view = app.view.prev(),
                            KeyCode::Char(c @ '1'..='9') => {
                                if let Some(v) = View::ALL.get(c as usize - '1' as usize) {
                                    app.view = *v;
                                }
                            }
                            KeyCode::Down | KeyCode::Char('j') => app.select_down(),
                            KeyCode::Up | KeyCode::Char('k') => app.select_up(),
                            KeyCode::Home => {
                                app.proc_sel = 0;
                                app.wifi_sel = 0;
                            }
                            KeyCode::Char('c') => app.proc_sort = ProcSort::Cpu,
                            KeyCode::Char('m') => app.proc_sort = ProcSort::Mem,
                            KeyCode::Char('p') => app.proc_sort = ProcSort::Pid,
                            KeyCode::Char('n') => app.proc_sort = ProcSort::Name,
                            KeyCode::Char(' ') => app.paused = !app.paused,
                            KeyCode::Char('r') => {
                                let _ = ctx.send(collect::Cmd::Refresh);
                            }
                            KeyCode::Char('s') => {
                                let _ = ctx.send(collect::Cmd::Rescan);
                                app.rescan_at = Some(Instant::now());
                            }
                            KeyCode::Char('+') | KeyCode::Char('=') => {
                                app.interval_ms = (app.interval_ms / 2).max(100);
                                let _ = ctx.send(collect::Cmd::Interval(Duration::from_millis(app.interval_ms)));
                            }
                            KeyCode::Char('-') | KeyCode::Char('_') => {
                                app.interval_ms = (app.interval_ms * 2).min(60_000);
                                let _ = ctx.send(collect::Cmd::Interval(Duration::from_millis(app.interval_ms)));
                            }
                            _ => {}
                        }
                    }
                    Event::Resize(_, _) => dirty = true,
                    _ => {}
                }
            }
        }
        Ok(())
    })();
    let _ = ctx.send(collect::Cmd::Quit);
    ratatui::restore();
    result
}
