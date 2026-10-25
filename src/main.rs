//! sysfun — fast, elegant system information in your terminal.

mod app;
mod collect;
mod config;
mod ui;
mod util;

use app::{App, Effect, ProcSort, View};
use clap::{ArgGroup, Parser};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Parser, Debug)]
#[command(
    name = "sysfun",
    version,
    about = "Fast, elegant system information in your terminal",
    long_about = "sysfun shows everything relevant about this machine in a live TUI.\n\
                  Every section starts with a plain-language summary and gets progressively\n\
                  more detailed the further down you read.",
    after_help = "Keys: Tab/←→ switch view · 1-9 jump · , settings · ↑↓ select · c/m/p/n sort processes · s rescan Wi-Fi · space pause · q quit\n\
                  Mouse: click tabs, table rows, process column headers and the ⚙ icon; wheel scrolls (over the tab bar it switches views)\n\
                  Settings (tab order, default view, refresh rate, °C/°F, theme, mouse) live in ~/.config/sysfun/config.toml"
)]
#[command(group(ArgGroup::new("view").args(["bat", "wifi", "disk", "cpu", "mem", "net", "temp", "ps", "overview", "settings"])))]
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
    /// Everything at a glance
    #[arg(long)]
    overview: bool,
    /// Open the Settings tab (tab order, default view, units, theme)
    #[arg(long)]
    settings: bool,
    /// With --once: draw the help/about overlay (used for screenshots)
    #[arg(long, hide = true)]
    show_help: bool,
    /// With --once: also write a blocky colour mosaic of the frame as a PPM image (debugging aid)
    #[arg(long, hide = true, value_name = "FILE")]
    screenshot: Option<PathBuf>,
    /// Refresh interval in milliseconds (overrides the saved setting)
    #[arg(short, long, value_name = "MS")]
    interval: Option<u64>,
    /// Print one frame as plain text and exit (no TUI)
    #[arg(long)]
    once: bool,
    /// Width used by --once (defaults to the terminal width or 120)
    #[arg(long, value_name = "COLS")]
    width: Option<u16>,
    /// Height used by --once (defaults to the terminal height or 45)
    #[arg(long, value_name = "ROWS")]
    height: Option<u16>,
    /// Settings file to use instead of ~/.config/sysfun/config.toml
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,
    /// Ignore the settings file and run with built-in defaults (nothing is written)
    #[arg(long)]
    no_config: bool,
}

impl Cli {
    /// View requested on the command line, if any.
    fn view(&self) -> Option<View> {
        if self.bat {
            Some(View::Power)
        } else if self.wifi {
            Some(View::Wifi)
        } else if self.disk {
            Some(View::Disk)
        } else if self.cpu {
            Some(View::Cpu)
        } else if self.mem {
            Some(View::Mem)
        } else if self.net {
            Some(View::Net)
        } else if self.temp {
            Some(View::Thermal)
        } else if self.ps {
            Some(View::Procs)
        } else if self.overview {
            Some(View::Overview)
        } else if self.settings {
            Some(View::Settings)
        } else {
            None
        }
    }
}

fn main() {
    let cli = Cli::parse();
    let cfg_path = cli.config.clone().unwrap_or_else(config::default_path);
    let (cfg, cfg_msg) = if cli.no_config { (config::Config::default(), None) } else { config::Config::load(&cfg_path) };
    let interval = cli.interval.unwrap_or(cfg.interval_ms).clamp(100, 60_000);
    let view = cli.view().unwrap_or(cfg.default_view);
    if cli.once {
        once(&cli, view, interval, cfg, cfg_path, cfg_msg);
        return;
    }
    if let Err(e) = run(view, interval, cfg, cfg_path, cfg_msg) {
        eprintln!("sysfun: {}", e);
        std::process::exit(1);
    }
}

