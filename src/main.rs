#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod history;
mod lang;
mod metrics;
mod settings;
mod ui;

use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, InvalidateRect, MonitorFromPoint, MonitorFromWindow, ScreenToClient,
    MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Registry::{
    RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ,
};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::HiDpi::{
    GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT,
};
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use config::{Config, Graph, Palette, ViewMode};
use lang::{t, K};
use history::History;
use metrics::{DriveInfo, Metric, Metrics};
use ui::{Fonts, Frame};

const TIMER_ID: usize = 1;
const TIMER_ANIM: usize = 2;
const TRAY_ID: u32 = 1;
const WM_TRAY: u32 = WM_APP + 1;

const ID_REFRESH: usize = 100;
const ID_AUTOSTART: usize = 101;
const ID_EXIT: usize = 102;
const ID_SETTINGS: usize = 103;
const ID_GRAPH_BASE: usize = 110;
const ID_FLOAT_BASE: usize = 120;
const ID_TRANS: usize = 130;
const ID_TOP: usize = 131;
const ID_COMPACT: usize = 132;
const ID_VIEW_ALL_BASE: usize = 140;
const ID_METRIC_MODE_BASE: usize = 500;
const ID_METRIC_GRAPH_BASE: usize = 520;
const ID_SETTINGS_FOR_BASE: usize = 560;
const ID_FLOAT_CLOSE: usize = 300;

const VPAD: i32 = 6;
const TASKBAR_GAP: i32 = 6;

pub(crate) struct App {
    pub(crate) cfg: Config,
    pub(crate) cfg_path: PathBuf,
    metrics: Metrics,
    drives: Vec<DriveInfo>,
    history: History,
    fonts: Option<Fonts>,
    palette: Palette,
    dpi: u32,
    last_capacity: Instant,
    taskbar_msg: u32,
    taskbar_hwnd: HWND,
    pub(crate) settings_hwnd: HWND,
    pub(crate) settings_target: Option<Metric>,
    taskbar_vertical: bool,
    taskbar_thickness: i32,
    taskbar_rect: RECT,
    taskbar_shown: bool,
    fit_compact: bool,
    fit_hide_charts: bool,
    fit_tiles: usize,
    menu_open: bool,
    led: [[f32; 2]; 5],
    led_target: [[f32; 2]; 5],
    tip_hwnd: HWND,
    tip_metric: Option<Metric>,
    tip_anchor: RECT,
    tip_anchor_vertical: bool,
    floats: Vec<(Metric, HWND)>,
    hidden: bool,
    tick: u32,
}

#[derive(Clone, Copy)]
enum Kind {
    Taskbar,
    Float(Metric),
}

struct Drag {
    start: POINT,
    origin: POINT,
}

struct WinCtx {
    app: *mut App,
    kind: Kind,
    drag: Option<Drag>,
    hover: Option<Metric>,
    tracking: bool,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--autostart") {
        let mode = args.get(i + 1).map(|s| s.as_str()).unwrap_or("status");
        unsafe {
            match mode {
                "on" => set_autostart(true),
                "off" => set_autostart(false),
                _ => {}
            }
            println!(
                "autostart {}",
                if autostart_enabled() { "on" } else { "off" }
            );
        }
        return;
    }
    let once = args.iter().any(|a| a == "--once");
    if once {
        let mut m = Metrics::new();
        std::thread::sleep(Duration::from_millis(1200));
        m.tick();
        println!(
            "cpu {:.1}%  mem {}% ({:.1}/{:.1} GB)  net rx {:.0} B/s tx {:.0} B/s  disk {:.1}% r {:.0} B/s w {:.0} B/s",
            m.cpu,
            m.mem_pct,
            m.mem_used as f64 / 1073741824.0,
            m.mem_total as f64 / 1073741824.0,
            m.rx,
            m.tx,
            m.disk_busy,
            m.read_bps,
            m.write_bps
        );
        for d in metrics::capacity(&[]) {
            println!(
                "{}: {:.0}% ({:.1}/{:.1} GB)",
                d.letter,
                d.used_pct(),
                (d.total - d.free) as f64 / 1073741824.0,
                d.total as f64 / 1073741824.0
            );
        }
        return;
    }
    unsafe { run() }
}

unsafe fn run() {
    let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

    let name = wide("Local\\statmon_singleton");
    let mutex = CreateMutexW(null(), 0, name.as_ptr());
    if mutex.is_null() || GetLastError() == ERROR_ALREADY_EXISTS {
        return;
    }

    let (cfg, cfg_path) = config::load();
    let palette = cfg.palette();
    let app = Box::into_raw(Box::new(App {
        cfg,
        cfg_path,
        metrics: Metrics::new(),
        drives: Vec::new(),
        history: History::new(),
        fonts: None,
        palette,
        dpi: 0,
        last_capacity: Instant::now() - Duration::from_secs(3600),
        taskbar_msg: RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()),
        taskbar_hwnd: null_mut(),
        settings_hwnd: null_mut(),
        settings_target: None,
        taskbar_vertical: false,
        taskbar_thickness: 0,
        taskbar_rect: zeroed(),
        taskbar_shown: false,
        fit_compact: false,
        fit_hide_charts: false,
        fit_tiles: 0,
        menu_open: false,
        led: [[0.0; 2]; 5],
        led_target: [[0.0; 2]; 5],
        tip_hwnd: null_mut(),
        tip_metric: None,
        tip_anchor: zeroed(),
        tip_anchor_vertical: false,
        floats: Vec::new(),
        hidden: false,
        tick: 0,
    }));

    register_widget_class();
    let hwnd = create_widget(app, Kind::Taskbar, 100, 100, 320, 40);
    if hwnd.is_null() {
        return;
    }
    (*app).taskbar_hwnd = hwnd;

    tray_add(hwnd);
    SetTimer(hwnd, TIMER_ID, (*app).cfg.refresh_ms, None);
    SetTimer(hwnd, TIMER_ANIM, 90, None);
    refresh(&mut *app);
    sync_floats(&mut *app);
    update_floats(&mut *app);

    let mut msg: MSG = zeroed();
    while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }

    drop(Box::from_raw(app));
}

