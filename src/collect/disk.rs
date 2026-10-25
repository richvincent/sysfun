//! Filesystems (statvfs) and block-device I/O rates (/proc/diskstats).

use super::cpu::{read_psi, Psi};
use crate::util::{read, read_u64};
use std::collections::HashMap;
use std::ffi::CString;
use std::time::Duration;

#[derive(Clone, Debug, Default)]
pub struct Mount {
    pub device: String,
    pub mount: String,
    pub fstype: String,
    pub total: u64,
    pub used: u64,
    pub avail: u64,
    pub inodes_total: u64,
    pub inodes_used: u64,
    pub opts: String,
    pub extra_mounts: Vec<String>,
    pub remote: bool,
    pub stale: bool,
}

impl Mount {
    pub fn used_pct(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.used as f64 / self.total as f64 * 100.0
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Partition {
    pub name: String,
    pub size: u64,
    pub mount: Option<String>,
    pub fstype: Option<String>,
    pub label: Option<String>,
}

/// Filesystem type / label for an unmounted partition from the udev database (no root needed).
fn udev_fs(base: &str, part: &str) -> (Option<String>, Option<String>) {
    let Some(devno) = read(format!("{}/{}/dev", base, part)) else { return (None, None) };
    let Some(db) = read(format!("/run/udev/data/b{}", devno)) else { return (None, None) };
    let mut fstype = None;
    let mut label = None;
    for l in db.lines() {
        if let Some(v) = l.strip_prefix("E:ID_FS_TYPE=") {
            fstype = Some(v.to_string());
        } else if let Some(v) = l.strip_prefix("E:ID_FS_LABEL=") {
            label = Some(v.to_string());
        } else if let Some(v) = l.strip_prefix("E:ID_PART_ENTRY_NAME=") {
            if label.is_none() && !v.is_empty() {
                label = Some(v.to_string());
            }
        }
    }
    (fstype, label)
}

#[derive(Clone, Debug, Default)]
pub struct BlockDev {
    pub name: String,
    pub model: Option<String>,
    pub serial: Option<String>,
    pub firmware: Option<String>,
    pub size: u64,
    pub rotational: bool,
    pub transport: String,
    pub scheduler: Option<String>,
    pub read_bps: f64,
    pub write_bps: f64,
    pub read_iops: f64,
    pub write_iops: f64,
    pub util_pct: f64,
    pub in_flight: u64,
    pub total_read: u64,
    pub total_written: u64,
    pub parts: Vec<Partition>,
    pub mapper_name: Option<String>,
    pub slaves: Vec<String>,
    pub discard: bool,
    pub removable: bool,
}

#[derive(Clone, Debug, Default)]
pub struct DiskSnap {
    pub mounts: Vec<Mount>,
    pub devices: Vec<BlockDev>,
    pub total_read_bps: f64,
    pub total_write_bps: f64,
    pub psi: Option<Psi>,
}

#[derive(Clone, Copy, Default)]
struct Raw {
    reads: u64,
    rsect: u64,
    writes: u64,
    wsect: u64,
    in_flight: u64,
    io_ms: u64,
}

pub struct DiskSampler {
    prev: HashMap<String, Raw>,
    remote_cache: HashMap<String, (u64, u64, u64, u64, u64)>,
    tick: u64,
}

const PSEUDO_FS: &[&str] = &[
    "proc",
    "sysfs",
    "devtmpfs",
    "devpts",
    "cgroup",
    "cgroup2",
    "securityfs",
    "pstore",
    "bpf",
    "tracefs",
    "debugfs",
    "configfs",
    "fusectl",
    "mqueue",
    "hugetlbfs",
    "autofs",
    "binfmt_misc",
    "efivarfs",
    "ramfs",
    "nsfs",
    "rpc_pipefs",
    "fuse.portal",
    "fuse.gvfsd-fuse",
    "squashfs",
    "overlay",
    "selinuxfs",
    "none",
];

fn is_remote(fstype: &str) -> bool {
    fstype.starts_with("fuse") || matches!(fstype, "nfs" | "nfs4" | "cifs" | "smb3" | "sshfs" | "afs" | "9p" | "ceph" | "glusterfs")
}

fn statvfs(path: &str) -> Option<(u64, u64, u64, u64, u64)> {
    let c = CString::new(path).ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
        return None;
    }
    let fr = s.f_frsize as u64;
    let total = s.f_blocks as u64 * fr;
    let free = s.f_bfree as u64 * fr;
    let avail = s.f_bavail as u64 * fr;
    let itotal = s.f_files as u64;
    let iused = itotal.saturating_sub(s.f_ffree as u64);
    Some((total, total.saturating_sub(free), avail, itotal, iused))
}

/// statvfs in a helper thread so a hung network mount cannot stall sampling.
fn statvfs_timeout(path: &str, d: Duration) -> Option<(u64, u64, u64, u64, u64)> {
    let (tx, rx) = std::sync::mpsc::channel();
    let p = path.to_string();
    std::thread::spawn(move || {
        let _ = tx.send(statvfs(&p));
    });
    rx.recv_timeout(d).ok().flatten()
}

impl DiskSampler {
    pub fn new() -> Self {
        let mut s = DiskSampler { prev: HashMap::new(), remote_cache: HashMap::new(), tick: 0 };
        s.prev = read_diskstats();
        s
    }

