use std::mem::zeroed;

use windows_sys::Win32::Foundation::{HWND, POINT, RECT, SIZE};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreatePen, CreateSolidBrush, DeleteObject, Ellipse, EndPaint, FillRect,
    GetDC, GetStockObject, GetTextExtentPoint32W, Pie, Polyline, ReleaseDC, SelectObject, SetBkMode,
    SetTextColor, TextOutW, ANTIALIASED_QUALITY, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS,
    DEFAULT_CHARSET, DEFAULT_PITCH, FF_DONTCARE, FW_NORMAL, FW_SEMIBOLD, HDC, HFONT, NULL_PEN,
    OUT_DEFAULT_PRECIS, PAINTSTRUCT, PS_SOLID, TRANSPARENT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect;

use crate::config::{Config, Graph, Palette};
use crate::history::History;
use crate::metrics::{DriveInfo, Metric, Metrics};

pub const KEY: [u8; 3] = [1, 1, 1];
const CHART_GAP: i32 = 5;
const PART_GAP: i32 = 3;
const COMPACT_GAP: i32 = 8;
const HOVER_PAD: i32 = 3;

pub struct Fonts {
    label: HFONT,
    value: HFONT,
    float_label: HFONT,
    float_value: HFONT,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

impl Fonts {
    pub unsafe fn new(dpi: u32, cfg: &Config) -> Fonts {
        let make = |px: i32, face: &str, weight: u32, quality: u32| -> HFONT {
            let h = -((px * dpi as i32 + 48) / 96);
            let wface = wide(face);
            CreateFontW(
                h,
                0,
                0,
                0,
                weight as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET as u32,
                OUT_DEFAULT_PRECIS as u32,
                CLIP_DEFAULT_PRECIS as u32,
                quality,
                (DEFAULT_PITCH | FF_DONTCARE) as u32,
                wface.as_ptr(),
            )
        };
        Fonts {
            label: make(
                cfg.font_label_px,
                "Segoe UI",
                FW_NORMAL,
                CLEARTYPE_QUALITY as u32,
            ),
            value: make(
                cfg.font_value_px,
                "Consolas",
                FW_SEMIBOLD,
                CLEARTYPE_QUALITY as u32,
            ),
            float_label: make(
                cfg.float_font_px - 1,
                "Segoe UI",
                FW_NORMAL,
                ANTIALIASED_QUALITY as u32,
            ),
            float_value: make(
                cfg.float_font_px,
                "Consolas",
                FW_SEMIBOLD,
                ANTIALIASED_QUALITY as u32,
            ),
        }
    }

    pub unsafe fn destroy(&self) {
        DeleteObject(self.label);
        DeleteObject(self.value);
        DeleteObject(self.float_label);
        DeleteObject(self.float_value);
    }

    fn label(&self, big: bool) -> HFONT {
        if big {
            self.float_label
        } else {
            self.label
        }
    }

    fn value(&self, big: bool) -> HFONT {
        if big {
            self.float_value
        } else {
            self.value
        }
    }
}

pub fn rgb(c: [u8; 3]) -> u32 {
    (c[0] as u32) | ((c[1] as u32) << 8) | ((c[2] as u32) << 16)
}

fn blend(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    [f(a[0], b[0]), f(a[1], b[1]), f(a[2], b[2])]
}

#[derive(Clone, Copy)]
enum ChartKind {
    Bar,
    Line,
    Pie,
}

pub struct Chart {
    kind: ChartKind,
    lines: Vec<(Vec<f32>, [u8; 3])>,
    pie: Vec<(f32, [u8; 3])>,
    max: f32,
}

pub struct TextPart {
    pub text: String,
    pub color: [u8; 3],
}

pub struct Tile {
    pub metric: Metric,
    pub label: String,
    pub chart: Option<Chart>,
    pub parts: Vec<TextPart>,
}

pub struct Look {
    pub transparent: bool,
    pub bg: [u8; 3],
    pub edge: Option<[u8; 3]>,
}

pub struct Frame<'a> {
    pub metrics: &'a Metrics,
    pub drives: &'a [DriveInfo],
    pub history: &'a History,
    pub cfg: &'a Config,
    pub palette: &'a Palette,
    pub metric: Option<Metric>,
    pub dpi: u32,
    pub hover: Option<Metric>,
}

impl<'a> Frame<'a> {
    fn big(&self) -> bool {
        self.metric.is_some()
    }

    fn compact(&self) -> bool {
        self.cfg.compact && !self.big()
    }