unsafe fn register_widget_class() {
    let hinst = GetModuleHandleW(null());
    let cls = wide("StatMonWidget");
    let wc = WNDCLASSW {
        style: 0,
        lpfnWndProc: Some(wndproc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: hinst,
        hIcon: app_icon(),
        hCursor: LoadCursorW(null_mut(), IDC_ARROW),
        hbrBackground: null_mut(),
        lpszMenuName: null(),
        lpszClassName: cls.as_ptr(),
    };
    RegisterClassW(&wc);
}

pub(crate) unsafe fn app_icon() -> HICON {
    let hinst = GetModuleHandleW(null());
    let icon = LoadIconW(hinst, 1 as *const u16);
    if icon.is_null() {
        LoadIconW(null_mut(), IDI_APPLICATION)
    } else {
        icon
    }
}

unsafe fn create_widget(
    app: *mut App,
    kind: Kind,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
) -> HWND {
    let hinst = GetModuleHandleW(null());
    let cls = wide("StatMonWidget");
    let ctx = Box::into_raw(Box::new(WinCtx {
        app,
        kind,
        drag: None,
        hover: None,
        tracking: false,
    }));
    let mut ex = WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;
    match kind {
        Kind::Taskbar => {
            ex |= WS_EX_TOPMOST;
            let cfg = &(*app).cfg;
            if cfg.bar_transparent {
                ex |= WS_EX_LAYERED;
            }
        }
        Kind::Float(m) => {
            let fc = (*app).cfg.float(m);
            if fc.topmost {
                ex |= WS_EX_TOPMOST;
            }
            if fc.transparent || fc.opacity < 100 {
                ex |= WS_EX_LAYERED;
            }
        }
    }
    let hwnd = CreateWindowExW(
        ex,
        cls.as_ptr(),
        wide("StatMon").as_ptr(),
        WS_POPUP,
        x,
        y,
        w,
        h,
        null_mut(),
        null_mut(),
        hinst,
        ctx as *const c_void,
    );
    if hwnd.is_null() {
        drop(Box::from_raw(ctx));
        return hwnd;
    }
    match kind {
        Kind::Taskbar => {
            let cfg = &(*app).cfg;
            apply_style(
                hwnd,
                false,
                if cfg.bar_transparent {
                    cfg.bar_opacity.max(30)
                } else {
                    100
                },
                true,
            );
        }
        Kind::Float(m) => {
            let fc = (*app).cfg.float(m);
            apply_style(hwnd, fc.transparent, fc.opacity, fc.topmost);
        }
    }
    hwnd
}

unsafe fn apply_style(hwnd: HWND, transparent: bool, opacity: u8, topmost: bool) {
    let mut ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as usize;
    ex &= !(WS_EX_LAYERED as usize);
    let alpha = ((opacity.clamp(0, 100) as u32 * 255 + 50) / 100) as u8;
    let layered = transparent || alpha < 255;
    if layered {
        ex |= WS_EX_LAYERED as usize;
    }
    SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex as isize);
    if layered {
        let mut flags = 0;
        if transparent {
            flags |= LWA_COLORKEY;
        }
        if alpha < 255 {
            flags |= LWA_ALPHA;
        }
        let key = if transparent { ui::rgb(ui::KEY) } else { 0 };
        SetLayeredWindowAttributes(hwnd, key, alpha, flags);
    }
    SetWindowPos(
        hwnd,
        if topmost { HWND_TOPMOST } else { HWND_NOTOPMOST },
        0,
        0,
        0,
        0,
        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
    );
}

pub(crate) unsafe fn apply_bar_style(app: &App) {
    if app.taskbar_hwnd.is_null() {
        return;
    }
    apply_style(
        app.taskbar_hwnd,
        false,
        if app.cfg.bar_transparent {
            app.cfg.bar_opacity.max(30)
        } else {
            100
        },
        true,
    );
    InvalidateRect(app.taskbar_hwnd, null(), 0);
}

impl App {
    fn save(&self) {
        config::save(&self.cfg, &self.cfg_path);
    }

    fn float_hwnd(&self, m: Metric) -> Option<HWND> {
        self.floats
            .iter()
            .find(|(x, _)| *x == m)
            .map(|(_, h)| *h)
    }

    pub(crate) unsafe fn apply_config(&mut self, cfg: Config) {
        self.cfg = cfg;
        self.save();
        self.palette = self.cfg.palette();
        if let Some(f) = self.fonts.take() {
            f.destroy();
        }
        self.dpi = 0;
        sync_floats(self);
        apply_bar_style(self);
        SetTimer(self.taskbar_hwnd, TIMER_ID, self.cfg.refresh_ms, None);
        SetTimer(self.taskbar_hwnd, TIMER_ANIM, 90, None);
        refresh(self);
    }

    fn set_float(&mut self, m: Metric, on: bool) {
        self.cfg.float_mut(m).on = on;
        self.save();
    }

    pub(crate) unsafe fn restyle(&mut self) {
        for m in Metric::ALL {
            apply_float_style(self, m);
        }
        self.save();
    }
}

unsafe fn taskbar_metric_at(hwnd: HWND, ctx: &WinCtx) -> Option<Metric> {
    let app = ctx.app.as_ref()?;
    let fonts = app.fonts.as_ref()?;
    let mut p: POINT = zeroed();
    GetCursorPos(&mut p);
    ScreenToClient(hwnd, &mut p);
    let f = frame(app, None, None);
    ui::tile_at(
        hwnd,
        &f,
        fonts,
        if app.taskbar_vertical { p.y } else { p.x },
    )
}

unsafe fn frame<'a>(app: &'a App, metric: Option<Metric>, hover: Option<Metric>) -> Frame<'a> {
    let bar = metric.is_none();
    frame_fit(
        app,
        metric,
        hover,
        bar && app.taskbar_vertical,
        if bar { app.taskbar_thickness } else { 0 },
        bar && app.fit_compact,
        bar && app.fit_hide_charts,
        if bar { app.fit_tiles } else { 0 },
    )
}

#[allow(clippy::too_many_arguments)]
unsafe fn frame_fit<'a>(
    app: &'a App,
    metric: Option<Metric>,
    hover: Option<Metric>,
    vertical: bool,
    thickness: i32,
    fit_compact: bool,
    hide_charts: bool,
    max_tiles: usize,
) -> Frame<'a> {
    Frame {
        metrics: &app.metrics,
        drives: &app.drives,
        history: &app.history,
        cfg: &app.cfg,
        palette: &app.palette,
        metric,
        dpi: app.dpi,
        hover,
        vertical,
        thickness,
        fit_compact,
        hide_charts,
        max_tiles,
        led: app.led,
    }
}

