# sysfun

Fast, elegant system information in your terminal. A Rust TUI for Linux.

Every view opens with a plain-language summary that anyone can read, then gets
progressively more technical the further down the screen you go. The Wi-Fi view
is the clearest example: the top says whether your connection is good and what
it is good for; the bottom shows nl80211 station counters, per-chain signal,
BSS load, PHY capabilities, regulatory domain and channel congestion.

```
sysfun            # overview of everything
sysfun --wifi     # signal, link rates, nearby networks, RF engineering detail
sysfun --bat      # charge, time left, wattage, health, chargers  (aliases: --battery, --power)
sysfun --disk     # filesystems, drives, live I/O, partition layout
sysfun --cpu      # per-core load and clocks, scheduler stats, pressure (PSI)
sysfun --mem      # RAM, swap, zram, kernel accounting, biggest consumers
sysfun --net      # interfaces, routes, DNS, sockets, TCP retransmits
sysfun --temp     # every hwmon sensor, thermal zones, cooling devices
sysfun --ps       # processes sorted by cpu / memory / pid / name
sysfun --once     # print one frame as plain text and exit (great for pasting into a ticket)
sysfun -i 250     # refresh every 250 ms
```

`sysfun --help` lists every flag and alias.

## Contents

- [Install](#install)
- [Views](#views)
- [Keys](#keys)
- [Non-interactive use](#non-interactive-use)
- [Design principles](#design-principles)
- [Performance](#performance)
- [Requirements and privacy](#requirements-and-privacy)
- [Troubleshooting](#troubleshooting)
- [Architecture](docs/ARCHITECTURE.md)
- [Contributing](CONTRIBUTING.md)

## Install

Prebuilt binaries are not published yet. Build from source with a stable Rust
toolchain (1.87 or newer):

```sh
git clone https://github.com/richvincent/sysfun
cd sysfun
cargo build --release
install -Dm755 target/release/sysfun ~/.local/bin/sysfun
```

Or, with Cargo managing the install location:

```sh
cargo install --path .
```

The result is a single static-ish binary that links only `libc` and `libgcc`.
It needs no configuration files and writes nothing to disk.

## Views

Each view is a tab in the TUI and a flag on the command line.

| view | flag | what it answers |
|------|------|-----------------|
| Overview | (default) | Is this machine healthy right now? Host, CPU, memory, swap, battery, Wi-Fi, disks, I/O, hottest sensor, busiest processes. |
| CPU | `--cpu` | How busy is the processor, which cores, at what clock? Load average with a plain reading, user/system/iowait split, per-core gauges with current and max frequency, context switches, interrupts, forks, governor and driver, CPU pressure (PSI). |
| Memory | `--mem` | Am I running out of RAM? Used, cache and swap gauges, a plain verdict, full `/proc/meminfo` breakdown, slab, commit limit, huge pages, zram compression ratio, memory pressure, biggest processes by resident size. |
| Disk | `--disk` | How full are my drives and how hard are they working? Filesystems with usage bars (btrfs subvolumes folded into one row), block devices with live read/write throughput, IOPS, busy % and queue depth, partition layout with filesystem types and labels, I/O history, I/O pressure. |
| Network | `--net` | Am I online and what is my address? Default route and gateway, DNS (including the systemd-resolved upstream), IPv6, per-interface addresses, rates, totals, errors and drops, socket counts, TCP retransmit rate. |
| Wi-Fi | `--wifi` | Is my Wi-Fi good? Then everything a wireless engineer would ask. See [below](#the-wi-fi-view). |
| Power | `--bat` | How long will the battery last and is it healthy? Charge gauge, time left in words, current draw with a plain reading, health versus design capacity, cycle count, full electrical and identity detail, chargers and USB-PD sources, platform power rails, draw history. |
| Thermal | `--temp` | Is anything hot? Hottest sensor with a verdict, fans, every hwmon reading grouped by kind with limits, thermal zones, cooling devices, temperature history. |
| Processes | `--ps` | What is using the machine? Sortable table with state, CPU %, memory %, RSS, threads, nice, CPU time, age and full command line. |

### The Wi-Fi view

From top to bottom:

1. **In plain English.** Connected or not, signal as bars and a word
   ("Good"), what the link speed is good for ("plenty for 4K video"), which
   band you are on and what that means, whether the network is password
   protected, and one tip if something is off (weak signal, open network,
   crowded channel, no address yet).
2. **Connection.** SSID, access point BSSID and country, channel, frequency
   and width, Wi-Fi generation, security suite, time connected, gateway,
   power-save state, signal with average and per-chain values, noise floor and
   SNR when the driver reports them, link quality, TX and RX rate strings
   straight from nl80211 (with MCS, NSS and guard interval when available),
   expected throughput, TX power, BSS load (client count and airtime
   utilisation from the AP's beacon).
3. **Nearby networks.** Every BSS the kernel or NetworkManager has seen:
   SSID, BSSID, channel, band, width, signal in dBm and bars, security,
   generation (Wi-Fi 4/5/6/6E/7 from HT/VHT/HE/EHT elements), client count,
   airtime busy %, and how long ago it was seen. The associated network is
   starred. Arrow keys scroll; `s` asks NetworkManager to rescan.
4. **RF engineering.** Station flags (authorized, authenticated, WMM, MFP),
   DTIM and beacon interval, inactivity, ACK signal; counters for packets,
   bytes, TX failures and retries with percentages, beacon loss, discards and
   missed beacons; the radio's driver and phy, regulatory domain, supported
   bands and PHY generations, antenna masks, cipher and interface-mode counts;
   the busiest channels nearby and how many other APs share your channel;
   signal and TX-rate history sparklines.

## Keys

| key | action |
|-----|--------|
| `Tab` `→` `l` / `Shift-Tab` `←` `h` | next / previous view |
| `1` … `9` | jump to a view |
| `↑` `↓` `j` `k` `Home` | move selection in the process and Wi-Fi tables |
| `c` `m` `p` `n` | sort processes by cpu, memory, pid or name |
| `s` | rescan Wi-Fi networks (via NetworkManager) |
| `space` | pause / resume sampling |
| `r` | sample now |
| `+` `-` | halve / double the refresh interval |
| `?` | help overlay |
| `q` `Esc` `Ctrl-C` | quit |

The footer shows the host, the tick interval, how long the last sample and
draw took, and which collector was slowest.

## Non-interactive use

`--once` renders a single frame to plain text (no colours, no alternate
screen) and exits. It defaults to your terminal size; `--width` and
`--height` override it.

```sh
sysfun --wifi --once --width 160 --height 50 > wifi-report.txt
sysfun --once | mail -s "$(hostname) status" you@example.com
```

`SYSFUN_TIMING=1` prints per-collector cost in milliseconds to stderr on
every sample, which is useful when tuning or when a view feels slow.

## Design principles

- **Top is for people, bottom is for experts.** Each section starts with a
  verdict in words and ends with raw counters. Nothing is hidden; it is just
  ordered.
- **Read the kernel, not a library.** All data comes from `/proc` and `/sys`
  via small, purpose-built parsers. The only subprocesses are `iw` and
  `nmcli`, used for Wi-Fi where nl80211 has no file interface.
- **Never block the screen.** Collection runs on a background thread; Wi-Fi
  (which spawns processes) runs on a second one; network and FUSE mounts are
  measured with a timeout. The UI thread only draws when there is a new
  snapshot or a key press.
- **Degrade gracefully.** No battery, no Wi-Fi card, no `iw`, no PSI, no
  sensors: every view explains what is missing instead of failing.
- **Fit any terminal.** Layouts are tested from 80x24 up. Columns are dropped
  before text is truncated where it matters.

## Performance

A full sample on an Apple M1 laptop running ~360 processes takes 25 to 35 ms
in release mode. The cost is dominated by two things that are hardware, not
code: reading Apple SMC sensors through sysfs (each read is a round-trip to
the management controller) and spawning `iw`. To keep that off the main tick:

- Wi-Fi collection runs on its own thread at a fixed one-second cadence and
  the main sampler clones the latest result.
- `iw info` and power-save state are refreshed every ten seconds; `iw scan
  dump` and `nmcli` every five; `iw phy info` once.
- Sensor and power-rail paths are discovered once (and re-discovered every
  60 ticks), so each tick reads only the `*_input` files.
- Process command lines are cached per `(pid, start time)` and read only for
  the processes that could be displayed.
- Rates (CPU, disk, network, TCP) come from deltas against the previous
  sample, so the first useful frame appears about 250 ms after launch.

## Requirements and privacy

- Linux with `/proc` and `/sys`. Tested on Arch Linux ARM (Asahi, kernel 7.x);
  nothing is architecture-specific.
- Optional: `iw` for Wi-Fi detail, `nmcli` for a longer-lived scan cache and
  on-demand rescans. Without them the Wi-Fi view falls back to
  `/proc/net/wireless`.
- No root required. Everything read is world-readable sysfs/procfs; unmounted
  partition types come from the udev database rather than `blkid`.
- **No network access, ever.** sysfun opens no sockets, phones nothing home,
  and works fully offline. It writes nothing to disk and has no config file.

## Troubleshooting

**Only one nearby network is listed.** The kernel drops scan results about
30 seconds after they were last seen, and NetworkManager only scans
periodically. Press `s` to request a rescan; results appear within a few
seconds.

**Noise floor says "not reported by driver".** Many drivers (brcmfmac among
them) do not expose a noise estimate through nl80211. SNR is shown whenever
noise is available.

**Channel width or generation shows "—" or "~Wi-Fi 5".** That row came from
NetworkManager's cache rather than the kernel's, so no information elements
were available; the generation is inferred from the advertised max rate and
marked with a tilde.

**A remote mount shows "(slow)".** Its `statvfs` call did not return within
400 ms. The last good numbers are kept and the mount is retried every ten
ticks.

**The battery shows "(estimated)" time.** The kernel driver did not supply a
time-to-empty/full, so it is computed from energy and current power draw.

## License

MIT. See [LICENSE](LICENSE).
