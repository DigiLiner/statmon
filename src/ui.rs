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
const CHART_GAP: i32 = 6;

pub enum Item {
    Text {
        text: String,
        value: bool,
        color: u32,
    },
    Divider,
    Gap(i32),
}

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

#[derive(Clone, Copy)]
pub enum ChartKind {
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

pub struct Frame<'a> {
    pub metrics: &'a Metrics,
    pub drives: &'a [DriveInfo],
    pub history: &'a History,
    pub cfg: &'a Config,
    pub palette: &'a Palette,
    pub metric: Option<Metric>,
}

impl<'a> Frame<'a> {
    fn big(&self) -> bool {
        self.metric.is_some()
    }

    pub fn items(&self) -> Vec<Item> {
        let mut items = Vec::new();
        let mut first = true;
        match self.metric {
            Some(m) => push_metric(&mut items, m, self, self.palette, None),
            None => {
                for m in Metric::ALL {
                    if self.cfg.shows(m) {
                        push_metric(&mut items, m, self, self.palette, Some(&mut first));
                    }
                }
            }
        }
        items
    }

    pub fn chart(&self) -> Option<Chart> {
        if self.cfg.graph == Graph::None {
            return None;
        }
        let p = self.palette;
        let h = self.history;
        let m = self.metric;
        let last = |v: &[f32]| v.last().copied().unwrap_or(0.0);

        let (kind, lines, pie, max) = match self.cfg.graph {
            Graph::None => return None,
            Graph::Bar => match m {
                None => (
                    ChartKind::Bar,
                    vec![(h.cpu.clone(), p.cpu)],
                    vec![],
                    100.0,
                ),
                Some(metric) => (
                    ChartKind::Bar,
                    vec![(h.series(metric).to_vec(), metric_color(metric, p))],
                    vec![],
                    chart_max(metric, h.series(metric)),
                ),
            },
            Graph::Line => match m {
                None => (
                    ChartKind::Line,
                    vec![(h.cpu.clone(), p.cpu), (h.ram.clone(), p.ram)],
                    vec![],
                    100.0,
                ),
                Some(metric) => (
                    ChartKind::Line,
                    vec![(h.series(metric).to_vec(), metric_color(metric, p))],
                    vec![],
                    chart_max(metric, h.series(metric)),
                ),
            },
            Graph::Pie => match m {
                None => (
                    ChartKind::Pie,
                    vec![],
                    vec![
                        (last(&h.cpu), p.cpu),
                        (last(&h.ram), p.ram),
                        (last(&h.disk), p.disk),
                    ],
                    100.0,
                ),
                Some(Metric::Net) => (
                    ChartKind::Pie,
                    vec![],
                    vec![
                        (self.metrics.rx as f32, p.net),
                        (self.metrics.tx as f32, p.net_up),
                    ],
                    100.0,
                ),
                Some(metric) => {
                    let v = last(h.series(metric)).clamp(0.0, 100.0);
                    (
                        ChartKind::Pie,
                        vec![],
                        vec![(v, metric_color(metric, p)), (100.0 - v, p.divider)],
                        100.0,
                    )
                }
            },
        };

        Some(Chart {
            kind,
            lines,
            pie,
            max,
        })
    }

    pub fn has_chart(&self) -> bool {
        self.cfg.graph != Graph::None
    }
}

