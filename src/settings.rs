use std::ffi::c_void;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::{GetStockObject, HFONT};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{
    BM_GETCHECK, BM_SETCHECK, BST_CHECKED, BST_UNCHECKED, CB_ADDSTRING, CB_GETCURSEL, CB_SETCURSEL,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::config::{FloatCfg, Graph, Theme};
use crate::metrics::Metric;
use crate::App;

const ID_REFRESH: usize = 200;
const ID_CAPACITY: usize = 201;
const ID_THEME: usize = 202;
const ID_FONT_LABEL: usize = 203;
const ID_FONT_VALUE: usize = 204;
const ID_GRAPH: usize = 205;
const ID_GRAPH_W: usize = 206;
const ID_FLOAT_FONT: usize = 207;
const ID_SHOW_BASE: usize = 210;
const ID_FLOAT_BASE: usize = 220;
const ID_TRANS: usize = 230;
const ID_TOP: usize = 231;
const ID_SAVE: usize = 240;
const ID_CLOSE: usize = 241;
const ID_STATUS: usize = 242;

const CLIENT_W: i32 = 560;
const CLIENT_H: i32 = 250;

struct Ctx {
    app: *mut App,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
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
    SetWindowTextW(h, wide(s).as_ptr());
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

pub unsafe fn open(app: *mut App) {
    if !(*app).settings_hwnd.is_null() {
        ShowWindow((*app).settings_hwnd, SW_SHOW);
        SetForegroundWindow((*app).settings_hwnd);
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
        hIcon: LoadIconW(null_mut(), IDI_APPLICATION),
        hCursor: LoadCursorW(null_mut(), IDC_ARROW),
        hbrBackground: 6usize as *mut c_void,
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

    let ctx = Box::into_raw(Box::new(Ctx { app }));
    let hwnd = CreateWindowExW(
        0,
        cls.as_ptr(),
        wide("StatMon - Ayarlar").as_ptr(),
        style,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
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
    ShowWindow(hwnd, SW_SHOW);
    SetForegroundWindow(hwnd);
}

unsafe fn build_controls(hwnd: HWND, app: &App) {
    let font = GetStockObject(17) as HFONT;
    let cfg = &app.cfg;
    let label = |t: &str, x: i32, y: i32, w: i32| -> HWND {
        unsafe {
            mk(
                hwnd,
                "STATIC",
                t,
                0,
                x,
                y,
                w,
                20,
                0,
                font,
            )
        }
    };
    let edit = |v: &str, x: i32, y: i32, id: usize| -> HWND {
        unsafe {
            let h = mk(
                hwnd,
                "EDIT",
                v,
                WS_BORDER | (ES_NUMBER as u32),
                x,
                y,
                54,
                22,
                id,
                font,
            );
            h
        }
    };

    label("Yenileme (ms)", 14, 16, 90);
    edit(&cfg.refresh_ms.to_string(), 108, 14, ID_REFRESH);
    label("Kapasite (sn)", 180, 16, 88);
    edit(&cfg.capacity_secs.to_string(), 274, 14, ID_CAPACITY);
    label("Tema", 342, 16, 40);
    let theme = mk(
        hwnd,
        "COMBOBOX",
        "",
        (CBS_DROPDOWNLIST as u32) | WS_VSCROLL,
        386,
        14,
        120,
        200,
        ID_THEME,
        font,
    );
    for t in Theme::ALL {
        SendMessageW(theme, CB_ADDSTRING, 0, wide(t.label()).as_ptr() as isize);
    }
    SendMessageW(theme, CB_SETCURSEL, cfg.theme.index(), 0);

    label("Font etiket", 14, 48, 72);
    edit(&cfg.font_label_px.to_string(), 88, 46, ID_FONT_LABEL);
    label("Font deÄŸer", 152, 48, 72);
    edit(&cfg.font_value_px.to_string(), 226, 46, ID_FONT_VALUE);
    label("YÃ¼zen font", 292, 48, 72);
    edit(&cfg.float_font_px.to_string(), 366, 46, ID_FLOAT_FONT);

    label("Grafik", 14, 80, 44);
    let graph = mk(
        hwnd,
        "COMBOBOX",
        "",
        (CBS_DROPDOWNLIST as u32) | WS_VSCROLL,
        62,
        78,
        100,
        200,
        ID_GRAPH,
        font,
    );
    for g in Graph::ALL {
        SendMessageW(graph, CB_ADDSTRING, 0, wide(g.label()).as_ptr() as isize);
    }
    SendMessageW(graph, CB_SETCURSEL, cfg.graph.index(), 0);
    label("Grafik geniÅŸlik", 184, 80, 92);
    edit(&cfg.graph_width.to_string(), 278, 78, ID_GRAPH_W);

    label("GÃ¶ster:", 14, 116, 50);
    label("YÃ¼zen:", 14, 146, 50);

    for (i, m) in Metric::ALL.iter().enumerate() {
        let x = 70 + i as i32 * 92;
        let shown = cfg.shows(*m);
        let b = mk(
            hwnd,
            "BUTTON",
            m.label(),
            BS_AUTOCHECKBOX as u32,
            x,
            114,
            84,
            22,
            ID_SHOW_BASE + i,
            font,
        );
        check(b, shown);

        let f = cfg.float(*m);
        let b = mk(
            hwnd,
            "BUTTON",
            m.label(),
            BS_AUTOCHECKBOX as u32,
            x,
            144,
            84,
            22,
            ID_FLOAT_BASE + i,
            font,
        );
        check(b, f.on);
    }

    let tr = mk(
        hwnd,
        "BUTTON",
        "Åeffaf arka plan",
        BS_AUTOCHECKBOX as u32,
        70,
        174,
        130,
        22,
        ID_TRANS,
        font,
    );
    check(tr, cfg.float_transparent);
    let tp = mk(
        hwnd,
        "BUTTON",
        "Her zaman Ã¼stte",
        BS_AUTOCHECKBOX as u32,
        214,
        174,
        140,
        22,
        ID_TOP,
        font,
    );
    check(tp, cfg.float_topmost);

    mk(
        hwnd,
        "BUTTON",
        "Kaydet",
        BS_PUSHBUTTON as u32,
        14,
        206,
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
        206,
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
        212,
        320,
        20,
        ID_STATUS,
        font,
    );
}

unsafe fn save_from(hwnd: HWND, app: &mut App) {
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
        .clamp(24, 400);

    let g = SendMessageW(GetDlgItem(hwnd, ID_GRAPH as i32), CB_GETCURSEL, 0, 0);
    if g >= 0 {
        cfg.graph = Graph::from_index(g as usize);
    }
    let t = SendMessageW(GetDlgItem(hwnd, ID_THEME as i32), CB_GETCURSEL, 0, 0);
    if t >= 0 {
        cfg.theme = Theme::from_index(t as usize);
    }

    for (i, m) in Metric::ALL.iter().enumerate() {
        cfg.set_show(
            *m,
            checked(GetDlgItem(hwnd, (ID_SHOW_BASE + i) as i32)),
        );
        let fc: &mut FloatCfg = cfg.float_mut(*m);
        fc.on = checked(GetDlgItem(hwnd, (ID_FLOAT_BASE + i) as i32));
    }
    cfg.float_transparent = checked(GetDlgItem(hwnd, ID_TRANS as i32));
    cfg.float_topmost = checked(GetDlgItem(hwnd, ID_TOP as i32));

    app.apply_config(cfg);
    set_text(GetDlgItem(hwnd, ID_STATUS as i32), "Kaydedildi");
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
                build_controls(hwnd, &*(*ctx).app);
            }
            0
        }
        WM_COMMAND => {
            let id = w & 0xFFFF;
            let ctx = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Ctx;
            if !ctx.is_null() {
                match id {
                    ID_SAVE => save_from(hwnd, &mut *(*ctx).app),
                    ID_CLOSE => {
                        DestroyWindow(hwnd);
                    }
                    _ => {}
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
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(ctx));
            }
            DefWindowProcW(hwnd, msg, w, l)
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}
