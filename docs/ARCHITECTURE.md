# Architecture

sysfun is a single binary with three concerns kept in separate modules:
collecting data, holding application state, and drawing.

```
src/
├── main.rs            CLI (clap), terminal setup, event loop, --once renderer
├── app.rs             App state: current view, latest Snapshot, history ring buffers, selections
├── util.rs            Formatting (bytes, rates, durations, bars) and tiny file/parse helpers
├── collect/
│   ├── mod.rs         Snapshot struct, Sampler (one tick = one Snapshot), background thread, Cmd channel
│   ├── host.rs        Static facts read once: hostname, OS, kernel, machine model, CPU model
│   ├── cpu.rs         /proc/stat deltas, per-core cpufreq, loadavg, PSI
│   ├── mem.rs         /proc/meminfo, zram mm_stat, PSI
│   ├── disk.rs        /proc/self/mounts + statvfs, /sys/block, /proc/diskstats deltas, udev db
│   ├── net.rs         /sys/class/net, getifaddrs(3), /proc/net/route, resolv.conf, sockstat, snmp
│   ├── wifi.rs        iw link/info/station/scan/phy/reg parsers, nmcli scan merge, /proc/net/wireless
│   ├── power.rs       /sys/class/power_supply uevent, hwmon power rails, backlight, governor
│   ├── thermal.rs     hwmon temp/fan/in/curr/power inputs, thermal zones, cooling devices
│   └── proc.rs        /proc/<pid>/stat deltas, cmdline cache, uid→user map
└── ui/
    ├── mod.rs         Frame layout: header tabs, body dispatch, footer, help overlay
    ├── widgets.rs     Theme (per-view accent colours), section blocks, gauges, sparklines, kv tables
    └── {overview,cpu,mem,disk,net,wifi,power,thermal,procs}.rs   one draw() per view
```

## Data flow

```
                 ┌────────────── sysfun-sampler thread ──────────────┐
 /proc, /sys ───►│ Sampler::sample()  ──►  Snapshot (owned, cloned)  │──► mpsc ──► UI thread
                 │   cpu/mem/disk/net/power/thermal/procs collectors │           App::accept()
                 └───────────────────────────▲───────────────────────┘           History::record()
                                             │ Mutex<WifiSnap> clone                ui::draw()
                 ┌────────────── sysfun-wifi thread ────────────────┐
 iw, nmcli ─────►│ WifiSampler::sample() every 1 s; scan every 5 s  │
                 └──────────────────────────────────────────────────┘
```

- **Sampler** owns one stateful sampler per collector that needs deltas
  (`CpuSampler`, `DiskSampler`, `NetSampler`, `ProcSampler`) and one per
  collector that caches discovery (`PowerSampler`, `ThermalSampler`). Each
  tick produces a complete `Snapshot` so the UI never sees a half-updated
  state.
- **Wi-Fi** is the only collector that spawns processes, so it lives on its
  own thread and publishes into an `Arc<Mutex<WifiSnap>>`. The main sampler
  clones the latest value (sub-millisecond). In `--once` mode the same
  `WifiSampler` is called synchronously.
- **Commands** flow the other way over a second channel: change interval,
  sample now, rescan Wi-Fi, quit.
- **History** lives in the UI (`app::History`): fixed-size ring buffers of
  `u64` fed from each snapshot, rendered as sparklines. Keeping history out
  of the collectors keeps `Snapshot` cheap to clone.

## Rendering

- ratatui with the crossterm backend. `ratatui::init()` installs a panic hook
  that restores the terminal.
- `ui::draw` lays out header / body / footer, draws the tab bar, dispatches to
  the current view, then overlays help if toggled.
- Views are plain functions `fn draw(frame, area, &mut App, &Theme)`. They
  compute their own vertical split from the content (for example the CPU view
  sizes the per-core block from the core count) and use `Min` constraints for
  the one section that should absorb leftover space.
- `widgets::Theme::for_view` gives each view an accent colour; `level()` and
  `level_inv()` map a 0–100 value to green/yellow/red so colour meaning is
  consistent everywhere (high usage is bad, high battery is good).
- `--once` renders to ratatui's `TestBackend` and prints the cell buffer,
  which guarantees the text output is exactly what the TUI would show.

## Data sources by view