    fn chart_px(&self) -> i32 {
        (self.cfg.graph_width * self.dpi.max(96) as i32 / 96).max(12)
    }

    pub fn tiles(&self) -> Vec<Tile> {
        let list: Vec<Metric> = match self.metric {
            Some(m) => vec![m],
            None => Metric::ALL
                .into_iter()
                .filter(|m| self.cfg.shows(*m))
                .collect(),
        };
        let mut tiles = Vec::with_capacity(list.len());
        for m in list {
            let mode = self.cfg.view(m);
            let kind = self.cfg.graph_for(m);
            let can_graph = mode.has_graph() && kind != Graph::None;
            let show_text = mode.has_text() || !can_graph;
            let chart = if can_graph { self.chart_for(m, kind) } else { None };
            let has_chart = chart.is_some();
            let label = if m == Metric::Capacity {
                if has_chart {
                    if self.compact() {
                        Metric::Capacity.short().to_string()
                    } else {
                        Metric::Capacity.label().to_string()
                    }
                } else {
                    String::new()
                }
            } else if self.compact() {
                m.short().to_string()
            } else {
                m.label().to_string()
            };
            let parts = if show_text { self.parts_for(m) } else { Vec::new() };
            tiles.push(Tile {
                metric: m,
                label,
                chart,
                parts,
            });
        }
        tiles
    }

    fn chart_for(&self, m: Metric, kind: Graph) -> Option<Chart> {
        if kind == Graph::None {
            return None;
        }
        let p = self.palette;
        let h = self.history;
        let data = h.series(m);
        match kind {
            Graph::None => None,
            Graph::Bar => Some(Chart {
                kind: ChartKind::Bar,
                lines: vec![(data.to_vec(), metric_color(m, p))],
                pie: Vec::new(),
                max: chart_max(m, data),
            }),
            Graph::Line => Some(Chart {
                kind: ChartKind::Line,
                lines: vec![(data.to_vec(), metric_color(m, p))],
                pie: Vec::new(),
                max: chart_max(m, data),
            }),
            Graph::Pie => {
                if m == Metric::Net {
                    Some(Chart {
                        kind: ChartKind::Pie,
                        lines: Vec::new(),
                        pie: vec![
                            (self.metrics.rx as f32, p.net),
                            (self.metrics.tx as f32, p.net_up),
                        ],
                        max: 100.0,
                    })
                } else {
                    let v = data.last().copied().unwrap_or(0.0).clamp(0.0, 100.0);
                    Some(Chart {
                        kind: ChartKind::Pie,
                        lines: Vec::new(),
                        pie: vec![(v, metric_color(m, p)), (100.0 - v, p.divider)],
                        max: 100.0,
                    })
                }
            }
        }
    }

    fn parts_for(&self, m: Metric) -> Vec<TextPart> {
        let p = self.palette;
        let met = self.metrics;
        let compact = self.compact();
        match m {
            Metric::Cpu => vec![TextPart {
                text: format!("{:>3}%", met.cpu.round() as i64),
                color: p.cpu,
            }],
            Metric::Ram => vec![TextPart {
                text: format!("{:>3}%", met.mem_pct),
                color: p.ram,
            }],
            Metric::Net => {
                if compact {
                    vec![
                        TextPart {
                            text: format!("{:>5}", rate(met.rx)),
                            color: p.net,
                        },
                        TextPart {
                            text: "/".into(),
                            color: p.label,
                        },
                        TextPart {
                            text: format!("{:>5}", rate(met.tx)),
                            color: p.net_up,
                        },
                    ]
                } else {
                    vec![
                        TextPart {
                            text: format!("\u{2193}{:>5}", rate(met.rx)),
                            color: p.net,
                        },
                        TextPart {
                            text: format!("\u{2191}{:>5}", rate(met.tx)),
                            color: p.net_up,
                        },
                    ]
                }
            }
            Metric::Disk => vec![TextPart {
                text: format!(
                    "{:>3}% {:>5}",
                    met.disk_busy.round() as i64,
                    rate(met.read_bps + met.write_bps)
                ),
                color: p.disk,
            }],
            Metric::Capacity => {
                if self.drives.is_empty() {
                    return Vec::new();
                }
                let warn = self.drives.iter().any(|d| d.used_pct() >= 90.0);
                vec![TextPart {
                    text: capacity_text(self.drives, compact),
                    color: if warn { p.warn } else { p.cap },
                }]
            }
        }
    }
}