fn any_led(cfg: &Config) -> bool {
    Metric::ALL.iter().any(|m| {
        cfg.shows(*m) && cfg.view(*m).has_graph() && cfg.graph_for(*m) == Graph::Led
    })
}

fn scale_rate(rate: f64, max: f32) -> f32 {
    let n = (rate as f32 / max).clamp(0.0, 1.0).powf(0.45);
    if rate >= 65536.0 {
        n.max(0.55)
    } else if rate >= 1024.0 {
        n.max(0.35)
    } else {
        n
    }
}

unsafe fn update_led_targets(app: &mut App) {
    let (net_max, disk_max) = {
        let h = &app.history;
        let n = h
            .rx
            .iter()
            .chain(h.tx.iter())
            .fold(0.0f32, |a, b| a.max(*b))
            .max(65536.0);
        let d = h
            .read
            .iter()
            .chain(h.write.iter())
            .fold(0.0f32, |a, b| a.max(*b))
            .max(65536.0);
        (n, d)
    };
    let cap = app
        .drives
        .iter()
        .map(|d| d.used_pct())
        .fold(0.0f32, |a, b| a.max(b))
        / 100.0;

    let targets = [
        [(app.metrics.cpu / 100.0).clamp(0.0, 1.0), 0.0],
        [(app.metrics.mem_pct as f32 / 100.0).clamp(0.0, 1.0), 0.0],
        [
            scale_rate(app.metrics.rx, net_max),
            scale_rate(app.metrics.tx, net_max),
        ],
        [
            scale_rate(app.metrics.read_bps, disk_max),
            scale_rate(app.metrics.write_bps, disk_max),
        ],
        [cap.clamp(0.0, 1.0), 0.0],
    ];
    app.led_target = targets;
}

unsafe fn animate_led(app: &mut App) {
    if !any_led(&app.cfg) {
        return;
    }
    let mut active = false;
    for i in 0..5 {
        for k in 0..2 {
            let target = app.led_target[i][k].clamp(0.0, 1.0);
            let cur = app.led[i][k];
            let diff = target - cur;
            if diff.abs() > 0.004 {
                app.led[i][k] = cur + diff * 0.18;
                active = true;
            } else {
                app.led[i][k] = target;
            }
        }
    }
    if !active {
        return;
    }
    if !app.taskbar_hwnd.is_null() {
        InvalidateRect(app.taskbar_hwnd, null(), 0);
    }
    for (_, h) in &app.floats {
        InvalidateRect(*h, null(), 0);
    }
}

unsafe fn refresh(app: &mut App) {
    app.metrics.tick();

    if app.last_capacity.elapsed().as_secs() >= app.cfg.capacity_secs as u64 {
        app.drives = metrics::capacity(&app.cfg.drives);
        app.last_capacity = Instant::now();
    }
    app.history.push(&app.metrics, &app.drives);
    update_led_targets(app);

    ensure_fonts(app);
    place_taskbar(app);
    update_floats(app);

    if app.tip_metric.is_some() {
        update_tip(app);
    }

    app.tick = app.tick.wrapping_add(1);
    if app.tick % 5 == 1 {
        tray_tip(app.taskbar_hwnd, &app.metrics);
    }
}

unsafe fn ensure_fonts(app: &mut App) {
    if app.taskbar_hwnd.is_null() {
        return;
    }
    let dpi = GetDpiForWindow(app.taskbar_hwnd).max(96);
    if dpi != app.dpi {
        if let Some(f) = app.fonts.take() {
            f.destroy();
        }
        app.dpi = dpi;
    }
    if app.fonts.is_none() {
        app.fonts = Some(Fonts::new(dpi, &app.cfg));
    }
}

