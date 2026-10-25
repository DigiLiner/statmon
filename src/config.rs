use serde::{Deserialize, Serialize};
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::ptr::null_mut;

use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};

use crate::metrics::Metric;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default, rename_all = "snake_case")]
pub struct Config {
    pub refresh_ms: u32,
    pub capacity_secs: u32,
    pub font_label_px: i32,
    pub font_value_px: i32,
    pub float_font_px: i32,
    pub pad_x: i32,
    pub divider_gap: i32,
    pub inner_gap: i32,
    pub theme: Theme,
    pub drives: Vec<String>,
    pub show_cpu: bool,
    pub show_ram: bool,
    pub show_net: bool,
    pub show_disk: bool,
    pub show_capacity: bool,
    pub graph: Graph,
    pub graph_width: i32,
    pub float_transparent: bool,
    pub float_topmost: bool,
    pub floats: Floats,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            refresh_ms: 1000,
            capacity_secs: 10,
            font_label_px: 12,
            font_value_px: 13,
            float_font_px: 14,
            pad_x: 6,
            divider_gap: 9,
            inner_gap: 4,
            theme: Theme::Auto,
            drives: Vec::new(),
            show_cpu: true,
            show_ram: true,
            show_net: true,
            show_disk: true,
            show_capacity: true,
            graph: Graph::None,
            graph_width: 96,
            float_transparent: true,
            float_topmost: true,
            floats: Floats::default(),
        }
    }
}

impl Config {
    pub fn shows(&self, m: Metric) -> bool {
        match m {
            Metric::Cpu => self.show_cpu,
            Metric::Ram => self.show_ram,
            Metric::Net => self.show_net,
            Metric::Disk => self.show_disk,
            Metric::Capacity => self.show_capacity,
        }
    }

    pub fn set_show(&mut self, m: Metric, on: bool) {
        match m {
            Metric::Cpu => self.show_cpu = on,
            Metric::Ram => self.show_ram = on,
            Metric::Net => self.show_net = on,
            Metric::Disk => self.show_disk = on,
            Metric::Capacity => self.show_capacity = on,
        }
    }

    pub fn float(&self, m: Metric) -> &FloatCfg {
        match m {
            Metric::Cpu => &self.floats.cpu,
            Metric::Ram => &self.floats.ram,
            Metric::Net => &self.floats.net,
            Metric::Disk => &self.floats.disk,
            Metric::Capacity => &self.floats.capacity,
        }
    }

    pub fn float_mut(&mut self, m: Metric) -> &mut FloatCfg {
        match m {
            Metric::Cpu => &mut self.floats.cpu,
            Metric::Ram => &mut self.floats.ram,
            Metric::Net => &mut self.floats.net,
            Metric::Disk => &mut self.floats.disk,
            Metric::Capacity => &mut self.floats.capacity,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Auto,
    Dark,
    Light,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Graph {
    None,
    Bar,
    Line,
    Pie,
}

impl Graph {
    pub const ALL: [Graph; 4] = [Graph::None, Graph::Bar, Graph::Line, Graph::Pie];

    pub fn label(self) -> &'static str {
        match self {
            Graph::None => "Yok",
            Graph::Bar => "Bar",
            Graph::Line => "Çizgi",
            Graph::Pie => "Pasta",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Graph::None => 0,
            Graph::Bar => 1,
            Graph::Line => 2,
            Graph::Pie => 3,
        }
    }

    pub fn from_index(i: usize) -> Graph {
        Graph::ALL.get(i).copied().unwrap_or(Graph::None)
    }
}

impl Theme {
    pub const ALL: [Theme; 3] = [Theme::Auto, Theme::Dark, Theme::Light];

    pub fn label(self) -> &'static str {
        match self {
            Theme::Auto => "Otomatik",
            Theme::Dark => "Koyu",
            Theme::Light => "Açık",
        }
    }

    pub fn index(self) -> usize {
        Theme::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }

    pub fn from_index(i: usize) -> Theme {
        Theme::ALL.get(i).copied().unwrap_or(Theme::Auto)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct FloatCfg {
    pub on: bool,
    pub x: i32,
    pub y: i32,
}

impl Default for FloatCfg {
    fn default() -> Self {
        FloatCfg {
            on: false,
            x: 40,
            y: 60,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Floats {
    pub cpu: FloatCfg,
    pub ram: FloatCfg,
    pub net: FloatCfg,
    pub disk: FloatCfg,
    pub capacity: FloatCfg,
}

impl Floats {
    pub fn prepare(&mut self) {
        self.cpu.y = 60;
        self.ram.y = 104;
        self.net.y = 148;
        self.disk.y = 192;
        self.capacity.y = 236;
    }
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: [u8; 3],
    pub label: [u8; 3],
    pub cpu: [u8; 3],
    pub ram: [u8; 3],
    pub net: [u8; 3],
    pub net_up: [u8; 3],
    pub disk: [u8; 3],
    pub cap: [u8; 3],
    pub warn: [u8; 3],
    pub divider: [u8; 3],
}

const DARK: Palette = Palette {
    bg: [28, 28, 30],
    label: [150, 152, 158],
    cpu: [86, 182, 255],
    ram: [126, 207, 138],
    net: [240, 178, 90],
    net_up: [190, 140, 255],
    disk: [255, 138, 101],
    cap: [196, 212, 120],
    warn: [255, 94, 87],
    divider: [72, 72, 78],
};

const LIGHT: Palette = Palette {
    bg: [243, 243, 243],
    label: [96, 98, 104],
    cpu: [0, 112, 190],
    ram: [36, 130, 62],
    net: [166, 100, 8],
    net_up: [110, 60, 170],
    disk: [196, 78, 48],
    cap: [86, 114, 18],
    warn: [190, 30, 30],
    divider: [204, 204, 209],
};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn system_uses_light_theme() -> bool {
    let sub = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
    let name = wide("SystemUsesLightTheme");
    let mut data: u32 = 0;
    let mut cb = std::mem::size_of::<u32>() as u32;
    let rc = RegGetValueW(
        HKEY_CURRENT_USER,
        sub.as_ptr(),
        name.as_ptr(),
        RRF_RT_REG_DWORD,
        null_mut(),
        &mut data as *mut u32 as *mut c_void,
        &mut cb,
    );
    rc == 0 && data == 1
}

pub fn palette(theme: Theme) -> Palette {
    match theme {
        Theme::Dark => DARK,
        Theme::Light => LIGHT,
        Theme::Auto => {
            if unsafe { system_uses_light_theme() } {
                LIGHT
            } else {
                DARK
            }
        }
    }
}

pub fn load() -> (Config, PathBuf) {
    let dir = std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("statmon");
    let path = dir.join("config.json");
    let mut cfg = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<Config>(&s).ok())
        .unwrap_or_default();
    if !path.exists() {
        cfg.floats.prepare();
        let _ = std::fs::create_dir_all(&dir);
        save(&cfg, &path);
    }
    (cfg, path)
}

pub fn save(cfg: &Config, path: &Path) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(s) = serde_json::to_string_pretty(cfg) {
        let _ = std::fs::write(path, s);
    }
}
