//! User preferences, persisted as a small TOML subset in `~/.config/sysfun/config.toml`.
//!
//! The file is written by the in-app Settings tab and is also hand-editable. Parsing is
//! deliberately tiny (strings, integers, booleans, arrays of strings) so sysfun has no
//! serialization dependencies.

use crate::app::View;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThemeName {
    Default,
    Mono,
}

impl ThemeName {
    pub fn slug(self) -> &'static str {
        match self {
            ThemeName::Default => "default",
            ThemeName::Mono => "mono",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            ThemeName::Default => "colour accents",
            ThemeName::Mono => "monochrome",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// Every content view in display order, with its visibility.
    pub tabs: Vec<(View, bool)>,
    pub default_view: View,
    pub interval_ms: u64,
    pub fahrenheit: bool,
    pub theme: ThemeName,
    pub mouse: bool,
    pub show_hidden_networks: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            tabs: View::ALL.iter().map(|v| (*v, true)).collect(),
            default_view: View::Overview,
            interval_ms: 1000,
            fahrenheit: false,
            theme: ThemeName::Default,
            mouse: true,
            show_hidden_networks: true,
        }
    }
}

pub const INTERVAL_STEPS: [u64; 6] = [250, 500, 1000, 2000, 5000, 10000];

pub fn default_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("sysfun").join("config.toml")
}

impl Config {
    /// Load from `path`; a missing file yields defaults. A malformed file yields defaults
    /// plus a message, so a typo never prevents the app from starting.
    pub fn load(path: &Path) -> (Config, Option<String>) {
        match std::fs::read_to_string(path) {
            Ok(text) => Config::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Config::default(), None),
            Err(e) => (Config::default(), Some(format!("could not read {}: {}", path.display(), e))),
        }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // Write to a sibling and rename so a crash mid-write never leaves a truncated file.
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, self.render())?;
        std::fs::rename(&tmp, path)
    }

    pub fn visible_tabs(&self) -> Vec<View> {
        self.tabs.iter().filter(|(_, on)| *on).map(|(v, _)| *v).collect()
    }

    /// Make sure every view appears exactly once and at least one is visible.
    pub fn normalize(&mut self) {
        let mut seen: Vec<View> = Vec::new();
        self.tabs.retain(|(v, _)| {
            if seen.contains(v) {
                false
            } else {
                seen.push(*v);
                true
            }
        });
        for v in View::ALL {
            if !seen.contains(&v) {
                self.tabs.push((v, true));
            }
        }
        if !self.tabs.iter().any(|(_, on)| *on) {
            self.tabs[0].1 = true;
        }
        if !INTERVAL_STEPS.contains(&self.interval_ms) {
            self.interval_ms = self.interval_ms.clamp(100, 60_000);
        }
    }

    fn parse(text: &str) -> (Config, Option<String>) {
        let mut cfg = Config::default();
        let mut problems: Vec<String> = Vec::new();
        let mut order: Option<Vec<View>> = None;
        let mut hidden: Vec<View> = Vec::new();
        for (n, raw) in text.lines().enumerate() {
            let line = strip_comment(raw).trim();
            if line.is_empty() {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                problems.push(format!("line {}: expected key = value", n + 1));
                continue;
            };
            let (k, v) = (k.trim(), v.trim());
            match k {
                "tabs" => {
                    let mut vs = Vec::new();
                    for s in parse_array(v) {
                        match View::from_slug(&s) {
                            Some(view) => vs.push(view),
                            None => problems.push(format!("line {}: unknown tab \"{}\"", n + 1, s)),
                        }
                    }
                    order = Some(vs);
                }
                "hidden_tabs" => {
                    for s in parse_array(v) {
                        match View::from_slug(&s) {
                            Some(view) => hidden.push(view),
                            None => problems.push(format!("line {}: unknown tab \"{}\"", n + 1, s)),
                        }
                    }
                }
                "default_view" => match View::from_slug(&unquote(v)) {
                    Some(view) => cfg.default_view = view,
                    None => problems.push(format!("line {}: unknown view \"{}\"", n + 1, unquote(v))),
                },
                "interval_ms" => match v.parse::<u64>() {
                    Ok(ms) => cfg.interval_ms = ms.clamp(100, 60_000),
                    Err(_) => problems.push(format!("line {}: interval_ms must be a number", n + 1)),
                },
                "temperature" => cfg.fahrenheit = unquote(v).starts_with('f') || unquote(v).starts_with('F'),
                "theme" => {
                    cfg.theme = match unquote(v).as_str() {
                        "mono" | "monochrome" => ThemeName::Mono,
                        _ => ThemeName::Default,
                    }
                }
                "mouse" => cfg.mouse = v == "true",
                "show_hidden_networks" => cfg.show_hidden_networks = v == "true",
                _ => problems.push(format!("line {}: unknown key \"{}\"", n + 1, k)),
            }
        }
        if let Some(order) = order {
            cfg.tabs = order.into_iter().map(|v| (v, !hidden.contains(&v))).collect();
        } else {
            for (v, on) in cfg.tabs.iter_mut() {
                *on = !hidden.contains(v);
            }
        }
        cfg.normalize();
        let msg = if problems.is_empty() { None } else { Some(problems.join("; ")) };
        (cfg, msg)
    }

    fn render(&self) -> String {
        let order: Vec<String> = self.tabs.iter().map(|(v, _)| format!("\"{}\"", v.slug())).collect();
        let hidden: Vec<String> = self.tabs.iter().filter(|(_, on)| !*on).map(|(v, _)| format!("\"{}\"", v.slug())).collect();
        format!(
            "# sysfun configuration. Edited in-app from the Settings tab (press ,), or by hand.\n\
             # Tab order, left to right. Valid names: {}\n\
             tabs = [{}]\n\
             # Tabs to leave out of the bar (they stay reachable from Settings).\n\
             hidden_tabs = [{}]\n\
             # View shown at launch when no flag is given.\n\
             default_view = \"{}\"\n\
             # Sampling interval in milliseconds (100–60000).\n\
             interval_ms = {}\n\
             # \"celsius\" or \"fahrenheit\".\n\
             temperature = \"{}\"\n\
             # \"default\" (a colour per view) or \"mono\".\n\
             theme = \"{}\"\n\
             # Capture mouse clicks and wheel.\n\
             mouse = {}\n\
             # List Wi-Fi networks that do not broadcast a name.\n\
             show_hidden_networks = {}\n",
            View::ALL.iter().map(|v| v.slug()).collect::<Vec<_>>().join(", "),
            order.join(", "),
            hidden.join(", "),
            self.default_view.slug(),
            self.interval_ms,
            if self.fahrenheit { "fahrenheit" } else { "celsius" },
            self.theme.slug(),
            self.mouse,
            self.show_hidden_networks,
        )
    }
}