    pub fn sample(&mut self, dt: f64) -> DiskSnap {
        self.tick += 1;
        let mounts = self.mounts();
        let devices = self.devices(dt, &mounts);
        let total_read_bps = devices.iter().filter(|d| d.slaves.is_empty() && !d.name.starts_with("zram")).map(|d| d.read_bps).sum();
        let total_write_bps = devices.iter().filter(|d| d.slaves.is_empty() && !d.name.starts_with("zram")).map(|d| d.write_bps).sum();
        DiskSnap { mounts, devices, total_read_bps, total_write_bps, psi: read_psi("io") }
    }

    fn mounts(&mut self) -> Vec<Mount> {
        let s = read("/proc/self/mounts").unwrap_or_default();
        let mut out: Vec<Mount> = Vec::new();
        for line in s.lines() {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 4 {
                continue;
            }
            let (dev, mnt, fstype, opts) = (f[0], unescape(f[1]), f[2], f[3]);
            if PSEUDO_FS.contains(&fstype) {
                continue;
            }
            if fstype == "tmpfs" && mnt != "/tmp" {
                continue;
            }
            if mnt.starts_with("/run/") || mnt.starts_with("/sys/") || mnt.starts_with("/proc/") || mnt.starts_with("/dev/") {
                continue;
            }
            // Fold btrfs subvolumes / bind mounts of the same device into one row.
            if dev.starts_with("/dev/") {
                if let Some(existing) = out.iter_mut().find(|m| m.device == dev && m.fstype == fstype) {
                    existing.extra_mounts.push(mnt.clone());
                    continue;
                }
            }
            let remote = is_remote(fstype);
            let short_opts: Vec<&str> = opts
                .split(',')
                .filter(|o| {
                    matches!(*o, "ro" | "rw" | "noatime" | "discard")
                        || o.starts_with("compress")
                        || o.starts_with("subvol=")
                        || o.starts_with("discard")
                })
                .collect();
            let mut m = Mount {
                device: dev.to_string(),
                mount: mnt.clone(),
                fstype: fstype.to_string(),
                opts: short_opts.join(","),
                remote,
                ..Default::default()
            };
            let stats = if remote {
                if self.tick % 10 == 1 || !self.remote_cache.contains_key(&mnt) {
                    match statvfs_timeout(&mnt, Duration::from_millis(400)) {
                        Some(v) => {
                            self.remote_cache.insert(mnt.clone(), v);
                            Some(v)
                        }
                        None => {
                            m.stale = true;
                            self.remote_cache.get(&mnt).copied()
                        }
                    }
                } else {
                    self.remote_cache.get(&mnt).copied()
                }
            } else {
                statvfs(&mnt)
            };
            if let Some((t, u, a, it, iu)) = stats {
                m.total = t;
                m.used = u;
                m.avail = a;
                m.inodes_total = it;
                m.inodes_used = iu;
            }
            out.push(m);
        }
        // Real devices first, then remote/virtual; biggest first within groups.
        out.sort_by(|a, b| a.remote.cmp(&b.remote).then(b.total.cmp(&a.total)));
        out
    }