unsafe fn place_taskbar(app: &mut App) {
    if app.menu_open {
        return;
    }
    if foreground_is_fullscreen(app.taskbar_hwnd) {
        set_hidden(app, true);
        return;
    }

    let Some((tb, tray)) = taskbar_rects() else {
        return;
    };
    let mon = MonitorFromWindow(app.taskbar_hwnd, MONITOR_DEFAULTTONEAREST);
    let mut mi: MONITORINFO = zeroed();
    mi.cbSize = size_of::<MONITORINFO>() as u32;
    let mon_ok = !mon.is_null() && GetMonitorInfoW(mon, &mut mi) != 0;

    let tb_w = tb.right - tb.left;
    let tb_h = tb.bottom - tb.top;
    let horizontal = tb_w >= tb_h;
    let thickness = if horizontal { tb_h } else { tb_w };
    let off_screen = if horizontal {
        mon_ok && (tb.top >= mi.rcMonitor.bottom - 1 || tb.bottom <= mi.rcMonitor.top + 1)
    } else {
        mon_ok && (tb.left >= mi.rcMonitor.right - 1 || tb.right <= mi.rcMonitor.left + 1)
    };
    if off_screen || IsWindowVisible(FindWindowW(wide("Shell_TrayWnd").as_ptr(), null())) == 0 {
        set_hidden(app, true);
        return;
    }
    app.taskbar_vertical = !horizontal;
    app.taskbar_thickness = thickness;

    let limit = if horizontal {
        if tray.left > tb.left + 8 && tray.left < tb.right {
            tray.left
        } else {
            tb.right
        }
    } else if tray.top > tb.top + 8 && tray.top < tb.bottom {
        tray.top
    } else {
        tb.bottom
    };
    let avail = if horizontal {
        (limit - tb.left - 8).max(0)
    } else {
        (limit - tb.top - 8).max(0)
    };

    let tiles_len = Metric::ALL
        .iter()
        .filter(|m| app.cfg.shows(**m))
        .count();
    if tiles_len == 0 {
        set_hidden(app, true);
        return;
    }

    let chosen = {
        let Some(fonts) = app.fonts.as_ref() else {
            return;
        };
        let measure = |c: bool, nc: bool, mt: usize| -> (i32, i32) {
            let f = frame_fit(
                app,
                None,
                None,
                app.taskbar_vertical,
                thickness,
                c,
                nc,
                mt,
            );
            if horizontal {
                (
                    ui::width(app.taskbar_hwnd, &f, fonts),
                    (thickness - VPAD * 2).max(16),
                )
            } else {
                ((thickness - 4).max(30), ui::height_vertical(&f))
            }
        };
        let fits = |w: i32, h: i32| {
            if horizontal {
                w <= avail
            } else {
                h <= avail
            }
        };
        let mut out = None;
        for (c, nc) in [(false, false), (true, false), (true, true)] {
            let (w, h) = measure(c, nc, 0);
            if fits(w, h) {
                out = Some((c, nc, 0usize, w, h));
                break;
            }
        }
        if out.is_none() {
            for n in (1..tiles_len).rev() {
                let (w, h) = measure(true, true, n);
                if fits(w, h) {
                    out = Some((true, true, n, w, h));
                    break;
                }
            }
        }
        out
    };

    let Some((fit_compact, hide_charts, max_tiles, width, height)) = chosen else {
        set_hidden(app, true);
        return;
    };
    app.fit_compact = fit_compact;
    app.fit_hide_charts = hide_charts;
    app.fit_tiles = max_tiles;

    set_hidden(app, false);

    let (x, y) = if horizontal {
        (
            (limit - width - TASKBAR_GAP).max(tb.left + 4),
            tb.top + (thickness - height) / 2,
        )
    } else {
        (
            tb.left + (thickness - width) / 2,
            (limit - height - TASKBAR_GAP).max(tb.top + 4),
        )
    };
    let target = RECT {
        left: x,
        top: y,
        right: x + width,
        bottom: y + height,
    };
    let changed = target.left != app.taskbar_rect.left
        || target.top != app.taskbar_rect.top
        || target.right != app.taskbar_rect.right
        || target.bottom != app.taskbar_rect.bottom;
    if changed || !app.taskbar_shown {
        SetWindowPos(
            app.taskbar_hwnd,
            HWND_TOPMOST,
            x,
            y,
            width,
            height,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
        app.taskbar_rect = target;
        app.taskbar_shown = true;
    } else {
        SetWindowPos(
            app.taskbar_hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
        );
    }
    InvalidateRect(app.taskbar_hwnd, null(), 0);
}

unsafe fn update_floats(app: &mut App) {
    if app.menu_open {
        return;
    }
    let Some(fonts) = app.fonts.as_ref() else {
        return;
    };
    let dpi = app.dpi.max(96) as i32;
    let height = ((app.cfg.float_font_px * dpi / 96) * 2 + 10).max(22);
    for i in 0..app.floats.len() {
        let (m, h) = app.floats[i];
        let (fx, fy, topmost) = {
            let fc = app.cfg.float(m);
            (fc.x, fc.y, fc.topmost)
        };
        let f = frame(app, Some(m), None);
        let width = ui::width(h, &f, fonts);
        let (cx, cy) = clamp_to_work(fx, fy, width, height);
        if cx != fx || cy != fy {
            let fc = app.cfg.float_mut(m);
            fc.x = cx;
            fc.y = cy;
        }
        let level = if topmost { HWND_TOPMOST } else { HWND_NOTOPMOST };
        SetWindowPos(
            h,
            level,
            cx,
            cy,
            width,
            height,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
        InvalidateRect(h, null(), 0);
    }
}

unsafe fn clamp_to_work(x: i32, y: i32, w: i32, h: i32) -> (i32, i32) {
    let mon = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
    let mut mi: MONITORINFO = zeroed();
    mi.cbSize = size_of::<MONITORINFO>() as u32;
    if mon.is_null() || GetMonitorInfoW(mon, &mut mi) == 0 {
        return (x, y);
    }
    let wr = mi.rcWork;
    let max_x = (wr.right - w).max(wr.left);
    let max_y = (wr.bottom - h).max(wr.top);
    (x.clamp(wr.left, max_x), y.clamp(wr.top, max_y))
}

unsafe fn set_hidden(app: &mut App, hidden: bool) {
    if app.hidden == hidden {
        return;
    }
    app.hidden = hidden;
    app.taskbar_shown = false;
    if hidden {
        tip_hide(app);
    }
    let cmd = if hidden { SW_HIDE } else { SW_SHOWNOACTIVATE };
    if !app.taskbar_hwnd.is_null() {
        ShowWindow(app.taskbar_hwnd, cmd);
    }
    for (_, h) in &app.floats {
        ShowWindow(*h, cmd);
    }
}

unsafe fn sync_floats(app: &mut App) {
    for m in Metric::ALL {
        let want = app.cfg.float(m).on;
        let have = app.float_hwnd(m);
        if want && have.is_none() {
            let fc = app.cfg.float(m).clone();
            let ptr = app as *mut App;
            let h = create_widget(ptr, Kind::Float(m), fc.x, fc.y, 160, 26);
            app.floats.push((m, h));
        } else if !want {
            if let Some(h) = have {
                if let Some(pos) = app.floats.iter().position(|(x, _)| *x == m) {
                    app.floats.remove(pos);
                }
                DestroyWindow(h);
            }
        }
    }
    for m in Metric::ALL {
        apply_float_style(app, m);
    }
}

unsafe fn apply_float_style(app: &mut App, m: Metric) {
    let Some(h) = app.float_hwnd(m) else {
        return;
    };
    let fc = app.cfg.float(m).clone();
    apply_style(h, fc.transparent, fc.opacity, fc.topmost);
    InvalidateRect(h, null(), 0);
}

unsafe fn taskbar_rects() -> Option<(RECT, RECT)> {
    let tb = FindWindowW(wide("Shell_TrayWnd").as_ptr(), null());
    if tb.is_null() {
        return None;
    }
    let mut tr: RECT = zeroed();
    if GetWindowRect(tb, &mut tr) == 0 {
        return None;
    }
    let mut tray = tr;
    let t = FindWindowExW(tb, null_mut(), wide("TrayNotifyWnd").as_ptr(), null());
    if !t.is_null() {
        let mut r2: RECT = zeroed();
        if GetWindowRect(t, &mut r2) != 0 {
            tray = r2;
        }
    }
    Some((tr, tray))
}

unsafe fn foreground_is_fullscreen(hwnd: HWND) -> bool {
    let fg = GetForegroundWindow();
    if fg.is_null() || fg == hwnd {
        return false;
    }
    let mut cls = [0u16; 64];
    let n = GetClassNameW(fg, cls.as_mut_ptr(), cls.len() as i32);
    if n > 0 {
        let name = String::from_utf16_lossy(&cls[..n as usize]);
        if matches!(name.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd") {
            return false;
        }
    }
    let mut r: RECT = zeroed();
    if GetWindowRect(fg, &mut r) == 0 {
        return false;
    }
    let mon = MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST);
    let mut mi: MONITORINFO = zeroed();
    mi.cbSize = size_of::<MONITORINFO>() as u32;
    if mon.is_null() || GetMonitorInfoW(mon, &mut mi) == 0 {
        return false;
    }
    let m = mi.rcMonitor;
    r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom
}

unsafe fn tray_add(hwnd: HWND) {
    let mut nid: NOTIFYICONDATAW = zeroed();
    nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = TRAY_ID;
    nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    nid.uCallbackMessage = WM_TRAY;
    nid.hIcon = app_icon();
    put_tip(&mut nid, "StatMon");
    Shell_NotifyIconW(NIM_ADD, &nid);
}

unsafe fn tray_tip(hwnd: HWND, m: &Metrics) {
    if hwnd.is_null() {
        return;
    }
    let mut nid: NOTIFYICONDATAW = zeroed();
    nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = TRAY_ID;
    nid.uFlags = NIF_TIP;
    let tip = format!(
        "CPU {:.0}%  RAM {}% ({:.1}/{:.1} GB)\nNET \u{2193}{}  \u{2191}{}\nDISK {:.0}%  {}",
        m.cpu,
        m.mem_pct,
        m.mem_used as f64 / 1073741824.0,
        m.mem_total as f64 / 1073741824.0,
        ui::rate(m.rx),
        ui::rate(m.tx),
        m.disk_busy,
        ui::rate(m.read_bps + m.write_bps)
    );
    put_tip(&mut nid, &tip);
    Shell_NotifyIconW(NIM_MODIFY, &nid);
}

fn put_tip(nid: &mut NOTIFYICONDATAW, s: &str) {
    let chars: Vec<u16> = s.encode_utf16().take(127).collect();
    for (i, c) in chars.iter().enumerate() {
        nid.szTip[i] = *c;
    }
    nid.szTip[chars.len()] = 0;
}

unsafe fn show_menu(app: *mut App, hwnd: HWND, metric: Option<Metric>) {
    let a = &mut *app;
    let lang = a.cfg.language;
    tip_hide(a);
    let menu = CreatePopupMenu();

    let checked = |on: bool| MF_STRING | if on { MF_CHECKED } else { 0 };

    if let Some(m) = metric {
        AppendMenuW(
            menu,
            MF_STRING | MF_DISABLED | MF_GRAYED,
            0,
            wide(m.label()).as_ptr(),
        );
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        let idx = Metric::ALL.iter().position(|x| *x == m).unwrap_or(0);
        AppendMenuW(
            menu,
            MF_STRING,
            ID_SETTINGS_FOR_BASE + idx,
            wide(t(lang, K::ThisMetricSettings)).as_ptr(),
        );

        let mm = CreatePopupMenu();
        for v in ViewMode::ALL {
            AppendMenuW(
                mm,
                checked(a.cfg.view(m) == v),
                ID_METRIC_MODE_BASE + idx * 3 + v.index(),
                wide(v.label_l(lang)).as_ptr(),
            );
        }
        AppendMenuW(
            menu,
            MF_POPUP,
            mm as usize,
            wide(t(lang, K::ThisMetricView)).as_ptr(),
        );

        let mg = CreatePopupMenu();
        for g in Graph::ALL {
            AppendMenuW(
                mg,
                checked(a.cfg.graph_for(m) == g),
                ID_METRIC_GRAPH_BASE + idx * 4 + g.index(),
                wide(g.label_l(lang)).as_ptr(),
            );
        }
        AppendMenuW(
            menu,
            MF_POPUP,
            mg as usize,
            wide(t(lang, K::ThisMetricChart)).as_ptr(),
        );
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
    }

    AppendMenuW(menu, MF_STRING, ID_SETTINGS, wide(t(lang, K::Settings)).as_ptr());
    AppendMenuW(menu, MF_SEPARATOR, 0, null());

    let gm = CreatePopupMenu();
    for g in Graph::ALL {
        AppendMenuW(
            gm,
            checked(a.cfg.graph == g),
            ID_GRAPH_BASE + g.index(),
            wide(g.label_l(lang)).as_ptr(),
        );
    }
    AppendMenuW(menu, MF_POPUP, gm as usize, wide(t(lang, K::ChartAll)).as_ptr());

    let fm = CreatePopupMenu();
    for (i, m) in Metric::ALL.iter().enumerate() {
        AppendMenuW(
            fm,
            checked(a.cfg.float(*m).on),
            ID_FLOAT_BASE + i,
            wide(m.label()).as_ptr(),
        );
    }
    AppendMenuW(fm, MF_SEPARATOR, 0, null());
    let all_trans = Metric::ALL
        .iter()
        .all(|m| a.cfg.float(*m).transparent);
    let all_top = Metric::ALL.iter().all(|m| a.cfg.float(*m).topmost);
    AppendMenuW(
        fm,
        checked(all_trans),
        ID_TRANS,
        wide(t(lang, K::TransparentAll)).as_ptr(),
    );
    AppendMenuW(
        fm,
        checked(all_top),
        ID_TOP,
        wide(t(lang, K::TopmostAll)).as_ptr(),
    );
    AppendMenuW(menu, MF_POPUP, fm as usize, wide(t(lang, K::Floats)).as_ptr());

    let vm = CreatePopupMenu();
    for (i, v) in ViewMode::ALL.iter().enumerate() {
        AppendMenuW(
            vm,
            MF_STRING,
            ID_VIEW_ALL_BASE + i,
            wide(v.label_l(lang)).as_ptr(),
        );
    }
    AppendMenuW(menu, MF_POPUP, vm as usize, wide(t(lang, K::ViewAll)).as_ptr());

    AppendMenuW(
        menu,
        checked(a.cfg.compact),
        ID_COMPACT,
        wide(t(lang, K::Compact)).as_ptr(),
    );

    if let Some(m) = metric {
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        let idx = Metric::ALL.iter().position(|x| *x == m).unwrap_or(0);
        if a.cfg.float(m).on {
            AppendMenuW(
                menu,
                MF_STRING,
                ID_FLOAT_CLOSE + idx,
                wide(t(lang, K::CloseWidget)).as_ptr(),
            );
        }
    }

    AppendMenuW(menu, MF_SEPARATOR, 0, null());
    AppendMenuW(menu, MF_STRING, ID_REFRESH, wide(t(lang, K::Refresh)).as_ptr());
    AppendMenuW(
        menu,
        checked(autostart_enabled()),
        ID_AUTOSTART,
        wide(t(lang, K::Autostart)).as_ptr(),
    );
    AppendMenuW(menu, MF_SEPARATOR, 0, null());
    AppendMenuW(menu, MF_STRING, ID_EXIT, wide(t(lang, K::Exit)).as_ptr());

    let mut pt: POINT = zeroed();
    GetCursorPos(&mut pt);
    SetForegroundWindow(hwnd);
    a.menu_open = true;
    TrackPopupMenu(
        menu,
        TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
        pt.x,
        pt.y,
        0,
        hwnd,
        null(),
    );
    DestroyMenu(menu);
    a.menu_open = false;
    PostMessageW(hwnd, WM_NULL, 0, 0);
}

unsafe fn on_command(app: *mut App, id: usize) {
    if id == ID_SETTINGS {
        settings::open(app, None);
        return;
    }
    if (ID_SETTINGS_FOR_BASE..ID_SETTINGS_FOR_BASE + 5).contains(&id) {
        settings::open(app, Some(Metric::ALL[id - ID_SETTINGS_FOR_BASE]));
        return;
    }
    if (ID_METRIC_MODE_BASE..ID_METRIC_MODE_BASE + 15).contains(&id) {
        let rel = id - ID_METRIC_MODE_BASE;
        let idx = rel / 3;
        let mode = ViewMode::from_index(rel % 3);
        let a = &mut *app;
        *a.cfg.view_mut(Metric::ALL[idx]) = mode;
        a.save();
        refresh(a);
        return;
    }
    if (ID_METRIC_GRAPH_BASE..ID_METRIC_GRAPH_BASE + 20).contains(&id) {
        let rel = id - ID_METRIC_GRAPH_BASE;
        let idx = rel / 4;
        let g = Graph::from_index(rel % 4);
        let a = &mut *app;
        a.cfg.set_graph_for(Metric::ALL[idx], g);
        a.save();
        refresh(a);
        return;
    }
    let a = &mut *app;
    if id == ID_REFRESH {
        a.last_capacity = Instant::now() - Duration::from_secs(3600);
        refresh(a);
        return;
    }
    if id == ID_AUTOSTART {
        set_autostart(!autostart_enabled());
        return;
    }
    if id == ID_EXIT {
        quit(a);
        return;
    }
    if id == ID_TRANS {
        let on = !Metric::ALL.iter().all(|m| a.cfg.float(*m).transparent);
        for m in Metric::ALL {
            a.cfg.float_mut(m).transparent = on;
        }
        a.save();
        sync_floats(a);
        refresh(a);
        return;
    }
    if id == ID_TOP {
        let on = !Metric::ALL.iter().all(|m| a.cfg.float(*m).topmost);
        for m in Metric::ALL {
            a.cfg.float_mut(m).topmost = on;
        }
        a.save();
        sync_floats(a);
        refresh(a);
        return;
    }
    if (ID_VIEW_ALL_BASE..ID_VIEW_ALL_BASE + 3).contains(&id) {
        let mode = ViewMode::from_index(id - ID_VIEW_ALL_BASE);
        for m in Metric::ALL {
            *a.cfg.view_mut(m) = mode;
        }
        a.save();
        refresh(a);
        return;
    }
    if id == ID_COMPACT {
        a.cfg.compact = !a.cfg.compact;
        a.save();
        refresh(a);
        return;
    }
    if (ID_GRAPH_BASE..ID_GRAPH_BASE + 4).contains(&id) {
        a.cfg.set_graph_all(Graph::from_index(id - ID_GRAPH_BASE));
        a.save();
        refresh(a);
        return;
    }
    if (ID_FLOAT_BASE..ID_FLOAT_BASE + 5).contains(&id) {
        let m = Metric::ALL[id - ID_FLOAT_BASE];
        a.set_float(m, !a.cfg.float(m).on);
        sync_floats(a);
        refresh(a);
        return;
    }
    if (ID_FLOAT_CLOSE..ID_FLOAT_CLOSE + 5).contains(&id) {
        let m = Metric::ALL[id - ID_FLOAT_CLOSE];
        a.set_float(m, false);
        sync_floats(a);
        refresh(a);
    }
}

unsafe fn quit(app: &mut App) {
    if !app.settings_hwnd.is_null() {
        PostMessageW(app.settings_hwnd, WM_CLOSE, 0, 0);
    }
    for (_, h) in &app.floats {
        PostMessageW(*h, WM_CLOSE, 0, 0);
    }
    if !app.taskbar_hwnd.is_null() {
        PostMessageW(app.taskbar_hwnd, WM_CLOSE, 0, 0);
    }
}

pub(crate) unsafe fn autostart_enabled() -> bool {
    let sub = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    let name = wide("StatMon");
    let mut data = [0u16; 512];
    let mut cb = (data.len() * 2) as u32;
    RegGetValueW(
        HKEY_CURRENT_USER,
        sub.as_ptr(),
        name.as_ptr(),
        RRF_RT_REG_SZ,
        null_mut(),
        data.as_mut_ptr() as *mut c_void,
        &mut cb,
    ) == 0
}

pub(crate) unsafe fn set_autostart(on: bool) {
    let sub = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    let name = wide("StatMon");
    if on {
        let exe = std::env::current_exe().unwrap_or_default();
        let val = wide(&format!("\"{}\"", exe.display()));
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            sub.as_ptr(),
            name.as_ptr(),
            REG_SZ,
            val.as_ptr() as *const c_void,
            (val.len() * 2) as u32,
        );
    } else {
        RegDeleteKeyValueW(HKEY_CURRENT_USER, sub.as_ptr(), name.as_ptr());
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, w: usize, l: isize) -> LRESULT {
    match msg {
        WM_NCCREATE => {
            let cs = &*(l as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
            DefWindowProcW(hwnd, msg, w, l)
        }
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            let ctx = ctx_ptr(hwnd);
            if let Some(ctx) = ctx.as_ref() {
                if let Some(app) = ctx.app.as_ref() {
                    if let Some(fonts) = app.fonts.as_ref() {
                        match ctx.kind {
                            Kind::Taskbar => {
                                let f = frame(app, None, ctx.hover);
                                let look = ui::Look {
                                    transparent: false,
                                    bg: config::parse_color(&app.cfg.bar_bg)
                                        .unwrap_or(app.palette.bg),
                                    edge: None,
                                };
                                ui::paint(hwnd, &f, fonts, &look);
                            }
                            Kind::Float(m) => {
                                let fc = app.cfg.float(m);
                                let f = frame(app, Some(m), ctx.hover);
                                let edge = if ctx.hover == Some(m) {
                                    Some(app.palette.accent)
                                } else if fc.border {
                                    Some(
                                        config::parse_color(&fc.border_color)
                                            .unwrap_or(app.palette.divider),
                                    )
                                } else {
                                    None
                                };
                                let look = ui::Look {
                                    transparent: fc.transparent,
                                    bg: config::parse_color(&fc.bg).unwrap_or(app.palette.bg),
                                    edge,
                                };
                                ui::paint(hwnd, &f, fonts, &look);
                            }
                        }
                    }
                }
            }
            0
        }
        WM_TIMER => {
            let ctx = ctx_ptr(hwnd);
            if let Some(ctx) = ctx.as_ref() {
                if let Kind::Taskbar = ctx.kind {
                    if let Some(app) = ctx.app.as_mut() {
                        if w == TIMER_ANIM {
                            animate_led(app);
                        } else {
                            refresh(app);
                        }
                    }
                }
            }
            0
        }
        WM_COMMAND => {
            let ctx = ctx_ptr(hwnd);
            if let Some(ctx) = ctx.as_ref() {
                on_command(ctx.app, w & 0xFFFF);
            }
            0
        }
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        WM_RBUTTONUP | WM_CONTEXTMENU => {
            let ctx = ctx_ptr(hwnd);
            if let Some(ctx) = ctx.as_ref() {
                let metric = match ctx.kind {
                    Kind::Float(m) => Some(m),
                    Kind::Taskbar => taskbar_metric_at(hwnd, ctx),
                };
                show_menu(ctx.app, hwnd, metric);
            }
            0
        }
        WM_LBUTTONDOWN => {
            let ctx = ctx_ptr(hwnd);
            if let Some(ctx) = ctx.as_mut() {
                if let Kind::Float(_) = ctx.kind {
                    let mut p: POINT = zeroed();
                    GetCursorPos(&mut p);
                    let mut r: RECT = zeroed();
                    GetWindowRect(hwnd, &mut r);
                    ctx.drag = Some(Drag {
                        start: p,
                        origin: POINT {
                            x: r.left,
                            y: r.top,
                        },
                    });
                    SetCapture(hwnd);
                }
            }
            0
        }
        WM_MOUSEMOVE => {
            let ctx = ctx_ptr(hwnd);
            if let Some(ctx) = ctx.as_mut() {
                if let Some(d) = &ctx.drag {
                    let mut p: POINT = zeroed();
                    GetCursorPos(&mut p);
                    let nx = d.origin.x + (p.x - d.start.x);
                    let ny = d.origin.y + (p.y - d.start.y);
                    SetWindowPos(
                        hwnd,
                        null_mut(),
                        nx,
                        ny,
                        0,
                        0,
                        SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                    if let Kind::Float(m) = ctx.kind {
                        if let Some(app) = ctx.app.as_mut() {
                            let fc = app.cfg.float_mut(m);
                            fc.x = nx;
                            fc.y = ny;
                        }
                    }
                } else {
                    if !ctx.tracking {
                        let mut tme: TRACKMOUSEEVENT = zeroed();
                        tme.cbSize = size_of::<TRACKMOUSEEVENT>() as u32;
                        tme.dwFlags = TME_LEAVE;
                        tme.hwndTrack = hwnd;
                        TrackMouseEvent(&mut tme);
                        ctx.tracking = true;
                    }
                    let hover = match ctx.kind {
                        Kind::Taskbar => taskbar_metric_at(hwnd, ctx),
                        Kind::Float(m) => Some(m),
                    };
                    if ctx.hover != hover {
                        ctx.hover = hover;
                        InvalidateRect(hwnd, null(), 0);
                        match hover {
                            Some(m) => tip_show(&mut *ctx.app, m, hwnd),
                            None => tip_hide(&mut *ctx.app),
                        }
                    }
                }
            }
            0
        }
        0x02A3 => {
            let ctx = ctx_ptr(hwnd);
            if let Some(ctx) = ctx.as_mut() {
                ctx.tracking = false;
                if ctx.hover.take().is_some() {
                    InvalidateRect(hwnd, null(), 0);
                }
                tip_hide(&mut *ctx.app);
            }
            0
        }
        WM_LBUTTONUP => {
            let ctx = ctx_ptr(hwnd);
            if let Some(ctx) = ctx.as_mut() {
                if ctx.drag.take().is_some() {
                    ReleaseCapture();
                    let mut r: RECT = zeroed();
                    GetWindowRect(hwnd, &mut r);
                    if let Kind::Float(m) = ctx.kind {
                        if let Some(app) = ctx.app.as_mut() {
                            let fc = app.cfg.float_mut(m);
                            fc.x = r.left;
                            fc.y = r.top;
                            app.save();
                        }
                    }
                }
            }
            0
        }
        WM_CAPTURECHANGED => {
            let ctx = ctx_ptr(hwnd);
            if let Some(ctx) = ctx.as_mut() {
                ctx.drag = None;
            }
            0
        }
        WM_DPICHANGED => {
            let ctx = ctx_ptr(hwnd);
            if let Some(ctx) = ctx.as_ref() {
                if let Some(app) = ctx.app.as_mut() {
                    app.dpi = 0;
                    refresh(app);
                }
            }
            0
        }
        WM_SETTINGCHANGE => {
            let ctx = ctx_ptr(hwnd);
            if let Some(ctx) = ctx.as_ref() {
                if let Some(app) = ctx.app.as_mut() {
                    app.palette = app.cfg.palette();
                    refresh(app);
                }
            }
            0
        }
        WM_NCDESTROY => {
            let ctx = ctx_ptr(hwnd);
            if !ctx.is_null() {
                let kind = (*ctx).kind;
                let app = (*ctx).app;
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(ctx));
                if !app.is_null() {
                    if let Kind::Taskbar = kind {
                        let mut nid: NOTIFYICONDATAW = zeroed();
                        nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
                        nid.hWnd = hwnd;
                        nid.uID = TRAY_ID;
                        Shell_NotifyIconW(NIM_DELETE, &nid);
                        KillTimer(hwnd, TIMER_ID);
                        KillTimer(hwnd, TIMER_ANIM);
                        if !(*app).tip_hwnd.is_null() {
                            DestroyWindow((*app).tip_hwnd);
                            (*app).tip_hwnd = null_mut();
                        }
                        PostQuitMessage(0);
                    }
                }
            }
            0
        }
        _ => {
            let ctx = ctx_ptr(hwnd);
            if msg == WM_TRAY {
                if l as u32 == WM_RBUTTONUP || l as u32 == WM_CONTEXTMENU {
                    if let Some(ctx) = ctx.as_ref() {
                        show_menu(ctx.app, hwnd, None);
                    }
                }
                return 0;
            }
            if !ctx.is_null() {
                if let Some(ctx) = ctx.as_ref() {
                    if let Some(app) = ctx.app.as_ref() {
                        if msg == app.taskbar_msg {
                            tray_add(hwnd);
                            refresh(&mut *ctx.app);
                            return 0;
                        }
                    }
                }
            }
            DefWindowProcW(hwnd, msg, w, l)
        }
    }
}

fn ctx_ptr(hwnd: HWND) -> *mut WinCtx {
    unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WinCtx }
}

