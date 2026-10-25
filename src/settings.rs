use std::ffi::c_void;
use std::mem::zeroed;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateRoundRectRgn, CreateSolidBrush, DeleteObject, DrawTextW,
    EndPaint, FillRect, FillRgn, GetStockObject, InvalidateRect, SetBkColor, SetTextColor,
    ScreenToClient, SelectObject, SetBkMode, SetWindowRgn, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET,
    DEFAULT_PITCH, FF_DONTCARE, FW_SEMIBOLD, HBRUSH, HFONT, OUT_DEFAULT_PRECIS, PAINTSTRUCT, Polyline,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{SetWindowTheme, DRAWITEMSTRUCT};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::config::{
    hex_color, is_dark, parse_color, Config, FloatCfg, Graph, Palette, Theme, ViewMode,
};
use crate::lang::{t, K, Lang};
use crate::metrics::Metric;
use crate::ui::rgb;
use crate::{app_icon, autostart_enabled, set_autostart, App};

const ID_REFRESH: usize = 200;
const ID_CAPACITY: usize = 201;
const ID_THEME: usize = 202;
const ID_LANG: usize = 203;
const ID_FONT_LABEL: usize = 204;
const ID_FONT_VALUE: usize = 206;
const ID_GRAPH_W: usize = 207;
const ID_FLOAT_FONT: usize = 208;
const ID_SHOW_BASE: usize = 210;
const ID_FLOAT_BASE: usize = 220;
const ID_TRANS: usize = 230;
const ID_TOP: usize = 231;
const ID_COMPACT: usize = 232;
const ID_SAVE: usize = 240;
const ID_CLOSE: usize = 241;
const ID_STATUS: usize = 242;
const ID_MODE_BASE: usize = 250;
const ID_WIDGET_SEL: usize = 260;
const ID_WIDGET_BG: usize = 261;
const ID_WIDGET_OPACITY: usize = 262;
const ID_WIDGET_BORDER: usize = 263;
const ID_GRAPH_METRIC_BASE: usize = 270;
const ID_BAR_BG: usize = 280;
const ID_BAR_OPACITY: usize = 281;
const ID_BAR_TRANS: usize = 282;
const ID_AUTOSTART: usize = 283;
const ID_COLOR_BASE: usize = 300;
const ID_RESET_COLORS: usize = 320;
const ID_SEC_GENERAL: usize = 330;
const ID_SEC_BAR: usize = 331;
const ID_SEC_COLORS: usize = 332;
const ID_SEC_METRICS: usize = 333;
const ID_SEC_FLOAT: usize = 334;

const CLIENT_W: i32 = 760;
const CLIENT_H: i32 = 540;
const HEADER_H: i32 = 44;

const EN_CHANGE: usize = 0x0300;
const EN_KILLFOCUS: usize = 0x0200;
const WM_SET_TARGET: u32 = WM_APP + 2;

const COLOR_KEYS: [&str; 7] = ["label", "warn", "cpu", "ram", "net", "disk", "cap"];

