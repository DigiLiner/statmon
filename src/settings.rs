use std::ffi::c_void;
use std::mem::zeroed;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
use windows_sys::Win32::Graphics::Gdi::{
    CreateSolidBrush, DeleteObject, FillRect, GetStockObject, InvalidateRect, SetBkColor,
    SetTextColor, HBRUSH, HFONT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{
    SetWindowTheme, BST_CHECKED, BST_UNCHECKED,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::config::{
    hex_color, is_dark, parse_color, Config, FloatCfg, Graph, Palette, Theme, ViewMode,
};
use crate::metrics::Metric;
use crate::ui::rgb;
use crate::{app_icon, autostart_enabled, set_autostart, App};

const ID_REFRESH: usize = 200;
const ID_CAPACITY: usize = 201;
const ID_THEME: usize = 202;
const ID_FONT_LABEL: usize = 203;
const ID_FONT_VALUE: usize = 204;
const ID_GRAPH_W: usize = 206;
const ID_FLOAT_FONT: usize = 207;
const ID_SHOW_BASE: usize = 210;
const ID_FLOAT_BASE: usize = 220;
const ID_TRANS: usize = 230;
const ID_TOP: usize = 231;
const ID_COMPACT: usize = 232;
const ID_SAVE: usize = 240;
const ID_CLOSE: usize = 241;
const ID_STATUS: usize = 242;
const ID_MODE_BASE: usize = 250;
const ID_GRAPH_METRIC_BASE: usize = 270;
const ID_BAR_BG: usize = 280;
const ID_BAR_OPACITY: usize = 281;
const ID_BAR_TRANS: usize = 282;
const ID_AUTOSTART: usize = 283;
const ID_WIDGET_SEL: usize = 260;
const ID_WIDGET_BG: usize = 261;
const ID_WIDGET_OPACITY: usize = 262;
const ID_WIDGET_BORDER: usize = 263;

const CLIENT_W: i32 = 560;
const CLIENT_H: i32 = 382;

const EN_CHANGE: usize = 0x0300;
const EN_KILLFOCUS: usize = 0x0200;
const WM_SET_TARGET: u32 = WM_APP + 2;

struct Ctx {
    app: *mut App,
    edit: Metric,
    ready: bool,
    loading: bool,
    palette: Palette,
    bg_brush: HBRUSH,
    edit_brush: HBRUSH,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
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
    let cap = rgb(p.bg);
    let txt = rgb(p.text);
    let edge = rgb(p.divider);
    for (attr, value) in [(35u32, cap), (36u32, txt), (34u32, edge)] {
        DwmSetWindowAttribute(
            hwnd,
            attr,
            &value as *const u32 as *const c_void,
            std::mem::size_of::<u32>() as u32,
        );
    }

    let theme = if dark != 0 {
        wide("DarkMode_Explorer")
    } else {
        wide("")
    };
    SetWindowTheme(hwnd, theme.as_ptr(), null());
    EnumChildWindows(hwnd, Some(theme_child), theme.as_ptr() as isize);
    InvalidateRect(hwnd, null(), 1);
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

unsafe fn checked(h: HWND) -> bool {
    SendMessageW(h, BM_GETCHECK, 0, 0) == BST_CHECKED as isize
}

unsafe fn check(h: HWND, on: bool) {
    SendMessageW(
        h,
        BM_SETCHECK,
        if on {
            BST_CHECKED as usize
        } else {
            BST_UNCHECKED as usize
        },
        0,
    );
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
    let cls = wide("StatMonSettings");
    let wc = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
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

    let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
    let mut r = RECT {
        left: 0,
        top: 0,
        right: CLIENT_W,
        bottom: CLIENT_H,
    };
    AdjustWindowRectEx(&mut r, style, 0, 0);
    let w = r.right - r.left;
    let h = r.bottom - r.top;

    let mut work: RECT = zeroed();
    SystemParametersInfoW(
        SPI_GETWORKAREA,
        0,
        &mut work as *mut RECT as *mut c_void,
        0,
    );
    let x = work.left + ((work.right - work.left) - w) / 2;
    let y = work.top + ((work.bottom - work.top) - h) / 2;

    let edit = initial.unwrap_or(Metric::Cpu);
    let ctx = Box::into_raw(Box::new(Ctx {
        app,
        edit,
        ready: false,
        loading: false,
        palette: (*app).palette,
        bg_brush: null_mut(),
        edit_brush: null_mut(),
    }));
    let hwnd = CreateWindowExW(
        0,
        cls.as_ptr(),
        wide("StatMon - Ayarlar").as_ptr(),
        style,
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
        return;
    }
    (*app).settings_hwnd = hwnd;
    (*app).settings_target = None;
    ShowWindow(hwnd, SW_SHOW);
    SetForegroundWindow(hwnd);
}

unsafe fn load_widget(hwnd: HWND, fc: &FloatCfg) {
    set_text(GetDlgItem(hwnd, ID_WIDGET_BG as i32), &fc.bg);
    set_text(
        GetDlgItem(hwnd, ID_WIDGET_OPACITY as i32),
        &fc.opacity.to_string(),
    );
    check(GetDlgItem(hwnd, ID_TRANS as i32), fc.transparent);
    check(GetDlgItem(hwnd, ID_TOP as i32), fc.topmost);
    check(GetDlgItem(hwnd, ID_WIDGET_BORDER as i32), fc.border);
}

unsafe fn read_widget(hwnd: HWND, cfg: &mut Config, edit: Metric) {
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
    fc.transparent = checked(GetDlgItem(hwnd, ID_TRANS as i32));
    fc.topmost = checked(GetDlgItem(hwnd, ID_TOP as i32));
    fc.border = checked(GetDlgItem(hwnd, ID_WIDGET_BORDER as i32));
    if changed_bg && fc.transparent {
        fc.transparent = false;
        check(GetDlgItem(hwnd, ID_TRANS as i32), false);
    }
}

unsafe fn build_controls(hwnd: HWND, app: &App, edit: Metric) {
    let font = GetStockObject(17) as HFONT;
    let cfg = &app.cfg;

    let label = |t: &str, x: i32, y: i32, w: i32| -> HWND {
        mk(hwnd, "STATIC", t, 0, x, y, w, 20, 0, font)
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
    let combo = |x: i32, y: i32, w: i32, id: usize| -> HWND {
        mk(
            hwnd,
            "COMBOBOX",
            "",
            (CBS_DROPDOWNLIST as u32) | WS_VSCROLL,
            x,
            y,
            w,
            200,
            id,
            font,
        )
    };
    let checkbox = |t: &str, x: i32, y: i32, w: i32, id: usize| -> HWND {
        mk(
            hwnd,
            "BUTTON",
            t,
            BS_AUTOCHECKBOX as u32,
            x,
            y,
            w,
            22,
            id,
            font,
        )
    };

    label("Yenileme (ms)", 14, 16, 90);
    num(&cfg.refresh_ms.to_string(), 108, 14, 54, ID_REFRESH);
    label("Kapasite (sn)", 180, 16, 88);
    num(&cfg.capacity_secs.to_string(), 274, 14, 54, ID_CAPACITY);
    label("Tema", 342, 16, 40);
    let theme = combo(386, 14, 120, ID_THEME);
    for t in Theme::ALL {
        SendMessageW(theme, CB_ADDSTRING, 0, wide(t.label()).as_ptr() as isize);
    }
    SendMessageW(theme, CB_SETCURSEL, cfg.theme.index(), 0);

    label("Font etiket", 14, 48, 72);
    num(&cfg.font_label_px.to_string(), 92, 46, 54, ID_FONT_LABEL);
    label("Font değer", 156, 48, 72);
    num(&cfg.font_value_px.to_string(), 230, 46, 54, ID_FONT_VALUE);
    label("Yüzen font", 300, 48, 72);
    num(&cfg.float_font_px.to_string(), 374, 46, 54, ID_FLOAT_FONT);

    label("Grafik genişlik", 14, 80, 92);
    num(&cfg.graph_width.to_string(), 110, 78, 54, ID_GRAPH_W);
    let compact = checkbox("Sıkı yerleşim", 180, 76, 110, ID_COMPACT);
    check(compact, cfg.compact);
    let auto = checkbox("Windows açılışında başlat", 296, 76, 240, ID_AUTOSTART);
    check(auto, autostart_enabled());

    label("Göster:", 14, 112, 50);
    label("Görünüm:", 14, 144, 52);
    label("Grafik türü:", 14, 176, 64);
    label("Yüzen:", 14, 208, 50);

    for (i, m) in Metric::ALL.iter().enumerate() {
        let x = 70 + i as i32 * 92;

        let b = checkbox(m.label(), x, 108, 84, ID_SHOW_BASE + i);
        check(b, cfg.shows(*m));

        let mode = combo(x, 140, 84, ID_MODE_BASE + i);
        for v in ViewMode::ALL {
            SendMessageW(mode, CB_ADDSTRING, 0, wide(v.label()).as_ptr() as isize);
        }
        SendMessageW(mode, CB_SETCURSEL, cfg.view(*m).index(), 0);

        let g = combo(x, 172, 84, ID_GRAPH_METRIC_BASE + i);
        for kind in Graph::ALL {
            SendMessageW(g, CB_ADDSTRING, 0, wide(kind.label()).as_ptr() as isize);
        }
        SendMessageW(g, CB_SETCURSEL, cfg.graph_for(*m).index(), 0);

        let b = checkbox(m.label(), x, 204, 84, ID_FLOAT_BASE + i);
        check(b, cfg.float(*m).on);
    }

    label("Widget stili:", 14, 240, 80);
    let sel = combo(96, 238, 84, ID_WIDGET_SEL);
    for m in Metric::ALL {
        SendMessageW(sel, CB_ADDSTRING, 0, wide(m.label()).as_ptr() as isize);
    }
    let idx = Metric::ALL.iter().position(|m| *m == edit).unwrap_or(0);
    SendMessageW(sel, CB_SETCURSEL, idx, 0);

    label("Arka plan", 190, 240, 58);
    mk(
        hwnd,
        "EDIT",
        &cfg.float(edit).bg,
        WS_BORDER,
        250,
        238,
        74,
        22,
        ID_WIDGET_BG,
        font,
    );
    label("Opaklık", 332, 240, 48);
    num(
        &cfg.float(edit).opacity.to_string(),
        384,
        238,
        40,
        ID_WIDGET_OPACITY,
    );
    label("%", 428, 240, 14);
    let border = checkbox("Kenarlık", 448, 238, 82, ID_WIDGET_BORDER);
    check(border, cfg.float(edit).border);

    let tr = checkbox("Şeffaf arka plan", 70, 268, 130, ID_TRANS);
    check(tr, cfg.float(edit).transparent);
    let tp = checkbox("Her zaman üstte", 214, 268, 140, ID_TOP);
    check(tp, cfg.float(edit).topmost);
    label("(şeffafken arka plan görünmez)", 366, 270, 190);

    label("Bar arka plan", 14, 302, 80);
    mk(
        hwnd,
        "EDIT",
        &cfg.bar_bg,
        WS_BORDER,
        98,
        300,
        76,
        22,
        ID_BAR_BG,
        font,
    );
    label("Opaklık", 186, 302, 48);
    num(&cfg.bar_opacity.to_string(), 238, 300, 40, ID_BAR_OPACITY);
    label("%", 282, 302, 14);
    let bt = checkbox("Şeffaf bar", 300, 300, 96, ID_BAR_TRANS);
    check(bt, cfg.bar_transparent);

    mk(
        hwnd,
        "BUTTON",
        "Kaydet",
        BS_PUSHBUTTON as u32,
        14,
        338,
        84,
        28,
        ID_SAVE,
        font,
    );
    mk(
        hwnd,
        "BUTTON",
        "Kapat",
        BS_PUSHBUTTON as u32,
        106,
        338,
        84,
        28,
        ID_CLOSE,
        font,
    );
    mk(
        hwnd,
        "STATIC",
        "",
        0,
        204,
        344,
        330,
        20,
        ID_STATUS,
        font,
    );
}

unsafe fn save_from(hwnd: HWND, app: &mut App, edit: Metric) {
    let mut cfg = app.cfg.clone();
    let old = cfg.clone();

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

    let t = SendMessageW(GetDlgItem(hwnd, ID_THEME as i32), CB_GETCURSEL, 0, 0);
    if t >= 0 {
        cfg.theme = Theme::from_index(t as usize);
    }

    for (i, m) in Metric::ALL.iter().enumerate() {
        cfg.set_show(
            *m,
            checked(GetDlgItem(hwnd, (ID_SHOW_BASE + i) as i32)),
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
        fc.on = checked(GetDlgItem(hwnd, (ID_FLOAT_BASE + i) as i32));
    }
    cfg.compact = checked(GetDlgItem(hwnd, ID_COMPACT as i32));

    let bar_bg = get_text(GetDlgItem(hwnd, ID_BAR_BG as i32));
    if let Some(c) = parse_color(&bar_bg) {
        cfg.bar_bg = hex_color(c);
    }
    cfg.bar_opacity =
        get_int(GetDlgItem(hwnd, ID_BAR_OPACITY as i32), cfg.bar_opacity as i32)
            .clamp(20, 100) as u8;
    cfg.bar_transparent = checked(GetDlgItem(hwnd, ID_BAR_TRANS as i32));

    read_widget(hwnd, &mut cfg, edit);

    let want_auto = checked(GetDlgItem(hwnd, ID_AUTOSTART as i32));
    if want_auto != autostart_enabled() {
        set_autostart(want_auto);
    }

    app.apply_config(cfg);
    set_text(GetDlgItem(hwnd, ID_STATUS as i32), "Uygulandı");
}

unsafe fn apply_widget_only(hwnd: HWND, app: &mut App, edit: Metric) {
    let mut cfg = app.cfg.clone();
    read_widget(hwnd, &mut cfg, edit);
    app.cfg = cfg;
    app.save();
    app.restyle();
}

unsafe fn apply_bar_only(hwnd: HWND, app: &mut App) {
    let mut cfg = app.cfg.clone();
    let bg = get_text(GetDlgItem(hwnd, ID_BAR_BG as i32));
    if let Some(c) = parse_color(&bg) {
        cfg.bar_bg = hex_color(c);
    }
    cfg.bar_opacity =
        get_int(GetDlgItem(hwnd, ID_BAR_OPACITY as i32), cfg.bar_opacity as i32)
            .clamp(20, 100) as u8;
    cfg.bar_transparent = checked(GetDlgItem(hwnd, ID_BAR_TRANS as i32));
    app.cfg = cfg;
    app.save();
    crate::apply_bar_style(&*app);
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
                build_controls(hwnd, &*(*ctx).app, (*ctx).edit);
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
                    load_widget(hwnd, (*ctx).app.as_ref().unwrap().cfg.float(m));
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
                FillRect(hdc_cast(w), &rc, (*ctx).bg_brush);
                return 1;
            }
            0
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => {
            let ctx = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Ctx;
            if !ctx.is_null() {
                let hdc = hdc_cast(w);
                SetTextColor(hdc, rgb((*ctx).palette.text));
                SetBkColor(hdc, rgb((*ctx).palette.bg));
                return (*ctx).bg_brush as LRESULT;
            }
            0
        }
        WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => {
            let ctx = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Ctx;
            if !ctx.is_null() {
                let hdc = hdc_cast(w);
                SetTextColor(hdc, rgb((*ctx).palette.text));
                SetBkColor(hdc, rgb((*ctx).palette.surface));
                return (*ctx).edit_brush as LRESULT;
            }
            0
        }
        WM_COMMAND => {
            let id = w & 0xFFFF;
            let code = (w >> 16) & 0xFFFF;
            let ctx = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Ctx;
            if !ctx.is_null() && (*ctx).ready && !(*ctx).loading {
                let app = (*ctx).app;
                match id {
                    ID_SAVE => {
                        save_from(hwnd, &mut *app, (*ctx).edit);
                        apply_window_theme(hwnd, &mut *ctx);
                    }
                    ID_CLOSE => {
                        DestroyWindow(hwnd);
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
                                load_widget(hwnd, (*app).cfg.float((*ctx).edit));
                                (*ctx).loading = false;
                            }
                        }
                    }
                    ID_WIDGET_BG | ID_WIDGET_OPACITY => {
                        if code == EN_CHANGE || code == EN_KILLFOCUS {
                            apply_widget_only(hwnd, &mut *app, (*ctx).edit);
                        }
                    }
                    ID_TRANS | ID_TOP | ID_WIDGET_BORDER => {
                        apply_widget_only(hwnd, &mut *app, (*ctx).edit);
                    }
                    ID_BAR_BG | ID_BAR_OPACITY | ID_BAR_TRANS => {
                        apply_bar_only(hwnd, &mut *app);
                    }
                    ID_AUTOSTART => {
                        let on = checked(GetDlgItem(hwnd, ID_AUTOSTART as i32));
                        set_autostart(on);
                    }
                    _ => {
                        let live = id == ID_COMPACT
                            || id == ID_THEME
                            || (ID_SHOW_BASE..ID_SHOW_BASE + 5).contains(&id)
                            || (ID_FLOAT_BASE..ID_FLOAT_BASE + 5).contains(&id)
                            || (ID_MODE_BASE..ID_MODE_BASE + 5).contains(&id)
                            || (ID_GRAPH_METRIC_BASE..ID_GRAPH_METRIC_BASE + 5).contains(&id);
                        if live {
                            save_from(hwnd, &mut *app, (*ctx).edit);
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
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(ctx));
            }
            DefWindowProcW(hwnd, msg, w, l)
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}

fn hdc_cast(w: usize) -> windows_sys::Win32::Graphics::Gdi::HDC {
    w as *mut c_void as windows_sys::Win32::Graphics::Gdi::HDC
}
