use serde::{Deserialize, Serialize};
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::ptr::null_mut;

use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};

use crate::lang::{t, K, Lang};
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
    pub graphs: GraphMap,
    pub graph_width: i32,
    pub compact: bool,
    pub language: Lang,
    pub colors: ColorCfg,
    pub bar_transparent: bool,
    pub bar_bg: String,
    pub bar_opacity: u8,
    pub views: Views,
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
            graph: Graph::Bar,
            graphs: GraphMap::default(),
            graph_width: 48,
            compact: false,
            language: Lang::default(),
            colors: ColorCfg::default(),
            bar_transparent: false,
            bar_bg: "#202020".into(),
            bar_opacity: 80,
            views: Views::default(),
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

    pub fn palette(&self) -> Palette {
        let mut p = palette(self.theme);
        self.colors.apply(&mut p);
        p
    }

    pub fn graph_for(&self, m: Metric) -> Graph {
        let g = match m {
            Metric::Cpu => self.graphs.cpu,
            Metric::Ram => self.graphs.ram,
            Metric::Net => self.graphs.net,
            Metric::Disk => self.graphs.disk,
            Metric::Capacity => self.graphs.capacity,
        };
        g.unwrap_or(self.graph)
    }

    pub fn set_graph_for(&mut self, m: Metric, g: Graph) {
        match m {
            Metric::Cpu => self.graphs.cpu = Some(g),
            Metric::Ram => self.graphs.ram = Some(g),
            Metric::Net => self.graphs.net = Some(g),
            Metric::Disk => self.graphs.disk = Some(g),
            Metric::Capacity => self.graphs.capacity = Some(g),
        }
    }

    pub fn set_graph_all(&mut self, g: Graph) {
        self.graph = g;
        self.graphs = GraphMap {
            cpu: Some(g),
            ram: Some(g),
            net: Some(g),
            disk: Some(g),
            capacity: Some(g),
        };
    }

    pub fn view(&self, m: Metric) -> ViewMode {
        match m {
            Metric::Cpu => self.views.cpu,
            Metric::Ram => self.views.ram,
            Metric::Net => self.views.net,
            Metric::Disk => self.views.disk,
            Metric::Capacity => self.views.capacity,
        }
    }

    pub fn view_mut(&mut self, m: Metric) -> &mut ViewMode {
        match m {
            Metric::Cpu => &mut self.views.cpu,
            Metric::Ram => &mut self.views.ram,
            Metric::Net => &mut self.views.net,
            Metric::Disk => &mut self.views.disk,
            Metric::Capacity => &mut self.views.capacity,
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

    pub fn label_l(self, lang: Lang) -> &'static str {
        t(
            lang,
            match self {
                Graph::None => K::GraphNone,
                Graph::Bar => K::GraphBar,
                Graph::Line => K::GraphLine,
                Graph::Pie => K::GraphPie,
            },
        )
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

    pub fn label_l(self, lang: Lang) -> &'static str {
        t(
            lang,
            match self {
                Theme::Auto => K::ThemeAuto,
                Theme::Dark => K::ThemeDark,
                Theme::Light => K::ThemeLight,
            },
        )
    }

    pub fn index(self) -> usize {
        Theme::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }

    pub fn from_index(i: usize) -> Theme {
        Theme::ALL.get(i).copied().unwrap_or(Theme::Auto)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct ColorCfg {
    pub bg: String,
    pub label: String,
    pub accent: String,
    pub warn: String,
    pub cpu: String,
    pub ram: String,
    pub net: String,
    pub net_up: String,
    pub disk: String,
    pub cap: String,
}

impl ColorCfg {
    pub fn apply(&self, p: &mut Palette) {
        let set = |src: &str, dst: &mut [u8; 3]| {
            if let Some(c) = parse_color(src) {
                *dst = c;
            }
        };
        set(&self.bg, &mut p.bg);
        set(&self.label, &mut p.label);
        set(&self.accent, &mut p.accent);
        set(&self.warn, &mut p.warn);
        set(&self.cpu, &mut p.cpu);
        set(&self.ram, &mut p.ram);
        set(&self.net, &mut p.net);
        set(&self.net_up, &mut p.net_up);
        set(&self.disk, &mut p.disk);
        set(&self.cap, &mut p.cap);
    }

    pub fn set(&mut self, key: &str, value: String) {
        match key {
            "bg" => self.bg = value,
            "label" => self.label = value,
            "accent" => self.accent = value,
            "warn" => self.warn = value,
            "cpu" => self.cpu = value,
            "ram" => self.ram = value,
            "net" => self.net = value,
            "net_up" => self.net_up = value,
            "disk" => self.disk = value,
            "cap" => self.cap = value,
            _ => {}
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct GraphMap {
    pub cpu: Option<Graph>,
    pub ram: Option<Graph>,
    pub net: Option<Graph>,
    pub disk: Option<Graph>,
    pub capacity: Option<Graph>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ViewMode {
    Text,
    Graph,
    Both,
}

impl ViewMode {
    pub const ALL: [ViewMode; 3] = [ViewMode::Text, ViewMode::Graph, ViewMode::Both];

    pub fn label_l(self, lang: Lang) -> &'static str {
        t(
            lang,
            match self {
                ViewMode::Text => K::ViewText,
                ViewMode::Graph => K::ViewGraph,
                ViewMode::Both => K::ViewBoth,
            },
        )
    }

    pub fn index(self) -> usize {
        match self {
            ViewMode::Text => 0,
            ViewMode::Graph => 1,
            ViewMode::Both => 2,
        }
    }

    pub fn from_index(i: usize) -> ViewMode {
        ViewMode::ALL.get(i).copied().unwrap_or(ViewMode::Text)
    }

    pub fn has_text(self) -> bool {
        matches!(self, ViewMode::Text | ViewMode::Both)
    }

    pub fn has_graph(self) -> bool {
        matches!(self, ViewMode::Graph | ViewMode::Both)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Views {
    pub cpu: ViewMode,
    pub ram: ViewMode,
    pub net: ViewMode,
    pub disk: ViewMode,
    pub capacity: ViewMode,
}

impl Default for Views {
    fn default() -> Self {
        Views {
            cpu: ViewMode::Both,
            ram: ViewMode::Text,
            net: ViewMode::Text,
            disk: ViewMode::Text,
            capacity: ViewMode::Text,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct FloatCfg {
    pub on: bool,
    pub x: i32,
    pub y: i32,
    pub transparent: bool,
    pub topmost: bool,
    pub bg: String,
    pub opacity: u8,
    pub border: bool,
    pub border_color: String,
}

impl Default for FloatCfg {
    fn default() -> Self {
        FloatCfg {
            on: false,
            x: 40,
            y: 60,
            transparent: true,
            topmost: true,
            bg: "#1c1c1e".into(),
            opacity: 100,
            border: false,
            border_color: "#4b4b52".into(),
        }
    }
}

pub fn parse_color(s: &str) -> Option<[u8; 3]> {
    let t = s.trim().trim_start_matches('#');
    if t.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&t[0..2], 16).ok()?;
    let g = u8::from_str_radix(&t[2..4], 16).ok()?;
    let b = u8::from_str_radix(&t[4..6], 16).ok()?;
    Some([r, g, b])
}

pub fn hex_color(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Floats {
    pub cpu: FloatCfg,
    pub ram: FloatCfg,
    pub net: FloatCfg,
    pub disk: FloatCfg,
    pub capacity: FloatCfg,
}

impl Default for Floats {
    fn default() -> Self {
        let at = |y| FloatCfg {
            y,
            ..FloatCfg::default()
        };
        Floats {
            cpu: at(60),
            ram: at(104),
            net: at(148),
            disk: at(192),
            capacity: at(236),
        }
    }
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: [u8; 3],
    pub surface: [u8; 3],
    pub label: [u8; 3],
    pub text: [u8; 3],
    pub cpu: [u8; 3],
    pub ram: [u8; 3],
    pub net: [u8; 3],
    pub net_up: [u8; 3],
    pub disk: [u8; 3],
    pub cap: [u8; 3],
    pub warn: [u8; 3],
    pub divider: [u8; 3],
    pub accent: [u8; 3],
}

const DARK: Palette = Palette {
    bg: [32, 32, 32],
    surface: [45, 45, 45],
    label: [157, 157, 157],
    text: [255, 255, 255],
    cpu: [76, 194, 255],
    ram: [108, 203, 95],
    net: [255, 185, 72],
    net_up: [180, 140, 255],
    disk: [255, 138, 101],
    cap: [197, 217, 122],
    warn: [255, 95, 87],
    divider: [58, 58, 58],
    accent: [0, 120, 212],
};

const LIGHT: Palette = Palette {
    bg: [243, 243, 243],
    surface: [255, 255, 255],
    label: [93, 93, 93],
    text: [26, 26, 26],
    cpu: [0, 103, 192],
    ram: [16, 124, 16],
    net: [157, 93, 0],
    net_up: [107, 79, 187],
    disk: [196, 62, 28],
    cap: [92, 122, 26],
    warn: [196, 43, 28],
    divider: [214, 214, 214],
    accent: [0, 103, 192],
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

unsafe fn system_accent(fallback: [u8; 3]) -> [u8; 3] {
    let sub = wide("Software\\Microsoft\\Windows\\DWM");
    let name = wide("AccentColor");
    let mut v: u32 = 0;
    let mut cb = std::mem::size_of::<u32>() as u32;
    let rc = RegGetValueW(
        HKEY_CURRENT_USER,
        sub.as_ptr(),
        name.as_ptr(),
        RRF_RT_REG_DWORD,
        null_mut(),
        &mut v as *mut u32 as *mut c_void,
        &mut cb,
    );
    if rc == 0 && (v & 0x00FF_FFFF) != 0 {
        [
            (v & 0xFF) as u8,
            ((v >> 8) & 0xFF) as u8,
            ((v >> 16) & 0xFF) as u8,
        ]
    } else {
        fallback
    }
}

pub fn palette(theme: Theme) -> Palette {
    let mut p = match theme {
        Theme::Dark => DARK,
        Theme::Light => LIGHT,
        Theme::Auto => {
            if unsafe { system_uses_light_theme() } {
                LIGHT
            } else {
                DARK
            }
        }
    };
    p.accent = unsafe { system_accent(p.accent) };
    p
}

pub fn is_dark(p: &Palette) -> bool {
    let l = p.bg[0] as u32 + p.bg[1] as u32 + p.bg[2] as u32;
    l < 380
}

pub fn load() -> (Config, PathBuf) {
    let dir = std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("statmon");
    let path = dir.join("config.json");
    let cfg = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<Config>(&s).ok())
        .unwrap_or_default();
    if !path.exists() {
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
