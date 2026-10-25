//! Small formatting and file helpers shared by collectors and UI.

use std::fs;
use std::path::Path;

pub fn read(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

pub fn read_u64(path: impl AsRef<Path>) -> Option<u64> {
    read(path)?.parse().ok()
}

pub fn read_f64(path: impl AsRef<Path>) -> Option<f64> {
    read(path)?.parse().ok()
}

/// Binary (IEC) byte size: "7.4 GiB", "389 GiB", "512 KiB".
pub fn bytes(n: u64) -> String {
    const U: [&str; 7] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{} B", n)
    } else if v < 10.0 {
        format!("{:.2} {}", v, U[i])
    } else if v < 100.0 {
        format!("{:.1} {}", v, U[i])
    } else {
        format!("{:.0} {}", v, U[i])
    }
}

/// Decimal (SI) rate: "12.3 MB/s".
pub fn rate(bps: f64) -> String {
    const U: [&str; 6] = ["B/s", "kB/s", "MB/s", "GB/s", "TB/s", "PB/s"];
    let mut v = bps.max(0.0);
    let mut i = 0;
    while v >= 1000.0 && i < U.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 {
        format!("{:.0} {}", v, U[i])
    } else if v < 10.0 {
        format!("{:.2} {}", v, U[i])
    } else if v < 100.0 {
        format!("{:.1} {}", v, U[i])
    } else {
        format!("{:.0} {}", v, U[i])
    }
}

/// Bits per second: "173 Mbit/s", "1.2 Gbit/s".
pub fn bits(bps: f64) -> String {
    const U: [&str; 5] = ["bit/s", "kbit/s", "Mbit/s", "Gbit/s", "Tbit/s"];
    let mut v = bps.max(0.0);
    let mut i = 0;
    while v >= 1000.0 && i < U.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if v < 10.0 && i > 0 {
        format!("{:.1} {}", v, U[i])
    } else {
        format!("{:.0} {}", v, U[i])
    }
}

/// Compact duration: "3h 23m", "2d 4h", "45s".
pub fn dur(secs: u64) -> String {
    let d = secs / 86400;
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if d > 0 {
        format!("{}d {}h", d, h)
    } else if h > 0 {
        format!("{}h {:02}m", h, m)
    } else if m > 0 {
        format!("{}m {:02}s", m, s)
    } else {
        format!("{}s", s)
    }
}

/// Friendly duration: "about 2 hours 9 minutes".
pub fn dur_words(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    match (h, m) {
        (0, 0) => "under a minute".into(),
        (0, m) => format!("about {} minute{}", m, if m == 1 { "" } else { "s" }),
        (h, 0) => format!("about {} hour{}", h, if h == 1 { "" } else { "s" }),
        (h, m) => format!("about {} hour{} {} minute{}", h, if h == 1 { "" } else { "s" }, m, if m == 1 { "" } else { "s" }),
    }
}

/// Unicode block bar of `width` cells filled to `ratio`.
pub fn bar(ratio: f64, width: usize) -> String {
    let ratio = ratio.clamp(0.0, 1.0);
    let cells = ratio * width as f64;
    let full = cells.floor() as usize;
    let frac = cells - full as f64;
    let mut s = String::with_capacity(width * 3);
    for _ in 0..full {
        s.push('█');
    }
    if full < width {
        let partial = ['░', '▏', '▎', '▍', '▌', '▋', '▊', '▉'];
        let idx = (frac * 8.0).floor() as usize;
        if idx > 0 {
            s.push(partial[idx.min(7)]);
        } else {
            s.push('░');
        }
        for _ in (full + 1)..width {
            s.push('░');
        }
    }
    s
}

/// Signal bars like a phone: 0–4 filled of ▂▄▆█.
pub fn signal_bars(level: usize) -> String {
    let glyphs = ['▂', '▄', '▆', '█'];
    let mut s = String::new();
    for (i, g) in glyphs.iter().enumerate() {
        if i < level {
            s.push(*g);
        } else {
            s.push('·');
        }
    }
    s
}

/// Parse "key: value" with the first ':' as separator.
pub fn kv(line: &str) -> Option<(String, String)> {
    let (k, v) = line.split_once(':')?;
    Some((k.trim().to_ascii_lowercase(), v.trim().to_string()))
}

/// First signed integer in a string ("-57 [-56, -59] dBm" -> -57).
pub fn first_int(s: &str) -> Option<i64> {
    let mut start = None;
    let b = s.as_bytes();
    for (i, c) in b.iter().enumerate() {
        if c.is_ascii_digit() || (*c == b'-' && i + 1 < b.len() && b[i + 1].is_ascii_digit()) {
            start = Some(i);
            break;
        }
    }
    let st = start?;
    let mut end = st + 1;
    while end < b.len() && b[end].is_ascii_digit() {
        end += 1;
    }
    s[st..end].parse().ok()
}

pub fn first_float(s: &str) -> Option<f64> {
    s.split(|c: char| c.is_whitespace() || c == '/' || c == '(' || c == ')' || c == '[' || c == ']' || c == ',')
        .find_map(|t| t.parse::<f64>().ok())
}

pub fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out =
        std::process::Command::new(cmd).args(args).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output().ok()?;
    if out.status.success() {
        String::from_utf8(out.stdout).ok()
    } else {
        None
    }
}

pub fn which(cmd: &str) -> bool {
    std::env::var_os("PATH").map(|p| std::env::split_paths(&p).any(|d| d.join(cmd).is_file())).unwrap_or(false)
}

pub fn now_epoch() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Local wall-clock "HH:MM:SS" without pulling in chrono (uses libc localtime_r).
pub fn clock() -> String {
    let t = now_epoch() as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe {
        libc::localtime_r(&t, &mut tm);
    }
    format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec)
}