    fn devices(&mut self, dt: f64, mounts: &[Mount]) -> Vec<BlockDev> {
        let now = read_diskstats();
        let dt = dt.max(0.001);
        let mut mount_by_dev: HashMap<String, (String, String)> = HashMap::new();
        for m in mounts {
            mount_by_dev.insert(m.device.trim_start_matches("/dev/").to_string(), (m.mount.clone(), m.fstype.clone()));
        }
        // mapper devices appear as /dev/mapper/name; map dm-N -> name.
        let mut out = Vec::new();
        let Ok(rd) = std::fs::read_dir("/sys/block") else { return out };
        let mut names: Vec<String> = rd.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        for name in names {
            if name.starts_with("loop") || name.starts_with("ram") {
                continue;
            }
            let base = format!("/sys/block/{}", name);
            let size = read_u64(format!("{}/size", base)).unwrap_or(0) * 512;
            if size == 0 && !name.starts_with("zram") {
                continue;
            }
            let model = read(format!("{}/device/model", base)).filter(|s| !s.is_empty());
            let serial = read(format!("{}/device/serial", base)).filter(|s| !s.is_empty());
            let firmware =
                read(format!("{}/device/firmware_rev", base)).or_else(|| read(format!("{}/device/rev", base))).filter(|s| !s.is_empty());
            let rotational = read_u64(format!("{}/queue/rotational", base)).unwrap_or(0) == 1;
            let scheduler = read(format!("{}/queue/scheduler", base)).and_then(|a| {
                a.split_whitespace().find(|t| t.starts_with('[')).map(|t| t.trim_matches(|c| c == '[' || c == ']').to_string())
            });
            let discard = read_u64(format!("{}/queue/discard_granularity", base)).unwrap_or(0) > 0;
            let removable = read_u64(format!("{}/removable", base)).unwrap_or(0) == 1;
            let mapper_name = read(format!("{}/dm/name", base));
            let slaves: Vec<String> = std::fs::read_dir(format!("{}/slaves", base))
                .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
                .unwrap_or_default();
            let link = std::fs::read_link(&base).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
            let transport = if name.starts_with("nvme") {
                "NVMe".into()
            } else if name.starts_with("mmcblk") {
                "SD/eMMC".into()
            } else if name.starts_with("zram") {
                "RAM (zram)".into()
            } else if name.starts_with("dm-") {
                if read(format!("{}/dm/uuid", base)).map(|u| u.starts_with("CRYPT")).unwrap_or(false) {
                    "LUKS mapper".into()
                } else if read(format!("{}/dm/uuid", base)).map(|u| u.starts_with("LVM")).unwrap_or(false) {
                    "LVM".into()
                } else {
                    "device-mapper".into()
                }
            } else if name.starts_with("md") {
                "RAID (md)".into()
            } else if name.starts_with("vd") {
                "virtio".into()
            } else if link.contains("/usb") {
                "USB".into()
            } else if link.contains("/ata") {
                if rotational {
                    "SATA HDD".into()
                } else {
                    "SATA SSD".into()
                }
            } else if name.starts_with("sd") {
                "SCSI/SATA".into()
            } else {
                "block".into()
            };
            let raw = now.get(&name).copied().unwrap_or_default();
            let prev = self.prev.get(&name).copied().unwrap_or(raw);
            let read_bps = raw.rsect.saturating_sub(prev.rsect) as f64 * 512.0 / dt;
            let write_bps = raw.wsect.saturating_sub(prev.wsect) as f64 * 512.0 / dt;
            let read_iops = raw.reads.saturating_sub(prev.reads) as f64 / dt;
            let write_iops = raw.writes.saturating_sub(prev.writes) as f64 / dt;
            let util_pct = (raw.io_ms.saturating_sub(prev.io_ms) as f64 / (dt * 1000.0) * 100.0).min(100.0);
            // partitions
            let mut parts = Vec::new();
            if let Ok(rd) = std::fs::read_dir(&base) {
                let mut pn: Vec<String> = rd
                    .filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|p| p.starts_with(&name) && std::path::Path::new(&format!("{}/{}/partition", base, p)).exists())
                    .collect();
                pn.sort_by_key(|p| p.trim_start_matches(&name).trim_start_matches('p').parse::<u32>().unwrap_or(0));
                for p in pn {
                    let psize = read_u64(format!("{}/{}/size", base, p)).unwrap_or(0) * 512;
                    let (udev_type, label) = udev_fs(&base, &p);
                    let (mount, fstype) = match mount_by_dev.get(&p) {
                        Some((m, f)) => (Some(m.clone()), Some(f.clone())),
                        None => (None, udev_type),
                    };
                    parts.push(Partition { name: p, size: psize, mount, fstype, label });
                }
            }
            out.push(BlockDev {
                name: name.clone(),
                model,
                serial,
                firmware,
                size,
                rotational,
                transport,
                scheduler,
                read_bps,
                write_bps,
                read_iops,
                write_iops,
                util_pct,
                in_flight: raw.in_flight,
                total_read: raw.rsect * 512,
                total_written: raw.wsect * 512,
                parts,
                mapper_name,
                slaves,
                discard,
                removable,
            });
        }
        self.prev = now;
        // Physical disks first, then mappers, then zram.
        out.sort_by_key(|d| (d.name.starts_with("zram"), !d.slaves.is_empty(), std::cmp::Reverse(d.size)));
        out
    }
}

fn read_diskstats() -> HashMap<String, Raw> {
    let s = read("/proc/diskstats").unwrap_or_default();
    let mut m = HashMap::new();
    for line in s.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 14 {
            continue;
        }
        let g = |i: usize| f.get(i).and_then(|v| v.parse().ok()).unwrap_or(0u64);
        m.insert(f[2].to_string(), Raw { reads: g(3), rsect: g(5), writes: g(7), wsect: g(9), in_flight: g(11), io_ms: g(12) });
    }
    m
}

fn unescape(s: &str) -> String {
    // /proc/mounts escapes space as \040 etc.
    let mut out = String::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 3 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 4], 8) {
                out.push(v as char);
                i += 4;
                continue;
            }
        }
        out.push(b[i] as char);
        i += 1;
    }
    out
}