fn strip_comment(line: &str) -> &str {
    let mut in_str = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_str = !in_str,
            '#' if !in_str => return &line[..i],
            _ => {}
        }
    }
    line
}

fn unquote(v: &str) -> String {
    v.trim().trim_matches('"').to_string()
}

fn parse_array(v: &str) -> Vec<String> {
    let inner = v.trim().trim_start_matches('[').trim_end_matches(']');
    inner.split(',').map(|s| s.trim().trim_matches('"').to_string()).filter(|s| !s.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let mut c = Config::default();
        c.tabs.swap(0, 3);
        c.tabs[2].1 = false;
        c.default_view = View::Wifi;
        c.interval_ms = 500;
        c.fahrenheit = true;
        c.theme = ThemeName::Mono;
        c.mouse = false;
        c.show_hidden_networks = false;
        let (back, msg) = Config::parse(&c.render());
        assert_eq!(msg, None);
        assert_eq!(back, c);
    }

    #[test]
    fn tolerates_garbage() {
        let (c, msg) = Config::parse("tabs = [\"wifi\", \"nope\"]\nbogus = 1\ninterval_ms = abc\n");
        assert!(msg.is_some());
        assert_eq!(c.tabs.len(), View::ALL.len());
        assert_eq!(c.tabs[0].0, View::Wifi);
        assert_eq!(c.interval_ms, 1000);
    }
}