unsafe fn register_tip_class() {
    let hinst = GetModuleHandleW(null());
    let cls = wide("StatMonTip");
    let wc = WNDCLASSW {
        style: 0,
        lpfnWndProc: Some(wndproc_tip),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: hinst,
        hIcon: app_icon(),
        hCursor: LoadCursorW(null_mut(), IDC_ARROW),
        hbrBackground: null_mut(),
        lpszMenuName: null(),
        lpszClassName: cls.as_ptr(),
    };
    RegisterClassW(&wc);
}

unsafe fn ensure_tip(app: &mut App) {
    if !app.tip_hwnd.is_null() {
        return;
    }
    register_tip_class();
    let ptr = app as *mut App;
    let hwnd = CreateWindowExW(
        WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_TRANSPARENT | WS_EX_LAYERED,
        wide("StatMonTip").as_ptr(),
        wide("StatMon").as_ptr(),
        WS_POPUP,
        0,
        0,
        260,
        60,
        null_mut(),
        null_mut(),
        GetModuleHandleW(null()),
        ptr as *const c_void,
    );
    if hwnd.is_null() {
        return;
    }
    SetLayeredWindowAttributes(hwnd, 0, 240, LWA_ALPHA);
    app.tip_hwnd = hwnd;
}

unsafe fn tip_show(app: &mut App, m: Metric, src: HWND) {
    ensure_tip(app);
    if app.tip_hwnd.is_null() {
        return;
    }
    app.tip_metric = Some(m);
    let mut r: RECT = zeroed();
    GetWindowRect(src, &mut r);
    app.tip_anchor_vertical = (r.bottom - r.top) > (r.right - r.left);
    app.tip_anchor = r;
    update_tip(app);
}