fn chart_max(m: Metric, data: &[f32]) -> f32 {
    match m {
        Metric::Net => {
            let top = data.iter().fold(0.0f32, |a, b| a.max(*b));
            (top * 1.15).max(1024.0)
        }
        Metric::Capacity => 100.0,
        _ => {
            let top = data.iter().fold(0.0f32, |a, b| a.max(*b));
            (top * 1.15).clamp(5.0, 100.0)
        }
    }
}

fn metric_color(m: Metric, p: &Palette) -> [u8; 3] {
    match m {
        Metric::Cpu => p.cpu,
        Metric::Ram => p.ram,
        Metric::Net => p.net,
        Metric::Disk => p.disk,
        Metric::Capacity => p.cap,
    }
}

fn capacity_text(drives: &[DriveInfo], compact: bool) -> String {
    let list: &[DriveInfo] = if compact {
        match drives
            .iter()
            .max_by(|a, b| a.used_pct().partial_cmp(&b.used_pct()).unwrap())
        {
            Some(d) => std::slice::from_ref(d),
            None => &[],
        }
    } else {
        &drives[..drives.len().min(3)]
    };
    let mut parts = Vec::new();
    for d in list {
        if compact {
            parts.push(format!("{}:{:>3}%", d.letter, d.used_pct().round() as i64));
        } else {
            parts.push(format!("{}:{:>3}%", d.letter, d.used_pct().round() as i64));
        }
    }
    parts.join("  ")
}

pub fn rate(v: f64) -> String {
    let units = ["B", "K", "M", "G"];
    let mut x = v.max(0.0);
    if x < 1.0 {
        return "0B".into();
    }
    let mut i = 0;
    while x >= 1024.0 && i < 3 {
        x /= 1024.0;
        i += 1;
    }
    if x >= 100.0 {
        format!("{:.0}{}", x, units[i])
    } else if x >= 10.0 {
        format!("{:.1}{}", x, units[i])
    } else {
        format!("{:.2}{}", x, units[i])
    }
}

fn text_width(hdc: HDC, font: HFONT, s: &str) -> i32 {
    unsafe {
        SelectObject(hdc, font);
        let w = s.encode_utf16().collect::<Vec<u16>>();
        let mut sz: SIZE = zeroed();
        if GetTextExtentPoint32W(hdc, w.as_ptr(), w.len() as i32, &mut sz) != 0 {
            sz.cx
        } else {
            0
        }
    }
}

fn tile_width(hdc: HDC, t: &Tile, fonts: &Fonts, big: bool, chart_px: i32) -> i32 {
    let mut w = 0;
    let mut first = true;
    if !t.label.is_empty() {
        w += text_width(hdc, fonts.label(big), &t.label);
        first = false;
    }
    if t.chart.is_some() {
        if !first {
            w += CHART_GAP;
        }
        w += chart_px;
        first = false;
    }
    let mut prev_glue = false;
    for part in &t.parts {
        let glue = part.text == "/";
        if !first && !glue && !prev_glue {
            w += PART_GAP;
        }
        w += text_width(hdc, fonts.value(big), &part.text);
        first = false;
        prev_glue = glue;
    }
    w
}

pub unsafe fn ranges(hwnd: HWND, f: &Frame, fonts: &Fonts) -> Vec<(Metric, i32, i32)> {
    let hdc = GetDC(hwnd);
    let big = f.big();
    let chart_px = f.chart_px();
    let mut x = f.cfg.pad_x;
    let mut out = Vec::new();
    for (i, t) in f.tiles().iter().enumerate() {
        if i > 0 {
            x += if f.compact() {
                COMPACT_GAP
            } else {
                f.cfg.divider_gap * 2 + 1
            };
        }
        let start = x;
        x += tile_width(hdc, t, fonts, big, chart_px);
        out.push((t.metric, start, x));
    }
    ReleaseDC(hwnd, hdc);
    out
}

pub unsafe fn tile_at(hwnd: HWND, f: &Frame, fonts: &Fonts, x: i32) -> Option<Metric> {
    for (m, a, b) in ranges(hwnd, f, fonts) {
        if x >= a - HOVER_PAD && x <= b + HOVER_PAD {
            return Some(m);
        }
    }
    None
}

