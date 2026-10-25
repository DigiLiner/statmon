use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::time::Instant;

use windows_sys::Win32::Foundation::FILETIME;
use windows_sys::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};
use windows_sys::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives,
};
use windows_sys::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterValue,
    PdhOpenQueryW, PDH_FMT_COUNTERVALUE, PDH_FMT_DOUBLE,
};
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows_sys::Win32::System::Threading::GetSystemTimes;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn ft(v: FILETIME) -> u64 {
    ((v.dwHighDateTime as u64) << 32) | v.dwLowDateTime as u64
}

pub struct Metrics {
    prev_idle: u64,
    prev_kernel: u64,
    prev_user: u64,
    prev_rx: u64,
    prev_tx: u64,
    prev_tick: Instant,
    query: isize,
    c_busy: isize,
    c_read: isize,
    c_write: isize,
    pdh_ok: bool,
    pub cpu: f32,
    pub mem_pct: u32,
    pub mem_used: u64,
    pub mem_total: u64,
    pub rx: f64,
    pub tx: f64,
    pub disk_busy: f32,
    pub read_bps: f64,
    pub write_bps: f64,
}

impl Metrics {
    pub fn new() -> Metrics {
        let mut m = Metrics {
            prev_idle: 0,
            prev_kernel: 0,
            prev_user: 0,
            prev_rx: 0,
            prev_tx: 0,
            prev_tick: Instant::now(),
            query: 0,
            c_busy: 0,
            c_read: 0,
            c_write: 0,
            pdh_ok: false,
            cpu: 0.0,
            mem_pct: 0,
            mem_used: 0,
            mem_total: 0,
            rx: 0.0,
            tx: 0.0,
            disk_busy: 0.0,
            read_bps: 0.0,
            write_bps: 0.0,
        };
        unsafe {
            m.init_cpu();
            m.init_net();
            m.init_pdh();
        }
        m
    }

    unsafe fn init_cpu(&mut self) {
        let mut idle: FILETIME = zeroed();
        let mut kernel: FILETIME = zeroed();
        let mut user: FILETIME = zeroed();
        if GetSystemTimes(&mut idle, &mut kernel, &mut user) != 0 {
            self.prev_idle = ft(idle);
            self.prev_kernel = ft(kernel);
            self.prev_user = ft(user);
        }
    }

    unsafe fn init_net(&mut self) {
        let (rx, tx) = net_total();
        self.prev_rx = rx;
        self.prev_tx = tx;
    }

    unsafe fn init_pdh(&mut self) {
        let mut q: isize = 0;
        if PdhOpenQueryW(null(), 0, &mut q) != 0 {
            return;
        }
        self.query = q;
        let paths = [
            "\\PhysicalDisk(_Total)\\% Disk Time",
            "\\PhysicalDisk(_Total)\\Disk Read Bytes/sec",
            "\\PhysicalDisk(_Total)\\Disk Write Bytes/sec",
        ];
        let mut counters: [isize; 3] = [0; 3];
        for (i, p) in paths.iter().enumerate() {
            let wp = wide(p);
            let mut c: isize = 0;
            if PdhAddEnglishCounterW(q, wp.as_ptr(), 0, &mut c) == 0 {
                counters[i] = c;
            }
        }
        self.c_busy = counters[0];
        self.c_read = counters[1];
        self.c_write = counters[2];
        self.pdh_ok = counters[0] != 0;
        PdhCollectQueryData(q);
    }

    pub fn tick(&mut self) {
        unsafe {
            self.tick_cpu();
            self.tick_mem();
            self.tick_net();
            self.tick_disk();
        }
    }

    unsafe fn tick_cpu(&mut self) {
        let mut idle: FILETIME = zeroed();
        let mut kernel: FILETIME = zeroed();
        let mut user: FILETIME = zeroed();
        if GetSystemTimes(&mut idle, &mut kernel, &mut user) == 0 {
            return;
        }
        let (i, k, u) = (ft(idle), ft(kernel), ft(user));
        let di = i.saturating_sub(self.prev_idle);
        let dk = k.saturating_sub(self.prev_kernel);
        let du = u.saturating_sub(self.prev_user);
        self.prev_idle = i;
        self.prev_kernel = k;
        self.prev_user = u;
        let total = dk + du;
        if total > 0 {
            self.cpu = ((total.saturating_sub(di)) as f64 * 100.0 / total as f64) as f32;
        }
    }