unsafe fn tip_hide(app: &mut App) {
    app.tip_metric = None;
    if !app.tip_hwnd.is_null() {
        ShowWindow(app.tip_hwnd, SW_HIDE);
    }
}

unsafe fn update_tip(app: &mut App) {
    let Some(m) = app.tip_metric else {
        return;
    };
    if app.tip_hwnd.is_null() {
        return;
    }
    let f = frame_fit(app, Some(m), None, false, 0, false, false, 0);
    let (w, h) = ui::tip_size(&f);

    let mut p: POINT = zeroed();
    GetCursorPos(&mut p);
    let mon = MonitorFromPoint(p, MONITOR_DEFAULTTONEAREST);
    let mut mi: MONITORINFO = zeroed();
    mi.cbSize = size_of::<MONITORINFO>() as u32;
    let work = if !mon.is_null() && GetMonitorInfoW(mon, &mut mi) != 0 {
        mi.rcWork
    } else {
        RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        }
    };

    let vertical = app.tip_anchor_vertical;
    let (mut x, mut y) = if vertical {
        (app.tip_anchor.left - w - 8, p.y - h / 2)
    } else {
        (p.x - w / 2, app.tip_anchor.top - h - 8)
    };
    if y < work.top + 4 {
        y = app.tip_anchor.bottom + 8;
    }
    let max_x = (work.right - w).max(work.left);
    let max_y = (work.bottom - h).max(work.top);
    x = x.clamp(work.left, max_x);
    y = y.clamp(work.top, max_y);

    SetWindowPos(
        app.tip_hwnd,
        HWND_TOPMOST,
        x,
        y,
        w,
        h,
        SWP_NOACTIVATE | SWP_SHOWWINDOW,
    );
    InvalidateRect(app.tip_hwnd, null(), 0);
}

unsafe extern "system" fn wndproc_tip(hwnd: HWND, msg: u32, w: usize, l: isize) -> LRESULT {
    match msg {
        WM_NCCREATE => {
            let cs = &*(l as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
            DefWindowProcW(hwnd, msg, w, l)
        }
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            let app = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
            if !app.is_null() {
                if let Some(m) = (*app).tip_metric {
                    if let Some(fonts) = (*app).fonts.as_ref() {
                        let f = frame_fit(&*app, Some(m), None, false, 0, false, false, 0);
                        ui::paint_tip(hwnd, &f, fonts, &(*app).palette);
                    }
                }
            }
            0
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}