pub unsafe fn width(hwnd: HWND, f: &Frame, fonts: &Fonts) -> i32 {
    let hdc = GetDC(hwnd);
    let big = f.big();
    let chart_px = f.chart_px();
    let mut total = f.cfg.pad_x * 2;
    for (i, t) in f.tiles().iter().enumerate() {
        if i > 0 {
            total += if f.compact() {
                COMPACT_GAP
            } else {
                f.cfg.divider_gap * 2 + 1
            };
        }
        total += tile_width(hdc, t, fonts, big, chart_px);
    }
    ReleaseDC(hwnd, hdc);
    total
}

pub unsafe fn paint(hwnd: HWND, f: &Frame, fonts: &Fonts, look: &Look) {
    let mut ps: PAINTSTRUCT = zeroed();
    let hdc = BeginPaint(hwnd, &mut ps);
    let mut rc: RECT = zeroed();
    GetClientRect(hwnd, &mut rc);

    let bg = if look.transparent { KEY } else { look.bg };
    let bg_br = CreateSolidBrush(rgb(bg));
    FillRect(hdc, &rc, bg_br);
    DeleteObject(bg_br);

    SetBkMode(hdc, TRANSPARENT as i32);
    let h = rc.bottom - rc.top;
    let big = f.big();
    let chart_px = f.chart_px();
    let mut x = f.cfg.pad_x;
    let tiles = f.tiles();

    for (i, t) in tiles.iter().enumerate() {
        if i > 0 {
            if f.compact() {
                x += COMPACT_GAP;
            } else {
                x += f.cfg.divider_gap;
                let br = CreateSolidBrush(rgb(f.palette.divider));
                let r = RECT {
                    left: x,
                    top: h / 4,
                    right: x + 1,
                    bottom: h - h / 4,
                };
                FillRect(hdc, &r, br);
                DeleteObject(br);
                x += 1 + f.cfg.divider_gap;
            }
        }

        let start = x;
        let tw = tile_width(hdc, t, fonts, big, chart_px);

        if f.hover == Some(t.metric) {
            let hl = blend(bg, f.palette.accent, 0.22);
            let br = CreateSolidBrush(rgb(hl));
            let r = RECT {
                left: (start - HOVER_PAD).max(0),
                top: 2,
                right: start + tw + HOVER_PAD,
                bottom: h - 2,
            };
            FillRect(hdc, &r, br);
            DeleteObject(br);
            let ab = CreateSolidBrush(rgb(f.palette.accent));
            let r = RECT {
                left: (start - HOVER_PAD).max(0),
                top: h - 3,
                right: start + tw + HOVER_PAD,
                bottom: h - 1,
            };
            FillRect(hdc, &r, ab);
            DeleteObject(ab);
        }

        let mut first = true;

        if !t.label.is_empty() {
            draw_text(hdc, x, h, &t.label, fonts.label(big), rgb(f.palette.label));
            x += text_width(hdc, fonts.label(big), &t.label);
            first = false;
        }

        if let Some(ch) = &t.chart {
            if !first {
                x += CHART_GAP;
            }
            let r = RECT {
                left: x,
                top: 3,
                right: x + chart_px,
                bottom: h - 3,
            };
            draw_chart(hdc, &r, ch, f.palette, bg);
            x += chart_px;
            first = false;
        }

        let mut prev_glue = false;
        for part in &t.parts {
            let glue = part.text == "/";
            if !first && !glue && !prev_glue {
                x += PART_GAP;
            }
            draw_text(hdc, x, h, &part.text, fonts.value(big), rgb(part.color));
            x += text_width(hdc, fonts.value(big), &part.text);
            first = false;
            prev_glue = glue;
        }
        x = start + tw;
    }

    if let Some(edge) = look.edge {
        let br = CreateSolidBrush(rgb(edge));
        let strips = [
            RECT {
                left: rc.left,
                top: rc.top,
                right: rc.right,
                bottom: rc.top + 1,
            },
            RECT {
                left: rc.left,
                top: rc.bottom - 1,
                right: rc.right,
                bottom: rc.bottom,
            },
            RECT {
                left: rc.left,
                top: rc.top,
                right: rc.left + 1,
                bottom: rc.bottom,
            },
            RECT {
                left: rc.right - 1,
                top: rc.top,
                right: rc.right,
                bottom: rc.bottom,
            },
        ];
        for strip in &strips {
            FillRect(hdc, strip, br);
        }
        DeleteObject(br);
    }

    EndPaint(hwnd, &mut ps);
}