    unsafe fn tick_mem(&mut self) {
        let mut st: MEMORYSTATUSEX = zeroed();
        st.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
        if GlobalMemoryStatusEx(&mut st) != 0 {
            self.mem_pct = st.dwMemoryLoad;
            self.mem_total = st.ullTotalPhys;
            self.mem_used = st.ullTotalPhys.saturating_sub(st.ullAvailPhys);
        }
    }

    unsafe fn tick_net(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.prev_tick).as_secs_f64();
        if dt < 0.2 {
            return;
        }
        self.prev_tick = now;
        let (rx, tx) = net_total();
        let drx = rx.saturating_sub(self.prev_rx);
        let dtx = tx.saturating_sub(self.prev_tx);
        self.prev_rx = rx;
        self.prev_tx = tx;
        self.rx = drx as f64 / dt;
        self.tx = dtx as f64 / dt;
    }

    unsafe fn tick_disk(&mut self) {
        if !self.pdh_ok {
            return;
        }
        PdhCollectQueryData(self.query);
        self.disk_busy = pdh_double(self.c_busy).unwrap_or(0.0).clamp(0.0, 100.0) as f32;
        self.read_bps = pdh_double(self.c_read).unwrap_or(0.0).max(0.0);
        self.write_bps = pdh_double(self.c_write).unwrap_or(0.0).max(0.0);
    }
}

impl Drop for Metrics {
    fn drop(&mut self) {
        unsafe {
            if self.query != 0 {
                PdhCloseQuery(self.query);
            }
        }
    }
}

unsafe fn pdh_double(c: isize) -> Option<f64> {
    if c == 0 {
        return None;
    }
    let mut v: PDH_FMT_COUNTERVALUE = zeroed();
    if PdhGetFormattedCounterValue(c, PDH_FMT_DOUBLE, null_mut(), &mut v) == 0 && v.CStatus == 0 {
        Some(v.Anonymous.doubleValue)
    } else {
        None
    }
}

unsafe fn net_total() -> (u64, u64) {
    let mut table: *mut MIB_IF_TABLE2 = null_mut();
    if GetIfTable2(&mut table) != 0 || table.is_null() {
        return (0, 0);
    }
    let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize);
    let mut rx: u64 = 0;
    let mut tx: u64 = 0;
    for r in rows {
        if r.Type == 24 || r.OperStatus != 1 {
            continue;
        }
        rx = rx.saturating_add(r.InOctets);
        tx = tx.saturating_add(r.OutOctets);
    }
    FreeMibTable(table as *const c_void);
    (rx, tx)
}

pub struct DriveInfo {
    pub letter: char,
    pub total: u64,
    pub free: u64,
}

impl DriveInfo {
    pub fn used_pct(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            (self.total.saturating_sub(self.free)) as f64 as f32 * 100.0 / self.total as f64 as f32
        }
    }
}

pub fn capacity(only: &[String]) -> Vec<DriveInfo> {
    let mut out = Vec::new();
    unsafe {
        let mask = GetLogicalDrives();
        for i in 0..26u32 {
            if mask & (1 << i) == 0 {
                continue;
            }
            let letter = (b'A' + i as u8) as char;
            let listed = !only.is_empty();
            let selected = only
                .iter()
                .any(|d| d.eq_ignore_ascii_case(&letter.to_string()));
            if listed && !selected {
                continue;
            }
            let root = wide(&format!("{}:\\", letter));
            if !listed && GetDriveTypeW(root.as_ptr()) != 3 {
                continue;
            }
            let mut free_avail: u64 = 0;
            let mut total: u64 = 0;
            let mut total_free: u64 = 0;
            if GetDiskFreeSpaceExW(
                root.as_ptr(),
                &mut free_avail,
                &mut total,
                &mut total_free,
            ) == 0
                || total == 0
            {
                continue;
            }
            out.push(DriveInfo {
                letter,
                total,
                free: total_free,
            });
        }
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Metric {
    Cpu,
    Ram,
    Net,
    Disk,
    Capacity,
}

impl Metric {
    pub const ALL: [Metric; 5] = [
        Metric::Cpu,
        Metric::Ram,
        Metric::Net,
        Metric::Disk,
        Metric::Capacity,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Metric::Cpu => "CPU",
            Metric::Ram => "RAM",
            Metric::Net => "NET",
            Metric::Disk => "DISK",
            Metric::Capacity => "DOLU",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            Metric::Cpu => "C",
            Metric::Ram => "R",
            Metric::Net => "N",
            Metric::Disk => "IO",
            Metric::Capacity => "V",
        }
    }


}