struct Ctx {
    app: *mut App,
    edit: Metric,
    ready: bool,
    loading: bool,
    checks: u64,
    palette: Palette,
    bg_brush: HBRUSH,
    edit_brush: HBRUSH,
    title_font: HFONT,
    section_font: HFONT,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn wide_n(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

unsafe fn make_font(px: i32, weight: u32) -> HFONT {
    let face = wide("Segoe UI");
    CreateFontW(
        -px,
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
        5u32,
        (DEFAULT_PITCH | FF_DONTCARE) as u32,
        face.as_ptr(),
    )
}

unsafe fn apply_window_theme(hwnd: HWND, ctx: &mut Ctx) {
    let p = (*ctx.app).palette;
    ctx.palette = p;
    if !ctx.bg_brush.is_null() {
        DeleteObject(ctx.bg_brush);
    }
    if !ctx.edit_brush.is_null() {
        DeleteObject(ctx.edit_brush);
    }
    ctx.bg_brush = CreateSolidBrush(rgb(p.bg));
    ctx.edit_brush = CreateSolidBrush(rgb(p.surface));

    let dark = if is_dark(&p) { 1i32 } else { 0i32 };
    DwmSetWindowAttribute(
        hwnd,
        20,
        &dark as *const i32 as *const c_void,
        std::mem::size_of::<i32>() as u32,
    );
    let theme = if dark != 0 {
        wide("DarkMode_Explorer")
    } else {
        wide("")
    };
    SetWindowTheme(hwnd, theme.as_ptr(), null());
    EnumChildWindows(hwnd, Some(theme_child), theme.as_ptr() as isize);
    InvalidateRect(hwnd, null(), 0);
}

unsafe extern "system" fn theme_child(child: HWND, lp: isize) -> BOOL {
    SetWindowTheme(child, lp as *const u16, null());
    1
}

unsafe fn mk(
    parent: HWND,
    class: &str,
    text: &str,
    style: u32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    id: usize,
    font: HFONT,
) -> HWND {
    let hwnd = CreateWindowExW(
        0,
        wide(class).as_ptr(),
        wide(text).as_ptr(),
        WS_CHILD | WS_VISIBLE | style,
        x,
        y,
        w,
        h,
        parent,
        id as *mut c_void,
        GetModuleHandleW(null()),
        null(),
    );
    if !hwnd.is_null() && !font.is_null() {
        SendMessageW(hwnd, WM_SETFONT, font as usize, 1);
    }
    hwnd
}

unsafe fn set_text(h: HWND, s: &str) {
    if !h.is_null() {
        SetWindowTextW(h, wide(s).as_ptr());
    }
}

unsafe fn get_text(h: HWND) -> String {
    let mut buf = [0u16; 128];
    let n = GetWindowTextW(h, buf.as_mut_ptr(), buf.len() as i32);
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

unsafe fn get_int(h: HWND, def: i32) -> i32 {
    get_text(h).trim().parse::<i32>().unwrap_or(def)
}

fn bit_of(id: usize) -> Option<u32> {
    if (ID_SHOW_BASE..ID_SHOW_BASE + 5).contains(&id) {
        Some((id - ID_SHOW_BASE) as u32)
    } else if (ID_FLOAT_BASE..ID_FLOAT_BASE + 5).contains(&id) {
        Some(5 + (id - ID_FLOAT_BASE) as u32)
    } else {
        match id {
            ID_TRANS => Some(10),
            ID_TOP => Some(11),
            ID_COMPACT => Some(12),
            ID_WIDGET_BORDER => Some(13),
            ID_AUTOSTART => Some(14),
            ID_BAR_TRANS => Some(15),
            _ => None,
        }
    }
}

fn bit_on(checks: u64, id: usize) -> bool {
    bit_of(id)
        .map(|b| checks & (1u64 << b) != 0)
        .unwrap_or(false)
}

fn bit_set(checks: &mut u64, id: usize, on: bool) {
    if let Some(b) = bit_of(id) {
        if on {
            *checks |= 1u64 << b;
        } else {
            *checks &= !(1u64 << b);
        }
    }
}

unsafe fn redraw_checks(hwnd: HWND) {
    for id in (ID_SHOW_BASE..ID_SHOW_BASE + 5)
        .chain(ID_FLOAT_BASE..ID_FLOAT_BASE + 5)
        .chain([
            ID_TRANS,
            ID_TOP,
            ID_COMPACT,
            ID_WIDGET_BORDER,
            ID_AUTOSTART,
            ID_BAR_TRANS,
        ])
    {
        let h = GetDlgItem(hwnd, id as i32);
        if !h.is_null() {
            InvalidateRect(h, null(), 1);
        }
    }
}

unsafe fn init_checks(hwnd: HWND, ctx: &mut Ctx) {
    let cfg = &(*ctx.app).cfg;
    for m in Metric::ALL {
        let i = Metric::ALL.iter().position(|x| *x == m).unwrap_or(0);
        bit_set(&mut ctx.checks, ID_SHOW_BASE + i, cfg.shows(m));
        bit_set(&mut ctx.checks, ID_FLOAT_BASE + i, cfg.float(m).on);
    }
    let fc = cfg.float(ctx.edit);
    bit_set(&mut ctx.checks, ID_TRANS, fc.transparent);
    bit_set(&mut ctx.checks, ID_TOP, fc.topmost);
    bit_set(&mut ctx.checks, ID_WIDGET_BORDER, fc.border);
    bit_set(&mut ctx.checks, ID_COMPACT, cfg.compact);
    bit_set(&mut ctx.checks, ID_AUTOSTART, autostart_enabled());
    bit_set(&mut ctx.checks, ID_BAR_TRANS, cfg.bar_transparent);
    redraw_checks(hwnd);
}

pub unsafe fn open(app: *mut App, initial: Option<Metric>) {
    if let Some(m) = initial {
        (*app).settings_target = Some(m);
    }
    if !(*app).settings_hwnd.is_null() {
        let h = (*app).settings_hwnd;
        ShowWindow(h, SW_SHOW);
        SetForegroundWindow(h);
        if initial.is_some() {
            PostMessageW(h, WM_SET_TARGET, 0, 0);
        }
        return;
    }

    let hinst = GetModuleHandleW(null());
    let lang = (*app).cfg.language;
    let cls = wide("StatMonSettings");
    let wc = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW | CS_DROPSHADOW,
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

    let mut work: RECT = zeroed();
    SystemParametersInfoW(
        SPI_GETWORKAREA,
        0,
        &mut work as *mut RECT as *mut c_void,
        0,
    );
    let x = work.left + ((work.right - work.left) - CLIENT_W) / 2;
    let y = work.top + ((work.bottom - work.top) - CLIENT_H) / 2;

    let edit = initial.unwrap_or(Metric::Cpu);
    let ctx = Box::into_raw(Box::new(Ctx {
        app,
        edit,
        ready: false,
        loading: false,
        checks: 0,
        palette: (*app).palette,
        bg_brush: null_mut(),
        edit_brush: null_mut(),
        title_font: null_mut(),
        section_font: null_mut(),
    }));
    let hwnd = CreateWindowExW(
        0,
        cls.as_ptr(),
        wide(t(lang, K::Title)).as_ptr(),
        WS_POPUP,
        x,
        y,
        CLIENT_W,
        CLIENT_H,
        null_mut(),
        null_mut(),
        hinst,
        ctx as *const c_void,
    );
    if hwnd.is_null() {
        drop(Box::from_raw(ctx));
        return;
    }
    (*app).settings_hwnd = hwnd;
    (*app).settings_target = None;
    SetWindowRgn(
        hwnd,
        CreateRoundRectRgn(0, 0, CLIENT_W + 1, CLIENT_H + 1, 14, 14),
        1,
    );
    ShowWindow(hwnd, SW_SHOW);
    SetForegroundWindow(hwnd);
}

unsafe fn load_widget(hwnd: HWND, checks: &mut u64, fc: &FloatCfg) {
    set_text(GetDlgItem(hwnd, ID_WIDGET_BG as i32), &fc.bg);
    set_text(
        GetDlgItem(hwnd, ID_WIDGET_OPACITY as i32),
        &fc.opacity.to_string(),
    );
    bit_set(checks, ID_TRANS, fc.transparent);
    bit_set(checks, ID_TOP, fc.topmost);
    bit_set(checks, ID_WIDGET_BORDER, fc.border);
    redraw_checks(hwnd);
}

unsafe fn read_widget(hwnd: HWND, cfg: &mut Config, edit: Metric, checks: &mut u64) {
    let bg = get_text(GetDlgItem(hwnd, ID_WIDGET_BG as i32));
    let opacity = get_int(GetDlgItem(hwnd, ID_WIDGET_OPACITY as i32), 100).clamp(20, 100);
    let changed_bg = parse_color(&bg)
        .map(|c| hex_color(c) != cfg.float(edit).bg)
        .unwrap_or(false);
    let fc = cfg.float_mut(edit);
    if let Some(c) = parse_color(&bg) {
        fc.bg = hex_color(c);
    }
    fc.opacity = opacity as u8;
    fc.transparent = bit_on(*checks, ID_TRANS);
    fc.topmost = bit_on(*checks, ID_TOP);
    fc.border = bit_on(*checks, ID_WIDGET_BORDER);
    if changed_bg && fc.transparent {
        fc.transparent = false;
        bit_set(checks, ID_TRANS, false);
        redraw_checks(hwnd);
    }
}

unsafe fn section(hwnd: HWND, text: &str, x: i32, y: i32, id: usize, font: HFONT) {
    mk(hwnd, "STATIC", text, 0, x, y, 300, 22, id, font);
}

unsafe fn build_controls(hwnd: HWND, app: &App, edit: Metric, ctx_fonts: (HFONT, HFONT)) {
    let font = GetStockObject(17) as HFONT;
    let (_title_font, section_font) = ctx_fonts;
    let cfg = &app.cfg;
    let lang = cfg.language;
    let p = cfg.palette();

    let label = |tx: &str, x: i32, y: i32, w: i32| -> HWND {
        mk(hwnd, "STATIC", tx, 0, x, y, w, 20, 0, font)
    };
    let num = |v: &str, x: i32, y: i32, w: i32, id: usize| -> HWND {
        mk(
            hwnd,
            "EDIT",
            v,
            WS_BORDER | (ES_NUMBER as u32),
            x,
            y,
            w,
            22,
            id,
            font,
        )
    };
    let hex = |v: &str, x: i32, y: i32, w: i32, id: usize| -> HWND {
        mk(hwnd, "EDIT", v, WS_BORDER, x, y, w, 22, id, font)
    };
    let combo = |x: i32, y: i32, w: i32, id: usize| -> HWND {
        mk(
            hwnd,
            "COMBOBOX",
            "",
            (CBS_DROPDOWNLIST as u32) | WS_VSCROLL,
            x,
            y,
            w,
            220,
            id,
            font,
        )
    };
    let checkbox = |tx: &str, x: i32, y: i32, w: i32, id: usize| -> HWND {
        mk(
            hwnd,
            "BUTTON",
            tx,
            BS_OWNERDRAW as u32,
            x,
            y,
            w,
            24,
            id,
            font,
        )
    };

    section(hwnd, t(lang, K::General), 24, 56, ID_SEC_GENERAL, section_font);

    label(t(lang, K::RefreshMs), 24, 84, 90);
    num(&cfg.refresh_ms.to_string(), 120, 82, 60, ID_REFRESH);
    label(t(lang, K::CapacityS), 196, 84, 90);
    num(&cfg.capacity_secs.to_string(), 292, 82, 60, ID_CAPACITY);

    label(t(lang, K::Theme), 24, 112, 50);
    let theme = combo(80, 110, 120, ID_THEME);
    for th in Theme::ALL {
        SendMessageW(theme, CB_ADDSTRING, 0, wide(th.label_l(lang)).as_ptr() as isize);
    }
    SendMessageW(theme, CB_SETCURSEL, cfg.theme.index(), 0);

    label(t(lang, K::Language), 212, 112, 62);
    let langc = combo(278, 110, 104, ID_LANG);
    for l in Lang::ALL {
        SendMessageW(langc, CB_ADDSTRING, 0, wide(l.label()).as_ptr() as isize);
    }
    SendMessageW(langc, CB_SETCURSEL, cfg.language.index(), 0);

    label(t(lang, K::FontLabel), 24, 140, 100);
    num(&cfg.font_label_px.to_string(), 128, 138, 46, ID_FONT_LABEL);
    label(t(lang, K::FontValue), 190, 140, 100);
    num(&cfg.font_value_px.to_string(), 294, 138, 46, ID_FONT_VALUE);

    label(t(lang, K::FloatFont), 24, 168, 100);
    num(&cfg.float_font_px.to_string(), 128, 166, 46, ID_FLOAT_FONT);
    label(t(lang, K::ChartWidth), 190, 168, 100);
    num(&cfg.graph_width.to_string(), 294, 166, 46, ID_GRAPH_W);

    checkbox(t(lang, K::Compact), 24, 194, 140, ID_COMPACT);
    checkbox(t(lang, K::Autostart), 24, 220, 250, ID_AUTOSTART);

    section(hwnd, t(lang, K::BarLook), 24, 250, ID_SEC_BAR, section_font);
    label(t(lang, K::Background), 24, 276, 80);
    hex(&cfg.bar_bg, 110, 274, 90, ID_BAR_BG);
    label(t(lang, K::Opacity), 214, 276, 60);
    num(&cfg.bar_opacity.to_string(), 280, 274, 46, ID_BAR_OPACITY);
    label("%", 330, 276, 14);
    checkbox(t(lang, K::BarTransparent), 24, 302, 190, ID_BAR_TRANS);

    section(hwnd, t(lang, K::Colors), 24, 332, ID_SEC_COLORS, section_font);
    label(t(lang, K::LabelColor), 24, 358, 90);
    hex(&hex_color(p.label), 118, 356, 80, ID_COLOR_BASE);
    label(t(lang, K::WarnColor), 210, 358, 85);
    hex(&hex_color(p.warn), 298, 356, 80, ID_COLOR_BASE + 1);

    let color_rows: [(usize, i32, &str, i32); 5] = [
        (2, 386, "CPU", 24),
        (3, 386, "RAM", 168),
        (4, 414, "NET", 24),
        (5, 414, "DISK", 168),
        (6, 442, "CAP", 24),
    ];
    for (i, y, name, x) in color_rows {
        label(name, x, y + 2, 44);
        let key = COLOR_KEYS[i];
        hex(&hex_color(color_of(&p, key)), x + 44, y, 80, ID_COLOR_BASE + i);
    }

    mk(
        hwnd,
        "BUTTON",
        t(lang, K::ResetColors),
        (BS_PUSHBUTTON as u32) | (BS_OWNERDRAW as u32),
        168,
        440,
        140,
        28,
        ID_RESET_COLORS,
        font,
    );

    section(hwnd, t(lang, K::Metrics), 400, 56, ID_SEC_METRICS, section_font);
    label(t(lang, K::Show), 400, 82, 50);
    label(t(lang, K::View), 456, 82, 40);
    label(t(lang, K::ChartType), 574, 82, 90);
    label(t(lang, K::Floating), 682, 82, 54);

    for (i, m) in Metric::ALL.iter().enumerate() {
        let y = 102 + i as i32 * 30;

        checkbox(m.label(), 400, y, 50, ID_SHOW_BASE + i);

        let mode = combo(452, y - 2, 114, ID_MODE_BASE + i);
        for v in ViewMode::ALL {
            SendMessageW(mode, CB_ADDSTRING, 0, wide(v.label_l(lang)).as_ptr() as isize);
        }
        SendMessageW(mode, CB_SETCURSEL, cfg.view(*m).index(), 0);

        let g = combo(570, y - 2, 108, ID_GRAPH_METRIC_BASE + i);
        for kind in Graph::ALL {
            SendMessageW(g, CB_ADDSTRING, 0, wide(kind.label_l(lang)).as_ptr() as isize);
        }
        SendMessageW(g, CB_SETCURSEL, cfg.graph_for(*m).index(), 0);

        checkbox("", 682, y, 24, ID_FLOAT_BASE + i);
    }

    section(hwnd, t(lang, K::FloatLook), 400, 258, ID_SEC_FLOAT, section_font);
    label(t(lang, K::Widget), 400, 284, 52);
    let sel = combo(456, 282, 90, ID_WIDGET_SEL);
    for m in Metric::ALL {
        SendMessageW(sel, CB_ADDSTRING, 0, wide(m.label()).as_ptr() as isize);
    }
    let idx = Metric::ALL.iter().position(|m| *m == edit).unwrap_or(0);
    SendMessageW(sel, CB_SETCURSEL, idx, 0);
    label(t(lang, K::Background), 556, 284, 76);
    hex(&cfg.float(edit).bg, 636, 282, 90, ID_WIDGET_BG);

    label(t(lang, K::Opacity), 400, 312, 56);
    num(
        &cfg.float(edit).opacity.to_string(),
        460,
        310,
        40,
        ID_WIDGET_OPACITY,
    );
    label("%", 504, 312, 14);
    checkbox(t(lang, K::Transparent), 520, 308, 104, ID_TRANS);
    checkbox(t(lang, K::AlwaysTop), 630, 308, 106, ID_TOP);
    checkbox(t(lang, K::Border), 400, 336, 70, ID_WIDGET_BORDER);
    label(t(lang, K::HintTransparent), 470, 340, 270);

    mk(
        hwnd,
        "BUTTON",
        t(lang, K::Save),
        (BS_PUSHBUTTON as u32) | (BS_OWNERDRAW as u32),
        520,
        490,
        100,
        34,
        ID_SAVE,
        font,
    );
    mk(
        hwnd,
        "BUTTON",
        t(lang, K::Close),
        (BS_PUSHBUTTON as u32) | (BS_OWNERDRAW as u32),
        630,
        490,
        90,
        34,
        ID_CLOSE,
        font,
    );
    mk(hwnd, "STATIC", "", 0, 400, 496, 110, 20, ID_STATUS, font);
}

unsafe fn color_of(p: &Palette, key: &str) -> [u8; 3] {
    match key {
        "label" => p.label,
        "warn" => p.warn,
        "cpu" => p.cpu,
        "ram" => p.ram,
        "net" => p.net,
        "disk" => p.disk,
        "cap" => p.cap,
        _ => p.text,
    }
}

unsafe fn save_from(hwnd: HWND, app: &mut App, edit: Metric, checks: &mut u64) {
    let mut cfg = app.cfg.clone();
    let old = cfg.clone();
    let lang = old.language;

    cfg.refresh_ms = get_int(GetDlgItem(hwnd, ID_REFRESH as i32), old.refresh_ms as i32)
        .clamp(200, 10000) as u32;
    cfg.capacity_secs = get_int(GetDlgItem(hwnd, ID_CAPACITY as i32), old.capacity_secs as i32)
        .clamp(2, 600) as u32;
    cfg.font_label_px = get_int(GetDlgItem(hwnd, ID_FONT_LABEL as i32), old.font_label_px)
        .clamp(8, 40);
    cfg.font_value_px = get_int(GetDlgItem(hwnd, ID_FONT_VALUE as i32), old.font_value_px)
        .clamp(8, 40);
    cfg.float_font_px = get_int(GetDlgItem(hwnd, ID_FLOAT_FONT as i32), old.float_font_px)
        .clamp(8, 48);
    cfg.graph_width = get_int(GetDlgItem(hwnd, ID_GRAPH_W as i32), old.graph_width)
        .clamp(18, 400);
    cfg.compact = bit_on(*checks, ID_COMPACT);

    let tsel = SendMessageW(GetDlgItem(hwnd, ID_THEME as i32), CB_GETCURSEL, 0, 0);
    if tsel >= 0 {
        cfg.theme = Theme::from_index(tsel as usize);
    }
    let lsel = SendMessageW(GetDlgItem(hwnd, ID_LANG as i32), CB_GETCURSEL, 0, 0);
    if lsel >= 0 {
        cfg.language = Lang::from_index(lsel as usize);
    }

    for (i, m) in Metric::ALL.iter().enumerate() {
        cfg.set_show(
            *m,
            bit_on(*checks, ID_SHOW_BASE + i),
        );
        let sel = SendMessageW(GetDlgItem(hwnd, (ID_MODE_BASE + i) as i32), CB_GETCURSEL, 0, 0);
        if sel >= 0 {
            *cfg.view_mut(*m) = ViewMode::from_index(sel as usize);
        }
        let g = SendMessageW(
            GetDlgItem(hwnd, (ID_GRAPH_METRIC_BASE + i) as i32),
            CB_GETCURSEL,
            0,
            0,
        );
        if g >= 0 {
            cfg.set_graph_for(*m, Graph::from_index(g as usize));
        }
        let fc: &mut FloatCfg = cfg.float_mut(*m);
        fc.on = bit_on(*checks, ID_FLOAT_BASE + i);
    }

    let bar_bg = get_text(GetDlgItem(hwnd, ID_BAR_BG as i32));
    if let Some(c) = parse_color(&bar_bg) {
        cfg.bar_bg = hex_color(c);
    }
    cfg.bar_opacity =
        get_int(GetDlgItem(hwnd, ID_BAR_OPACITY as i32), cfg.bar_opacity as i32)
            .clamp(20, 100) as u8;
    cfg.bar_transparent = bit_on(*checks, ID_BAR_TRANS);

    for (i, _) in COLOR_KEYS.iter().enumerate() {
        let h = GetDlgItem(hwnd, (ID_COLOR_BASE + i) as i32);
        let s = get_text(h);
        if let Some(c) = parse_color(&s) {
            cfg.colors.set(COLOR_KEYS[i], hex_color(c));
        }
    }

    read_widget(hwnd, &mut cfg, edit, checks);

    let want_auto = bit_on(*checks, ID_AUTOSTART);
    if want_auto != autostart_enabled() {
        set_autostart(want_auto);
    }

    app.apply_config(cfg);
    set_text(GetDlgItem(hwnd, ID_STATUS as i32), t(lang, K::Applied));
}

unsafe fn apply_widget_only(hwnd: HWND, app: &mut App, edit: Metric, checks: &mut u64) {
    let mut cfg = app.cfg.clone();
    read_widget(hwnd, &mut cfg, edit, checks);
    app.cfg = cfg;
    app.save();
    app.restyle();
}

unsafe fn apply_bar_only(hwnd: HWND, app: &mut App, checks: &mut u64) {
    let mut cfg = app.cfg.clone();
    let bg = get_text(GetDlgItem(hwnd, ID_BAR_BG as i32));
    if let Some(c) = parse_color(&bg) {
        cfg.bar_bg = hex_color(c);
    }
    cfg.bar_opacity =
        get_int(GetDlgItem(hwnd, ID_BAR_OPACITY as i32), cfg.bar_opacity as i32)
            .clamp(20, 100) as u8;
    cfg.bar_transparent = bit_on(*checks, ID_BAR_TRANS);
    app.cfg = cfg;
    app.save();
    crate::apply_bar_style(&*app);
}

unsafe fn draw_item(dis: &DRAWITEMSTRUCT, ctx: &Ctx) {
    let id = dis.CtlID as usize;
    let r = dis.rcItem;
    let hdc = dis.hDC;
    let p = ctx.palette;
    SetBkMode(hdc, 1);
    if id == ID_SAVE || id == ID_CLOSE || id == ID_RESET_COLORS {
        let bg = if id == ID_SAVE { p.accent } else { p.surface };
        let br = CreateSolidBrush(rgb(bg));
        let rgn = CreateRoundRectRgn(r.left, r.top, r.right + 1, r.bottom + 1, 10, 10);
        FillRgn(hdc, rgn, br);
        DeleteObject(rgn);
        DeleteObject(br);
        let mut text = [0u16; 128];
        let n = GetWindowTextW(dis.hwndItem, text.as_mut_ptr(), text.len() as i32);
        let col = if id == ID_SAVE {
            [255u8, 255u8, 255u8]
        } else {
            p.text
        };
        SetTextColor(hdc, rgb(col));
        let mut rc = r;
        DrawTextW(
            hdc,
            text.as_ptr(),
            n,
            &mut rc,
            0x0001 | 0x0004 | 0x0020,
        );
    } else {
        let on = bit_of(id).map(|b| ctx.checks & (1u64 << b) != 0).unwrap_or(false);
        let size = 16;
        let bx = r.left + 1;
        let by = r.top + (r.bottom - r.top - size) / 2;
        let br = CreateSolidBrush(rgb(if on { p.accent } else { p.surface }));
        let rgn = CreateRoundRectRgn(bx, by, bx + size + 1, by + size + 1, 6, 6);
        FillRgn(hdc, rgn, br);
        DeleteObject(rgn);
        DeleteObject(br);
        let edge = CreateSolidBrush(rgb(p.divider));
        let r1 = RECT {
            left: bx,
            top: by,
            right: bx + size,
            bottom: by + 1,
        };
        FillRect(hdc, &r1, edge);
        DeleteObject(edge);
        if on {
            let pen = create_pen_check(rgb([255, 255, 255]));
            let old = SelectObject(hdc, pen);
            let pts = [
                POINT { x: bx + 4, y: by + 8 },
                POINT { x: bx + 7, y: by + 11 },
                POINT { x: bx + 12, y: by + 5 },
            ];
            Polyline(hdc, pts.as_ptr(), 3);
            SelectObject(hdc, old);
            DeleteObject(pen);
        }
        let mut text = [0u16; 128];
        let n = GetWindowTextW(dis.hwndItem, text.as_mut_ptr(), text.len() as i32);
        if n > 0 {
            SetTextColor(hdc, rgb(p.text));
            let mut rc = RECT {
                left: r.left + size + 8,
                top: r.top,
                right: r.right,
                bottom: r.bottom,
            };
            DrawTextW(
                hdc,
                text.as_ptr(),
                n,
                &mut rc,
                0x0004 | 0x0020 | 0x8000,
            );
        }
    }
}

unsafe fn create_pen_check(color: u32) -> windows_sys::Win32::Graphics::Gdi::HPEN {
    windows_sys::Win32::Graphics::Gdi::CreatePen(0, 2, color)
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, w: usize, l: isize) -> LRESULT {
    match msg {
        WM_NCCREATE => {
            let cs = &*(l as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
            DefWindowProcW(hwnd, msg, w, l)
        }
        WM_CREATE => {
            let ctx = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Ctx;
            if !ctx.is_null() {
                let title = make_font(20, FW_SEMIBOLD);
                let sect = make_font(16, FW_SEMIBOLD);
                (*ctx).title_font = title;
                (*ctx).section_font = sect;
                build_controls(hwnd, &*(*ctx).app, (*ctx).edit, (title, sect));
                init_checks(hwnd, &mut *ctx);
                apply_window_theme(hwnd, &mut *ctx);
                (*ctx).ready = true;
            }
            0
        }
        WM_SET_TARGET => {
            let ctx = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Ctx;
            if !ctx.is_null() {
                if let Some(m) = (*ctx).app.as_mut().and_then(|a| a.settings_target.take()) {
                    (*ctx).edit = m;
                    let idx = Metric::ALL.iter().position(|x| *x == m).unwrap_or(0);
                    (*ctx).loading = true;
                    SendMessageW(
                        GetDlgItem(hwnd, ID_WIDGET_SEL as i32),
                        CB_SETCURSEL,
                        idx,
                        0,
                    );
                    if let Some(app) = (*ctx).app.as_ref() {
                        load_widget(hwnd, &mut (*ctx).checks, app.cfg.float(m));
                    }
                    (*ctx).loading = false;
                }
            }
            0
        }
        WM_ERASEBKGND => {
            let ctx = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Ctx;
            if !ctx.is_null() && !(*ctx).bg_brush.is_null() {
                let mut rc: RECT = zeroed();
                GetClientRect(hwnd, &mut rc);
                FillRect(w as *mut c_void, &rc, (*ctx).bg_brush);
                return 1;
            }
            DefWindowProcW(hwnd, msg, w, l)
        }
        WM_PAINT => {
            let ctx = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Ctx;
            let mut ps: PAINTSTRUCT = zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            if !ctx.is_null() {
                let p = (*ctx).palette;
                let mut rc: RECT = zeroed();
                GetClientRect(hwnd, &mut rc);
                let bg = CreateSolidBrush(rgb(p.bg));
                FillRect(hdc, &rc, bg);
                DeleteObject(bg);

                let head = RECT {
                    left: 0,
                    top: 0,
                    right: rc.right,
                    bottom: HEADER_H,
                };
                let hb = CreateSolidBrush(rgb(p.surface));
                FillRect(hdc, &head, hb);
                DeleteObject(hb);

                let lang = (*ctx).app.as_ref().map(|a| a.cfg.language).unwrap_or(Lang::En);
                SetBkMode(hdc, 1);
                SetTextColor(hdc, rgb(p.text));
                SelectObject(hdc, (*ctx).title_font);
                let mut tr = RECT {
                    left: 24,
                    top: 0,
                    right: rc.right - 56,
                    bottom: HEADER_H,
                };
                let title = wide_n(t(lang, K::Title));
                DrawTextW(
                    hdc,
                    title.as_ptr(),
                    title.len() as i32,
                    &mut tr,
                    0x0001 | 0x0004 | 0x0020,
                );

                SetTextColor(hdc, rgb(p.label));
                SelectObject(hdc, GetStockObject(17) as HFONT);
                let close = wide_n("\u{00D7}");
                let mut cr = RECT {
                    left: rc.right - 44,
                    top: 0,
                    right: rc.right - 8,
                    bottom: HEADER_H,
                };
                DrawTextW(
                    hdc,
                    close.as_ptr(),
                    close.len() as i32,
                    &mut cr,
                    0x0001 | 0x0004 | 0x0020,
                );
            }
            EndPaint(hwnd, &mut ps);
            0
        }
        WM_NCHITTEST => {
            let mut rc: RECT = zeroed();
            GetClientRect(hwnd, &mut rc);
            let x = (l & 0xFFFF) as i16 as i32;
            let y = ((l >> 16) & 0xFFFF) as i16 as i32;
            let mut pt = POINT { x, y };
            ScreenToClient(hwnd, &mut pt);
            if pt.y < HEADER_H {
                if pt.x > rc.right - 52 {
                    return HTCLIENT as LRESULT;
                }
                return HTCAPTION as LRESULT;
            }
            HTCLIENT as LRESULT
        }
        WM_LBUTTONUP => {
            let mut rc: RECT = zeroed();
            GetClientRect(hwnd, &mut rc);
            let x = (l & 0xFFFF) as i16 as i32;
            let y = ((l >> 16) & 0xFFFF) as i16 as i32;
            if y < HEADER_H && x > rc.right - 52 {
                DestroyWindow(hwnd);
            }
            0
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => {
            let ctx = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Ctx;
            if !ctx.is_null() {
                let hdc = w as *mut c_void;
                let child = l as HWND;
                let cid = GetDlgCtrlID(child) as usize;
                let color = if [
                    ID_SEC_GENERAL,
                    ID_SEC_BAR,
                    ID_SEC_COLORS,
                    ID_SEC_METRICS,
                    ID_SEC_FLOAT,
                ]
                .contains(&cid)
                {
                    (*ctx).palette.accent
                } else {
                    (*ctx).palette.text
                };
                SetTextColor(hdc, rgb(color));
                SetBkColor(hdc, rgb((*ctx).palette.bg));
                return (*ctx).bg_brush as LRESULT;
            }
            0
        }
        WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => {
            let ctx = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Ctx;
            if !ctx.is_null() {
                let hdc = w as *mut c_void;
                SetTextColor(hdc, rgb((*ctx).palette.text));
                SetBkColor(hdc, rgb((*ctx).palette.surface));
                return (*ctx).edit_brush as LRESULT;
            }
            0
        }
        WM_DRAWITEM => {
            let ctx = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Ctx;
            if !ctx.is_null() {
                let dis = &*(l as *const DRAWITEMSTRUCT);
                draw_item(dis, &*ctx);
            }
            1
        }
        WM_COMMAND => {
            let id = w & 0xFFFF;
            let code = (w >> 16) & 0xFFFF;
            let ctx = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Ctx;
            if !ctx.is_null() && (*ctx).ready && !(*ctx).loading {
                let app = (*ctx).app;
                if code == 0 {
                    if let Some(b) = bit_of(id) {
                        let now = (*ctx).checks & (1u64 << b) != 0;
                        bit_set(&mut (*ctx).checks, id, !now);
                        let h = GetDlgItem(hwnd, id as i32);
                        if !h.is_null() {
                            InvalidateRect(h, null(), 1);
                        }
                    }
                }
                match id {
                    ID_SAVE => {
                        save_from(hwnd, &mut *app, (*ctx).edit, &mut (*ctx).checks);
                        apply_window_theme(hwnd, &mut *ctx);
                    }
                    ID_CLOSE => {
                        DestroyWindow(hwnd);
                    }
                    ID_RESET_COLORS => {
                        (*app).cfg.colors = Default::default();
                        (*app).apply_config((*app).cfg.clone());
                        let edit = (*ctx).edit;
                        DestroyWindow(hwnd);
                        open(app, Some(edit));
                    }
                    ID_LANG => {
                        if code == 1 {
                            let sel = SendMessageW(
                                GetDlgItem(hwnd, ID_LANG as i32),
                                CB_GETCURSEL,
                                0,
                                0,
                            );
                            if sel >= 0 {
                                let lang = Lang::from_index(sel as usize);
                                if lang != (*app).cfg.language {
                                    (*app).cfg.language = lang;
                                    (*app).save();
                                    let edit = (*ctx).edit;
                                    DestroyWindow(hwnd);
                                    open(app, Some(edit));
                                }
                            }
                        }
                    }
                    ID_WIDGET_SEL => {
                        if code == 1 {
                            let sel = SendMessageW(
                                GetDlgItem(hwnd, ID_WIDGET_SEL as i32),
                                CB_GETCURSEL,
                                0,
                                0,
                            );
                            if sel >= 0 {
                                (*ctx).edit = Metric::ALL[sel as usize];
                                (*ctx).loading = true;
                                if let Some(a) = (*ctx).app.as_ref() {
                                    load_widget(hwnd, &mut (*ctx).checks, a.cfg.float((*ctx).edit));
                                }
                                (*ctx).loading = false;
                            }
                        }
                    }
                    ID_WIDGET_BG | ID_WIDGET_OPACITY => {
                        if code == EN_CHANGE || code == EN_KILLFOCUS {
                            apply_widget_only(hwnd, &mut *app, (*ctx).edit, &mut (*ctx).checks);
                        }
                    }
                    ID_TRANS | ID_TOP | ID_WIDGET_BORDER => {
                        apply_widget_only(hwnd, &mut *app, (*ctx).edit, &mut (*ctx).checks);
                    }
                    ID_BAR_BG | ID_BAR_OPACITY | ID_BAR_TRANS => {
                        apply_bar_only(hwnd, &mut *app, &mut (*ctx).checks);
                    }
                    ID_AUTOSTART => {
                        let on = bit_on((*ctx).checks, ID_AUTOSTART);
                        set_autostart(on);
                    }
                    _ => {
                        let live = id == ID_COMPACT
                            || id == ID_THEME
                            || (ID_SHOW_BASE..ID_SHOW_BASE + 5).contains(&id)
                            || (ID_FLOAT_BASE..ID_FLOAT_BASE + 5).contains(&id)
                            || (ID_MODE_BASE..ID_MODE_BASE + 5).contains(&id)
                            || (ID_GRAPH_METRIC_BASE..ID_GRAPH_METRIC_BASE + 5).contains(&id)
                            || (ID_COLOR_BASE..ID_COLOR_BASE + 7).contains(&id);
                        if live {
                            save_from(hwnd, &mut *app, (*ctx).edit, &mut (*ctx).checks);
                            apply_window_theme(hwnd, &mut *ctx);
                        }
                    }
                }
            }
            0
        }
        WM_CLOSE => {
            DestroyWindow(hwnd);
            0
        }
        WM_NCDESTROY => {
            let ctx = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Ctx;
            if !ctx.is_null() {
                let app = (*ctx).app;
                if (*app).settings_hwnd == hwnd {
                    (*app).settings_hwnd = null_mut();
                }
                if !(*ctx).bg_brush.is_null() {
                    DeleteObject((*ctx).bg_brush);
                }
                if !(*ctx).edit_brush.is_null() {
                    DeleteObject((*ctx).edit_brush);
                }
                if !(*ctx).title_font.is_null() {
                    DeleteObject((*ctx).title_font);
                }
                if !(*ctx).section_font.is_null() {
                    DeleteObject((*ctx).section_font);
                }
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(ctx));
            }
            DefWindowProcW(hwnd, msg, w, l)
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}