unsafe fn draw_text(hdc: HDC, x: i32, h: i32, s: &str, font: HFONT, color: u32) {
    SelectObject(hdc, font);
    SetTextColor(hdc, color);
    let w = s.encode_utf16().collect::<Vec<u16>>();
    let mut sz: SIZE = zeroed();
    GetTextExtentPoint32W(hdc, w.as_ptr(), w.len() as i32, &mut sz);
    let y = (h - sz.cy) / 2;
    TextOutW(hdc, x, y, w.as_ptr(), w.len() as i32);
}

unsafe fn draw_chart(hdc: HDC, r: &RECT, ch: &Chart, p: &Palette, bg: [u8; 3]) {
    let w = r.right - r.left;
    let h = r.bottom - r.top;
    if w < 8 || h < 6 {
        return;
    }
    let max = ch.max.max(1.0);

    match ch.kind {
        ChartKind::Bar => {
            let Some((data, color)) = ch.lines.first() else {
                return;
            };
            let n = ((w / 4).max(1)) as usize;
            let start = data.len().saturating_sub(n);
            let count = data.len() - start;
            if count == 0 {
                return;
            }
            let slot = w as f32 / count as f32;
            let bw = (slot - 1.0).max(1.0) as i32;
            let br = CreateSolidBrush(rgb(*color));
            for (i, v) in data[start..].iter().enumerate() {
                let bh = ((v / max).clamp(0.0, 1.0) * (h - 1) as f32) as i32;
                if bh <= 0 {
                    continue;
                }
                let x = r.left + (i as f32 * slot) as i32;
                let rc = RECT {
                    left: x,
                    top: r.bottom - bh,
                    right: x + bw,
                    bottom: r.bottom,
                };
                FillRect(hdc, &rc, br);
            }
            DeleteObject(br);
        }
        ChartKind::Line => {
            for (data, color) in &ch.lines {
                if data.len() < 2 {
                    continue;
                }
                let n = data.len().min(w.max(2) as usize);
                let start = data.len() - n;
                let mut pts: Vec<POINT> = Vec::with_capacity(n);
                for (i, v) in data[start..].iter().enumerate() {
                    let x = r.left
                        + ((i as f32 * (w - 1) as f32) / (n.max(2) - 1) as f32) as i32;
                    let y = r.bottom
                        - ((v / max).clamp(0.0, 1.0) * (h - 1) as f32) as i32;
                    pts.push(POINT { x, y });
                }
                let pen = CreatePen(PS_SOLID, 1, rgb(*color));
                let old = SelectObject(hdc, pen);
                Polyline(hdc, pts.as_ptr(), pts.len() as i32);
                SelectObject(hdc, old);
                DeleteObject(pen);
            }
        }
        ChartKind::Pie => {
            let cx = (r.left + r.right) / 2;
            let cy = (r.top + r.bottom) / 2;
            let rad = ((w.min(h)) / 2 - 1).max(3);
            let total: f32 = ch.pie.iter().map(|(v, _)| v.max(0.0)).sum();
            let old_pen = SelectObject(hdc, GetStockObject(NULL_PEN));

            if total < 0.5 {
                let br = CreateSolidBrush(rgb(p.divider));
                let old_br = SelectObject(hdc, br);
                Ellipse(hdc, cx - rad, cy - rad, cx + rad, cy + rad);
                SelectObject(hdc, old_br);
                DeleteObject(br);
            } else {
                let mut start = 0.0f32;
                for (v, color) in &ch.pie {
                    let sweep = (v.max(0.0) / total) * 360.0;
                    if sweep < 3.0 {
                        start += sweep;
                        continue;
                    }
                    let a0 = start.to_radians();
                    let a1 = (start + sweep).to_radians();
                    let x1 = cx + (rad as f32 * a0.sin()) as i32;
                    let y1 = cy - (rad as f32 * a0.cos()) as i32;
                    let x2 = cx + (rad as f32 * a1.sin()) as i32;
                    let y2 = cy - (rad as f32 * a1.cos()) as i32;
                    let br = CreateSolidBrush(rgb(*color));
                    let old_br = SelectObject(hdc, br);
                    Pie(hdc, cx - rad, cy - rad, cx + rad, cy + rad, x2, y2, x1, y1);
                    SelectObject(hdc, old_br);
                    DeleteObject(br);
                    start += sweep;
                }
            }

            let hole = (rad as f32 * 0.55) as i32;
            let br = CreateSolidBrush(rgb(bg));
            let old_br = SelectObject(hdc, br);
            Ellipse(hdc, cx - hole, cy - hole, cx + hole, cy + hole);
            SelectObject(hdc, old_br);
            DeleteObject(br);
            SelectObject(hdc, old_pen);
        }
    }
}
