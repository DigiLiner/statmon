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
    GetMonitorInfoW, InvalidateRect, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Registry::{
    RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ,
};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::HiDpi::{
    GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use config::{Config, Graph, Palette};
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
        hIcon: LoadIconW(null_mut(), IDI_APPLICATION),
        hCursor: LoadCursorW(null_mut(), IDC_ARROW),
        hbrBackground: null_mut(),
        lpszMenuName: null(),
        lpszClassName: cls.as_ptr(),
    };
    RegisterClassW(&wc);
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
    }));
    let mut ex = WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;
    match kind {
        Kind::Taskbar => ex |= WS_EX_TOPMOST,
        Kind::Float(_) => {
            if (*app).cfg.float_topmost {
                ex |= WS_EX_TOPMOST;
            }
            if (*app).cfg.float_transparent {
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
    if let Kind::Float(_) = kind {
        if (*app).cfg.float_transparent {
            SetLayeredWindowAttributes(hwnd, ui::rgb(ui::KEY), 0, LWA_COLORKEY);
        }
    }
    hwnd
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
        self.hidden = false;
        sync_floats(self);
        SetTimer(self.taskbar_hwnd, TIMER_ID, self.cfg.refresh_ms, None);
        refresh(self);
    }

    fn set_float(&mut self, m: Metric, on: bool) {
        self.cfg.float_mut(m).on = on;
        self.save();
    }
}

unsafe fn frame<'a>(app: &'a App, metric: Option<Metric>) -> Frame<'a> {
    Frame {
        metrics: &app.metrics,
        drives: &app.drives,
        history: &app.history,
        cfg: &app.cfg,
        palette: &app.palette,
        metric,
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
    let f = frame(app, None);
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
        let fc = app.cfg.float(m);
        let f = frame(app, Some(m));
        let width = ui::width(h, &f, fonts);
        let level = if app.cfg.float_topmost {
            HWND_TOPMOST
        } else {
            HWND_NOTOPMOST
        };
        SetWindowPos(
            h,
            level,
            fc.x,
            fc.y,
            width,
            height,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
        InvalidateRect(h, null(), 0);
    }
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
    let mut ex = GetWindowLongPtrW(h, GWL_EXSTYLE) as usize;
    ex &= !(WS_EX_LAYERED as usize);
    if app.cfg.float_transparent {
        ex |= WS_EX_LAYERED as usize;
    }
    SetWindowLongPtrW(h, GWL_EXSTYLE, ex as isize);
    if app.cfg.float_transparent {
        SetLayeredWindowAttributes(h, ui::rgb(ui::KEY), 0, LWA_COLORKEY);
    }
    let level = if app.cfg.float_topmost {
        HWND_TOPMOST
    } else {
        HWND_NOTOPMOST
    };
    SetWindowPos(
        h,
        level,
        0,
        0,
        0,
        0,
        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
    );
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
    nid.hIcon = LoadIconW(null_mut(), IDI_APPLICATION);
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
        "CPU {:.0}%  RAM {}% ({:.1}/{:.1} GB)\nNET ↓{}  ↑{}\nDISK {:.0}%  {}",
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

    let mut checked = |on: bool| MF_STRING | if on { MF_CHECKED } else { 0 };

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
    AppendMenuW(menu, MF_POPUP, gm as usize, wide("Grafik").as_ptr());

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
    AppendMenuW(
        fm,
        checked(a.cfg.float_transparent),
        ID_TRANS,
        wide("Şeffaf").as_ptr(),
    );
    AppendMenuW(
        fm,
        checked(a.cfg.float_topmost),
        ID_TOP,
        wide("Her zaman üstte").as_ptr(),
    );
    AppendMenuW(menu, MF_POPUP, fm as usize, wide("Yüzen widget").as_ptr());

    if let Some(m) = metric {
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        let idx = Metric::ALL.iter().position(|x| *x == m).unwrap_or(0);
        AppendMenuW(
            menu,
            MF_STRING,
            ID_FLOAT_CLOSE + idx,
            wide("Bu widget'ı kapat").as_ptr(),
        );
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
        settings::open(app);
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
        a.cfg.float_transparent = !a.cfg.float_transparent;
        a.save();
        sync_floats(a);
        refresh(a);
        return;
    }
    if id == ID_TOP {
        a.cfg.float_topmost = !a.cfg.float_topmost;
        a.save();
        sync_floats(a);
        refresh(a);
        return;
    }
    if (ID_GRAPH_BASE..ID_GRAPH_BASE + 4).contains(&id) {
        a.cfg.graph = Graph::from_index(id - ID_GRAPH_BASE);
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
        DestroyWindow(app.settings_hwnd);
        app.settings_hwnd = null_mut();
    }
    let list: Vec<HWND> = app.floats.drain(..).map(|(_, h)| h).collect();
    for h in list {
        DestroyWindow(h);
    }
    let tb = app.taskbar_hwnd;
    app.taskbar_hwnd = null_mut();
    if !tb.is_null() {
        DestroyWindow(tb);
    }
}

unsafe fn autostart_enabled() -> bool {
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

unsafe fn set_autostart(on: bool) {
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
                                let f = frame(app, None);
                                ui::paint(hwnd, &f, fonts, false);
                            }
                            Kind::Float(m) => {
                                let f = frame(app, Some(m));
                                ui::paint(hwnd, &f, fonts, app.cfg.float_transparent);
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
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        WM_RBUTTONUP | WM_CONTEXTMENU => {
            let ctx = ctx_ptr(hwnd);
            if let Some(ctx) = ctx.as_ref() {
                let metric = match ctx.kind {
                    Kind::Float(m) => Some(m),
                    _ => None,
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
                    let dx = p.x - d.start.x;
                    let dy = p.y - d.start.y;
                    SetWindowPos(
                        hwnd,
                        null_mut(),
                        d.origin.x + dx,
                        d.origin.y + dy,
                        0,
                        0,
                        SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                    );
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
                    let a = &mut *app;
                    match kind {
                        Kind::Taskbar => {
                            let mut nid: NOTIFYICONDATAW = zeroed();
                            nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
                            nid.hWnd = hwnd;
                            nid.uID = TRAY_ID;
                            Shell_NotifyIconW(NIM_DELETE, &nid);
                            KillTimer(hwnd, TIMER_ID);
                            PostQuitMessage(0);
                        }
                        Kind::Float(m) => {
                            a.floats.retain(|(x, _)| *x != m);
                        }
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
