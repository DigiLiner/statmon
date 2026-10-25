#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod history;
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
use history::History;
use metrics::{DriveInfo, Metric, Metrics};
use ui::{Fonts, Frame};

const TIMER_ID: usize = 1;
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
    let palette = config::palette(cfg.theme);
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
        self.palette = config::palette(self.cfg.theme);
        if let Some(f) = self.fonts.take() {
            f.destroy();
        }
        self.dpi = 0;
        sync_floats(self);
        apply_bar_style(self);
        SetTimer(self.taskbar_hwnd, TIMER_ID, self.cfg.refresh_ms, None);
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
    ui::tile_at(hwnd, &f, fonts, p.x)
}

unsafe fn frame<'a>(app: &'a App, metric: Option<Metric>, hover: Option<Metric>) -> Frame<'a> {
    Frame {
        metrics: &app.metrics,
        drives: &app.drives,
        history: &app.history,
        cfg: &app.cfg,
        palette: &app.palette,
        metric,
        dpi: app.dpi,
        hover,
    }
}

unsafe fn refresh(app: &mut App) {
    app.metrics.tick();

    if app.last_capacity.elapsed().as_secs() >= app.cfg.capacity_secs as u64 {
        app.drives = metrics::capacity(&app.cfg.drives);
        app.last_capacity = Instant::now();
    }
    app.history.push(&app.metrics, &app.drives);

    ensure_fonts(app);
    place_taskbar(app);
    update_floats(app);

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

    let horizontal = (tb.right - tb.left) >= (tb.bottom - tb.top);
    let off_screen = if horizontal {
        mon_ok && (tb.top >= mi.rcMonitor.bottom - 1 || tb.bottom <= mi.rcMonitor.top + 1)
    } else {
        mon_ok && (tb.left >= mi.rcMonitor.right - 1 || tb.right <= mi.rcMonitor.left + 1)
    };
    if off_screen || IsWindowVisible(FindWindowW(wide("Shell_TrayWnd").as_ptr(), null())) == 0 {
        set_hidden(app, true);
        return;
    }
    set_hidden(app, false);

    let Some(fonts) = app.fonts.as_ref() else {
        return;
    };
    let f = frame(app, None, None);
    let width = ui::width(app.taskbar_hwnd, &f, fonts);
    let th = tb.bottom - tb.top;
    let height = (th - VPAD * 2).max(16);

    let (x, y) = if horizontal {
        let limit = if tray.left > tb.left + 8 && tray.left < tb.right {
            tray.left
        } else {
            tb.right
        };
        (
            (limit - width - TASKBAR_GAP).max(tb.left + 4),
            tb.top + (th - height) / 2,
        )
    } else {
        let limit = if tray.top > tb.top + 8 && tray.top < tb.bottom {
            tray.top
        } else {
            tb.bottom
        };
        (
            tb.left + ((tb.right - tb.left) - width) / 2,
            limit - height - TASKBAR_GAP,
        )
    };

    SetWindowPos(
        app.taskbar_hwnd,
        HWND_TOPMOST,
        x,
        y,
        width,
        height,
        SWP_NOACTIVATE | SWP_SHOWWINDOW,
    );
    InvalidateRect(app.taskbar_hwnd, null(), 0);
}

unsafe fn update_floats(app: &mut App) {
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
        let level = if topmost {
            HWND_TOPMOST
        } else {
            HWND_NOTOPMOST
        };
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
            wide("Bu metrik için ayarlar...").as_ptr(),
        );

        let mm = CreatePopupMenu();
        for v in ViewMode::ALL {
            AppendMenuW(
                mm,
                checked(a.cfg.view(m) == v),
                ID_METRIC_MODE_BASE + idx * 3 + v.index(),
                wide(v.label()).as_ptr(),
            );
        }
        AppendMenuW(
            menu,
            MF_POPUP,
            mm as usize,
            wide("Bu metriğin görünümü").as_ptr(),
        );

        let mg = CreatePopupMenu();
        for g in Graph::ALL {
            AppendMenuW(
                mg,
                checked(a.cfg.graph_for(m) == g),
                ID_METRIC_GRAPH_BASE + idx * 4 + g.index(),
                wide(g.label()).as_ptr(),
            );
        }
        AppendMenuW(
            menu,
            MF_POPUP,
            mg as usize,
            wide("Bu metriğin grafiği").as_ptr(),
        );
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
    }

    AppendMenuW(menu, MF_STRING, ID_SETTINGS, wide("Ayarlar...").as_ptr());
    AppendMenuW(menu, MF_SEPARATOR, 0, null());

    let gm = CreatePopupMenu();
    for g in Graph::ALL {
        AppendMenuW(
            gm,
            checked(a.cfg.graph == g),
            ID_GRAPH_BASE + g.index(),
            wide(g.label()).as_ptr(),
        );
    }
    AppendMenuW(menu, MF_POPUP, gm as usize, wide("Grafik türü (tümü)").as_ptr());

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
        wide("Şeffaf (tümü)").as_ptr(),
    );
    AppendMenuW(
        fm,
        checked(all_top),
        ID_TOP,
        wide("Her zaman üstte (tümü)").as_ptr(),
    );
    AppendMenuW(menu, MF_POPUP, fm as usize, wide("Yüzen widget").as_ptr());

    let vm = CreatePopupMenu();
    for (i, v) in ViewMode::ALL.iter().enumerate() {
        AppendMenuW(
            vm,
            MF_STRING,
            ID_VIEW_ALL_BASE + i,
            wide(v.label()).as_ptr(),
        );
    }
    AppendMenuW(menu, MF_POPUP, vm as usize, wide("Tümü için görünüm").as_ptr());

    AppendMenuW(
        menu,
        checked(a.cfg.compact),
        ID_COMPACT,
        wide("Sıkı yerleşim").as_ptr(),
    );

    if let Some(m) = metric {
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        let idx = Metric::ALL.iter().position(|x| *x == m).unwrap_or(0);
        if a.cfg.float(m).on {
            AppendMenuW(
                menu,
                MF_STRING,
                ID_FLOAT_CLOSE + idx,
                wide("Bu widget'ı kapat").as_ptr(),
            );
        }
    }

    AppendMenuW(menu, MF_SEPARATOR, 0, null());
    AppendMenuW(menu, MF_STRING, ID_REFRESH, wide("Yenile").as_ptr());
    AppendMenuW(
        menu,
        checked(autostart_enabled()),
        ID_AUTOSTART,
        wide("Başlangıçta çalıştır").as_ptr(),
    );
    AppendMenuW(menu, MF_SEPARATOR, 0, null());
    AppendMenuW(menu, MF_STRING, ID_EXIT, wide("Çıkış").as_ptr());

    let mut pt: POINT = zeroed();
    GetCursorPos(&mut pt);
    SetForegroundWindow(hwnd);
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
                        refresh(app);
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
                    app.palette = config::palette(app.cfg.theme);
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