fn chart_max(m: Metric, data: &[f32]) -> f32 {
    if m == Metric::Net {
        let top = data.iter().fold(0.0f32, |a, b| a.max(*b));
        (top * 1.15).max(1024.0)
    } else {
        100.0
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

fn push_metric(
    items: &mut Vec<Item>,
    m: Metric,
    f: &Frame,
    p: &Palette,
    first: Option<&mut bool>,
) {
    let in_taskbar = first.is_some();
    if let Some(first) = first {
        if *first {
            *first = false;
        } else {
            items.push(Item::Gap(f.cfg.divider_gap));
            items.push(Item::Divider);
            items.push(Item::Gap(f.cfg.divider_gap));
        }
    }

    if m == Metric::Capacity {
        if f.drives.is_empty() {
            return;
        }
        if in_taskbar {
            let warn = f.drives.iter().any(|d| d.used_pct() >= 90.0);
            items.push(Item::Text {
                text: capacity_text(f.drives),
                value: true,
                color: rgb(if warn { p.warn } else { p.cap }),
            });
            return;
        }
        items.push(Item::Text {
            text: "DOLU".into(),
            value: false,
            color: rgb(p.label),
        });
        items.push(Item::Gap(f.cfg.inner_gap));
        let warn = f.drives.iter().any(|d| d.used_pct() >= 90.0);
        items.push(Item::Text {
            text: capacity_text(f.drives),
            value: true,
            color: rgb(if warn { p.warn } else { p.cap }),
        });
        return;
    }

    items.push(Item::Text {
        text: m.label().into(),
        value: false,
        color: rgb(p.label),
    });
    items.push(Item::Gap(f.cfg.inner_gap));

    let (text, color) = match m {
        Metric::Cpu => (
            format!("{:>3}%", f.metrics.cpu.round() as i64),
            p.cpu,
        ),
        Metric::Ram => (format!("{:>3}%", f.metrics.mem_pct), p.ram),
        Metric::Net => (
            format!(
                "↓{:>5} ↑{:>5}",
                rate(f.metrics.rx),
                rate(f.metrics.tx)
            ),
            p.net,
        ),
        Metric::Disk => (
            format!(
                "{:>3}% {:>5}",
                f.metrics.disk_busy.round() as i64,
                rate(f.metrics.read_bps + f.metrics.write_bps)
            ),
            p.disk,
        ),
        Metric::Capacity => (String::new(), p.cap),
    };

    items.push(Item::Text {
        text,
        value: true,
        color: rgb(color),
    });
}

fn capacity_text(drives: &[DriveInfo]) -> String {
    let mut parts = Vec::new();
    for d in drives {
        parts.push(format!("{}:{:>3}%", d.letter, d.used_pct().round() as i64));
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

fn chart_width(f: &Frame) -> i32 {
    if f.has_chart() {
        f.cfg.graph_width.max(16) + CHART_GAP
    } else {
        0
    }
}

pub unsafe fn width(hwnd: HWND, f: &Frame, fonts: &Fonts) -> i32 {
    let hdc = GetDC(hwnd);
    let big = f.big();
    let mut total = 0;
    for it in f.items() {
        total += match &it {
            Item::Text { text, value, .. } => {
                let font = if *value {
                    fonts.value(big)
                } else {
                    fonts.label(big)
                };
                text_width(hdc, font, text)
            }
            Item::Divider => 1,
            Item::Gap(g) => *g,
        };
    }
    ReleaseDC(hwnd, hdc);
    total + chart_width(f) + f.cfg.pad_x * 2
}

pub unsafe fn paint(hwnd: HWND, f: &Frame, fonts: &Fonts, transparent: bool) {
    let mut ps: PAINTSTRUCT = zeroed();
    let hdc = BeginPaint(hwnd, &mut ps);
    let mut rc: RECT = zeroed();
    GetClientRect(hwnd, &mut rc);

    let bg = if transparent { KEY } else { f.palette.bg };
    let bg_br = CreateSolidBrush(rgb(bg));
    FillRect(hdc, &rc, bg_br);
    DeleteObject(bg_br);

    SetBkMode(hdc, TRANSPARENT as i32);
    let h = rc.bottom - rc.top;
    let big = f.big();
    let mut x = f.cfg.pad_x;

    if let Some(ch) = f.chart() {
        let cw = f.cfg.graph_width.max(16);
        let r = RECT {
            left: x,
            top: 3,
            right: x + cw,
            bottom: h - 3,
        };
        draw_chart(hdc, &r, &ch, f.palette, bg);
        x += cw + CHART_GAP;
    }

    for it in f.items() {
        match it {
            Item::Text { text, value, color } => {
                let font = if value {
                    fonts.value(big)
                } else {
                    fonts.label(big)
                };
                SelectObject(hdc, font);
                SetTextColor(hdc, color);
                let w = text.encode_utf16().collect::<Vec<u16>>();
                let mut sz: SIZE = zeroed();
                GetTextExtentPoint32W(hdc, w.as_ptr(), w.len() as i32, &mut sz);
                let y = (h - sz.cy) / 2;
                TextOutW(hdc, x, y, w.as_ptr(), w.len() as i32);
                x += sz.cx;
            }
            Item::Divider => {
                let br = CreateSolidBrush(rgb(f.palette.divider));
                let r = RECT {
                    left: x,
                    top: h / 4,
                    right: x + 1,
                    bottom: h - h / 4,
                };
                FillRect(hdc, &r, br);
                DeleteObject(br);
                x += 1;
            }
            Item::Gap(g) => x += g,
        }
    }

    EndPaint(hwnd, &mut ps);
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
                let mut start = -90.0f32;
                for (v, color) in &ch.pie {
                    let sweep = (v.max(0.0) / total) * 360.0;
                    if sweep < 0.4 {
                        start += sweep;
                        continue;
                    }
                    let a0 = start.to_radians();
                    let a1 = (start + sweep).to_radians();
                    let x1 = cx + (rad as f32 * a0.cos()) as i32;
                    let y1 = cy + (rad as f32 * a0.sin()) as i32;
                    let x2 = cx + (rad as f32 * a1.cos()) as i32;
                    let y2 = cy + (rad as f32 * a1.sin()) as i32;
                    let br = CreateSolidBrush(rgb(*color));
                    let old_br = SelectObject(hdc, br);
                    Pie(hdc, cx - rad, cy - rad, cx + rad, cy + rad, x1, y1, x2, y2);
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