fn once(cli: &Cli, view: View, interval: u64, cfg: config::Config, cfg_path: PathBuf, cfg_msg: Option<String>) {
    use ratatui::backend::TestBackend;
    let (w, h) = crossterm::terminal::size().unwrap_or((120, 45));
    let w = cli.width.unwrap_or(w).max(40);
    let h = cli.height.unwrap_or(h).max(12);
    let mut sampler = collect::Sampler::new(false);
    std::thread::sleep(Duration::from_millis(300));
    let snap = sampler.sample();
    let mut app = App::new(view, sampler.host.clone(), interval, cfg, cfg_path, cfg_msg);
    app.accept(snap);
    app.show_help = cli.show_help;
    let mut term = ratatui::Terminal::new(TestBackend::new(w, h)).expect("test backend");
    term.draw(|f| ui::draw(f, &mut app)).expect("draw");
    let buf = term.backend().buffer().clone();
    if let Some(path) = &cli.screenshot {
        if let Err(e) = mosaic(&buf, path) {
            eprintln!("sysfun: could not write {}: {}", path.display(), e);
        }
    }
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

/// Write the frame as a coarse colour mosaic (each cell = 2x4 pixels) so half-block art and
/// colour choices can be checked without a terminal emulator. Text glyphs become a centre dot.
fn mosaic(buf: &ratatui::buffer::Buffer, path: &PathBuf) -> std::io::Result<()> {
    use ratatui::style::Color;
    let (cw, ch) = (2usize, 4usize);
    let (w, h) = (buf.area.width as usize, buf.area.height as usize);
    let rgb = |c: Color, fallback: (u8, u8, u8)| match c {
        Color::Rgb(r, g, b) => (r, g, b),
        Color::Black => (0, 0, 0),
        Color::White => (235, 235, 235),
        Color::Gray => (160, 160, 160),
        Color::DarkGray => (90, 90, 90),
        Color::Yellow => (230, 190, 90),
        _ => fallback,
    };
    let mut img = vec![0u8; w * cw * h * ch * 3];
    for y in 0..h {
        for x in 0..w {
            let cell = &buf[(x as u16, y as u16)];
            let bg = rgb(cell.bg, (24, 28, 32));
            let fg = rgb(cell.fg, (200, 200, 200));
            let sym = cell.symbol();
            for py in 0..ch {
                for px in 0..cw {
                    let top = py < ch / 2;
                    let color = match sym {
                        "▀" => {
                            if top {
                                fg
                            } else {
                                bg
                            }
                        }
                        "▄" => {
                            if top {
                                bg
                            } else {
                                fg
                            }
                        }
                        " " => bg,
                        _ => {
                            if py == ch / 2 && px == 0 {
                                fg
                            } else {
                                bg
                            }
                        }
                    };
                    let i = ((y * ch + py) * w * cw + x * cw + px) * 3;
                    img[i..i + 3].copy_from_slice(&[color.0, color.1, color.2]);
                }
            }
        }
    }
    let mut out = format!("P6\n{} {}\n255\n", w * cw, h * ch).into_bytes();
    out.extend_from_slice(&img);
    std::fs::write(path, out)
}

fn set_mouse(on: bool) {
    let _ = if on {
        crossterm::execute!(std::io::stdout(), EnableMouseCapture)
    } else {
        crossterm::execute!(std::io::stdout(), DisableMouseCapture)
    };
}

fn run(view: View, interval_ms: u64, cfg: config::Config, cfg_path: PathBuf, cfg_msg: Option<String>) -> std::io::Result<()> {
    let (host, rx, ctx) = collect::spawn(Duration::from_millis(interval_ms));
    let mouse = cfg.mouse;
    let mut app = App::new(view, host, interval_ms, cfg, cfg_path, cfg_msg);
    let mut terminal = ratatui::init();
    if mouse {
        set_mouse(true);
    }
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
            // Apply side effects requested by the settings view.
            for e in std::mem::take(&mut app.effects) {
                match e {
                    Effect::Interval(ms) => {
                        let _ = ctx.send(collect::Cmd::Interval(Duration::from_millis(ms)));
                    }
                    Effect::Mouse(on) => set_mouse(on),
                }
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
                        let shift = k.modifiers.contains(KeyModifiers::SHIFT);
                        let in_settings = app.view == View::Settings && !app.show_help;
                        match k.code {
                            KeyCode::Char('q') | KeyCode::Esc => {
                                if app.show_help {
                                    app.show_help = false;
                                } else if k.code == KeyCode::Esc && in_settings {
                                    app.view = app.cfg.default_view;
                                } else {
                                    break;
                                }
                            }
                            KeyCode::Char('c') if ctrl => break,
                            KeyCode::Char('?') => app.show_help = !app.show_help,
                            KeyCode::Char(',') => app.view = if in_settings { app.cfg.default_view } else { View::Settings },
                            KeyCode::Tab => app.next_view(),
                            KeyCode::BackTab => app.prev_view(),
                            KeyCode::Char(c @ '1'..='9') => app.jump(c as usize - '0' as usize),
                            // Settings-specific keys first, so they take precedence over global ones.
                            KeyCode::Up | KeyCode::Char('K') if in_settings && (shift || k.code == KeyCode::Char('K')) => {
                                app.settings_reorder(-1)
                            }
                            KeyCode::Down | KeyCode::Char('J') if in_settings && (shift || k.code == KeyCode::Char('J')) => {
                                app.settings_reorder(1)
                            }
                            KeyCode::Char('[') if in_settings => app.settings_reorder(-1),
                            KeyCode::Char(']') if in_settings => app.settings_reorder(1),
                            KeyCode::Left | KeyCode::Char('h') if in_settings => app.settings_cycle(-1),
                            KeyCode::Right | KeyCode::Char('l') if in_settings => app.settings_cycle(1),
                            KeyCode::Char(' ') | KeyCode::Enter if in_settings => app.settings_toggle(),
                            KeyCode::Char('d') if in_settings => app.settings_reset(),
                            // Global keys.
                            KeyCode::Right | KeyCode::Char('l') => app.next_view(),
                            KeyCode::Left | KeyCode::Char('h') => app.prev_view(),
                            KeyCode::Down | KeyCode::Char('j') => app.select_down(),
                            KeyCode::Up | KeyCode::Char('k') => app.select_up(),
                            KeyCode::Home => {
                                app.proc_sel = 0;
                                app.wifi_sel = 0;
                                app.settings_sel = 0;
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
                    Event::Mouse(m) => match m.kind {
                        MouseEventKind::Down(MouseButton::Left) => {
                            app.click(m.column, m.row);
                            dirty = true;
                        }
                        MouseEventKind::ScrollDown => {
                            app.scroll(true, m.column, m.row);
                            dirty = true;
                        }
                        MouseEventKind::ScrollUp => {
                            app.scroll(false, m.column, m.row);
                            dirty = true;
                        }
                        _ => {}
                    },
                    Event::Resize(_, _) => dirty = true,
                    _ => {}
                }
            }
        }
        Ok(())
    })();
    let _ = ctx.send(collect::Cmd::Quit);
    set_mouse(false);
    ratatui::restore();
    result
}