| view | files and commands |
|------|--------------------|
| host | `/proc/sys/kernel/{hostname,osrelease}`, `/etc/os-release`, `/proc/device-tree/model` or `/sys/class/dmi/id/*`, `/proc/cpuinfo`, `/sys/devices/system/cpu/cpu*/of_node/compatible` (ARM core types), `/proc/stat` btime |
| cpu | `/proc/stat` (cpu lines, ctxt, intr, processes, procs_running/blocked), `/proc/loadavg`, `/sys/devices/system/cpu/cpu*/cpufreq/*`, `/proc/pressure/cpu` |
| mem | `/proc/meminfo`, `/sys/block/zram*/{mm_stat,disksize,comp_algorithm}`, `/proc/pressure/memory` |
| disk | `/proc/self/mounts` (pseudo filesystems filtered, same-device mounts folded), `statvfs(3)` (with a 400 ms timeout for network/FUSE), `/sys/block/*` (size, model, serial, firmware, rotational, scheduler, dm name/uuid, slaves, partitions), `/run/udev/data/b<maj>:<min>` (fs type/label of unmounted partitions), `/proc/diskstats`, `/proc/pressure/io` |
| net | `/sys/class/net/*` (operstate, carrier, address, mtu, speed, duplex, driver symlink, statistics), `getifaddrs(3)`, `/proc/net/route`, `/proc/net/ipv6_route`, `/etc/resolv.conf` and `/run/systemd/resolve/resolv.conf`, `/proc/net/sockstat`, `/proc/net/snmp` |
| wifi | `iw dev X link`, `iw dev X info`, `iw dev X station dump`, `iw dev X get power_save`, `iw dev X scan dump`, `iw phy phyN info` (once), `iw reg get`, `nmcli -t dev wifi list --rescan no` (merge), `nmcli dev wifi rescan` (on `s`), `/proc/net/wireless`, `/sys/class/net/X/{address,phy80211/name,device/driver}` |
| power | `/sys/class/power_supply/*/uevent` (one read per supply), `/sys/class/hwmon/*/power*_input` + `_label`, `/sys/class/backlight/*`, `cpufreq/scaling_governor`, `energy_performance_preference`, `/sys/firmware/acpi/platform_profile`, `/proc/acpi/button/lid/*` |
| thermal | `/sys/class/hwmon/*/{name,temp*,fan*,in*,curr*,power*}` with `_label`, `_max`, `_crit`; `/sys/class/thermal/thermal_zone*`, `cooling_device*` |
| procs | `/proc/<pid>/stat`, `/proc/<pid>/cmdline` (cached per pid+starttime, only for displayable processes), directory owner uid, `/etc/passwd`, `sysconf(_SC_CLK_TCK)`, `_SC_PAGESIZE` |

## Wi-Fi parsing notes

- `iw` output is `key: value` lines indented with tabs. Scan dumps are
  blocks starting at `BSS <bssid>(on <iface>)`; nested information elements
  are indented further and prefixed with `*`. The parser tracks the current
  top-level section so that `Authentication suites` is only read inside
  `RSN:`/`WPA:`.
- Channel width: VHT/HE operation reports `channel width: 0 (20 or 40 MHz)`
  when it defers to HT; in that case the HT operation element's
  `STA channel width` or `secondary channel offset` decides.
- Security label: SAE → WPA3, PSK → WPA2 (or WPA if only a WPA IE), 802.1X →
  Enterprise, OWE → Enhanced Open, `Privacy` capability without RSN/WPA → WEP,
  otherwise Open.
- Generation: EHT → Wi-Fi 7, HE → Wi-Fi 6 (6E on 6 GHz), VHT → Wi-Fi 5,
  HT → Wi-Fi 4, else legacy. NetworkManager rows have no IEs, so generation
  is inferred from max rate and prefixed with `~`.
- NetworkManager signal is a 0–100 percentage; it is mapped back to dBm with
  `pct / 2 - 100`, the inverse of NM's own formula.
- cfg80211 expires BSS entries ~30 s after last seen, which is why the NM
  cache is merged and why `s` triggers `nmcli dev wifi rescan` (works
  unprivileged through polkit for local sessions).

## Adding a view

1. Add a collector in `src/collect/` returning a `Clone + Default` snapshot
   type and wire it into `Snapshot` and `Sampler::sample` (wrap it in
   `timed(...)` so it shows up in `SYSFUN_TIMING`).
2. Add a `View` variant in `app.rs` (and to `View::ALL`, `title`), a flag in
   `main.rs`, and an accent colour in `widgets::Theme::for_view`.
3. Write `src/ui/<view>.rs` with `pub fn draw(frame, area, app, th)`; start
   with a "plain English" section and end with the raw numbers.
4. Dispatch it from `ui::draw`.
