use anyhow::Result;
use core::ffi::c_void;
use thiserror::Error;
use windows::Win32::Graphics::Gdi::BeginPaint;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, EndPaint, SelectObject,
    AC_SRC_ALPHA, AC_SRC_OVER, BLENDFUNCTION, PAINTSTRUCT,
};
use windows::Win32::Graphics::GdiPlus::{
    FontStyleRegular, GdipCreateFont, GdipCreateFontFamilyFromName, GdipCreateFromHDC,
    GdipCreateSolidFill, GdipDeleteBrush, GdipDeleteFont, GdipDeleteFontFamily, GdipDrawString,
    GdipGraphicsClear, GdipMeasureString, GdipSetInterpolationMode, GdipSetSmoothingMode,
    GdipSetTextRenderingHint, GdiplusStartup, GdiplusStartupInput, GpBrush, GpFont, GpFontFamily,
    GpGraphics, GpSolidFill, InterpolationModeHighQualityBicubic, RectF, SmoothingModeAntiAlias,
    TextRenderingHintAntiAlias, UnitPoint,
};
use windows::{
    core::*, Win32::Foundation::*, Win32::Graphics::GdiPlus,
    Win32::System::LibraryLoader::GetModuleHandleW, Win32::UI::WindowsAndMessaging::FindWindowW,
    Win32::UI::WindowsAndMessaging::*,
};

use crate::api;
use tokio::sync::mpsc;

pub struct Window {
    pub hwnd: usize,
    pub width: i32,
    pub height: i32,
    class_name: String,
    title: String,
    pub pos: POINT,
    pub sender: mpsc::Sender<api::TradePair>,
    trade_pair: api::TradePair,
}

#[derive(Error, Debug)]
#[error("{erro_msg}")]
struct WindowError {
    erro_msg: String,
}

impl Window {
    pub const WM_FRESH: u32 = WM_USER + 1;
    const COMAMND_BTCUSDT: usize = 1;
    const COMAMND_ETHUSDT: usize = 2;
    const COMAMND_SOLUSDT: usize = 3;
    const COMAMND_EXIT: usize = 4;

    const ALPHA_SHIFT: u32 = 24;
    const RED_SHIFT: u32 = 16;
    const GREEN_SHIFT: u32 = 8;
    const BLUE_SHIFT: u32 = 0;

    pub fn new(
        class_name: Option<&str>,
        title: Option<&str>,
        width: Option<i32>,
        sender: mpsc::Sender<api::TradePair>,
        trade_pair: api::TradePair,
    ) -> Self {
        let width = width.unwrap_or(70);
        let class_name = class_name.unwrap_or("mjj").to_string();
        let title = title.unwrap_or("mjj").to_string();
        Window {
            hwnd: 0,
            pos: POINT::default(),
            height: 0,
            width,
            class_name,
            title,
            sender,
            trade_pair,
        }
    }

    fn make_argb(a: u32, r: u32, g: u32, b: u32) -> u32 {
        (b << Self::BLUE_SHIFT)
            | (g << Self::GREEN_SHIFT)
            | (r << Self::RED_SHIFT)
            | (a << Self::ALPHA_SHIFT)
    }

    fn string_to_pwcstr(content_str: &str) -> PCWSTR {
        let mut content: Vec<u16> = content_str.encode_utf16().collect();
        content.push(0);
        PCWSTR::from_raw(content.as_ptr())
    }

    fn create_font(font_family_name: &str, font_size: f32) -> *mut GpFont {
        unsafe {
            let mut font_family: *mut GpFontFamily = std::ptr::null_mut();
            GdipCreateFontFamilyFromName(
                Self::string_to_pwcstr(font_family_name),
                std::ptr::null_mut(),
                &mut font_family,
            );
            let mut font: *mut GpFont = std::ptr::null_mut();
            GdipCreateFont(
                font_family,
                font_size,
                FontStyleRegular.0,
                UnitPoint,
                &mut font,
            );
            GdipDeleteFontFamily(font_family);
            font
        }
    }

    fn create_solid_brush(color: u32) -> *mut GpBrush {
        unsafe {
            let mut fill: *mut GpSolidFill = std::ptr::null_mut();
            GdipCreateSolidFill(color, &mut fill);
            fill as *mut GpBrush
        }
    }

    fn meansuer_string(
        graphics: *mut GpGraphics,
        content: PCWSTR,
        font: *const GpFont,
        lay_box: &RectF,
    ) -> RectF {
        let mut bound_box = RectF::default();
        unsafe {
            GdipMeasureString(
                graphics,
                content,
                -1,
                font,
                lay_box,
                std::ptr::null_mut(),
                &mut bound_box,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
        }
        bound_box
    }

    fn generate_mid_rect(lay_rect: &RectF, text_bound: &RectF) -> RectF {
        let mut dst_rect = RectF::default();
        if lay_rect.Width >= text_bound.Width {
            dst_rect.X = (lay_rect.Width - text_bound.Width) / 2. + lay_rect.X;
        } else {
            dst_rect.X = lay_rect.X;
        }
        if lay_rect.Height >= text_bound.Height {
            dst_rect.Y = (lay_rect.Height - text_bound.Height) / 2. + lay_rect.Y;
        } else {
            dst_rect.Y = lay_rect.Y;
        }
        dst_rect.Width = text_bound.Width;
        dst_rect.Height = text_bound.Height;
        dst_rect
    }

    fn draw_price(
        graphics: *mut GpGraphics,
        font_price: *mut GpFont,
        brush_price: *mut GpBrush,
        font_pair: *mut GpFont,
        brush_pair: *mut GpBrush,
        window: &mut Window,
        price:&api::Price
    ) {
        let lay_box_price = RectF {
            X: 0.,
            Y: window.height as f32 / 2.2,
            Width: window.width as f32,
            Height: window.height as f32 / 2.,
        };
        let lay_box_pair = RectF {
            X: 0.,
            Y: window.height as f32 * 0.1,
            Width: window.width as f32,
            Height: window.height as f32 / 2.,
        };
        let content_str = format!("{:.1}", price.tag_price);
        let bound = Self::meansuer_string(
            graphics,
            Self::string_to_pwcstr(&content_str),
            font_price,
            &lay_box_price,
        );
        let dst_rect = Self::generate_mid_rect(&lay_box_price, &bound);
        unsafe {
            GdipDrawString(
                graphics,
                Self::string_to_pwcstr(&content_str),
                -1,
                font_price,
                &dst_rect,
                std::ptr::null_mut(),
                brush_price,
            );
        }
        let content_str = &api::TRADE_INFO.get(&window.trade_pair).unwrap().show_name;

        let bound = Self::meansuer_string(
            graphics,
            Self::string_to_pwcstr(&content_str),
            font_pair,
            &lay_box_pair,
        );
        let dst_rect = Self::generate_mid_rect(&lay_box_pair, &bound);
        unsafe {
            GdipDrawString(
                graphics,
                Self::string_to_pwcstr(&content_str),
                -1,
                font_pair,
                &dst_rect,
                std::ptr::null_mut(),
                brush_pair,
            );
        }
    }

    fn draw_notify(graphics: *mut GpGraphics, font: *const GpFont, brush:* const GpBrush, window:& mut Window, not_msg:&str){
        let lay_box = RectF {
            X: 0.,
            Y: 0.,
            Width: window.width as f32,
            Height: window.height as f32,
        };
        let bound = Self::meansuer_string(
            graphics,
            Self::string_to_pwcstr(not_msg),
            font,
            &lay_box,
        );
        let dst_rect = Self::generate_mid_rect(&lay_box, &bound);
        unsafe{GdipDrawString(
            graphics,
            Self::string_to_pwcstr(not_msg),
            -1,
            font,
            &dst_rect,
            std::ptr::null_mut(),
            brush,
        );}
    }

    fn fresh_window(hwnd: &HWND, wparam: &WPARAM) -> Result<()> {
        unsafe {
            let api_msg = Box::from_raw(wparam.0 as *mut api::ApiMessage);
            let window = &mut *(GetWindowLongPtrW(*hwnd, GWLP_USERDATA) as *mut Self);
            match &*api_msg {
                api::ApiMessage::Price(price) => {
                    let check;
                    let cur_trade_name = api::TRADE_INFO
                        .get(&window.trade_pair)
                        .unwrap()
                        .pair_name
                        .clone();
                    check = cur_trade_name == price.name;
                    if !check {
                        return Ok(());
                    }
                }
                _ => {}
            }
            let mut client_rect = RECT::default();
            GetClientRect(*hwnd, &mut client_rect)?;
            let width = client_rect.right - client_rect.left;
            let height = client_rect.bottom - client_rect.top;

            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(*hwnd, &mut ps);
            let hdc_mem = CreateCompatibleDC(hdc);
            let h_bitmap = CreateCompatibleBitmap(hdc, width, height);
            SelectObject(hdc_mem, h_bitmap);

            let mut graphics: *mut GpGraphics = std::ptr::null_mut();
            GdipCreateFromHDC(hdc_mem, &mut graphics);
            GdipSetTextRenderingHint(graphics, TextRenderingHintAntiAlias);
            GdipSetSmoothingMode(graphics, SmoothingModeAntiAlias);
            GdipSetInterpolationMode(graphics, InterpolationModeHighQualityBicubic);

            GdipGraphicsClear(graphics, Self::make_argb(1, 255, 255, 255));
            let font = Self::create_font("Microsoft YaHei UI", 9.);
            let font_small = Self::create_font("Microsoft YaHei UI", 9.);
            let brush = Self::create_solid_brush(Self::make_argb(255, 0, 0, 0));

            match *api_msg {
                api::ApiMessage::Price(price) => {
                    Self::draw_price(graphics, font, brush, font_small, brush, window, &price);
                }
                api::ApiMessage::Notify(not_msg) => {
                    Self::draw_notify(graphics, font, brush, window, &not_msg);
                }
            }
            let mut blend = BLENDFUNCTION::default();
            blend.BlendOp = AC_SRC_OVER as u8;
            blend.BlendFlags = 0;
            blend.SourceConstantAlpha = 255;
            blend.AlphaFormat = AC_SRC_ALPHA as u8;
            let size = SIZE {
                cx: width,
                cy: height,
            };
            let point = POINT { x: 0, y: 0 };
            let _ = UpdateLayeredWindow(
                *hwnd,
                hdc,
                None,
                Some(&size),
                hdc_mem,
                Some(&point),
                None,
                Some(&blend),
                ULW_ALPHA,
            );

            GdipDeleteFont(font);
            GdipDeleteBrush(brush);
            let _ = DeleteObject(h_bitmap);
            let _ = DeleteDC(hdc_mem);
            let _ = EndPaint(*hwnd, &ps);
            Ok(())
        }
    }

    const GET_X_LPARAM: fn(LPARAM) -> i32 = |lparam| (lparam.0 & 0xFFFF) as i32;
    const GET_Y_LPARAM: fn(LPARAM) -> i32 = |lparam| ((lparam.0 >> 16) & 0xFFFF) as i32;
    extern "system" fn wndproc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        unsafe {
            match message {
                WM_RBUTTONDOWN => {
                    let menu = CreatePopupMenu().unwrap();
                    AppendMenuW(
                        menu,
                        MF_STRING,
                        Self::COMAMND_BTCUSDT,
                        Self::string_to_pwcstr(
                            &api::TRADE_INFO
                                .get(&api::TradePair::BTCUSDT)
                                .unwrap()
                                .show_name,
                        ),
                    )
                    .unwrap();
                    AppendMenuW(
                        menu,
                        MF_STRING,
                        Self::COMAMND_ETHUSDT,
                        Self::string_to_pwcstr(
                            &api::TRADE_INFO
                                .get(&api::TradePair::ETHUSDT)
                                .unwrap()
                                .show_name,
                        ),
                    )
                    .unwrap();
                    AppendMenuW(
                        menu,
                        MF_STRING,
                        Self::COMAMND_SOLUSDT,
                        Self::string_to_pwcstr(
                            &api::TRADE_INFO
                                .get(&api::TradePair::SOLUSDT)
                                .unwrap()
                                .show_name,
                        ),
                    )
                    .unwrap();
                    AppendMenuW(menu, MF_SEPARATOR, 0, None).unwrap();
                    AppendMenuW(menu, MF_STRING, Self::COMAMND_EXIT, w!("退出")).unwrap();

                    let point = POINT {
                        x: Self::GET_X_LPARAM(lparam),
                        y: Self::GET_Y_LPARAM(lparam),
                    };
                    let mut window_rect = RECT::default();
                    GetWindowRect(hwnd, &mut window_rect).unwrap();
                    let _ = TrackPopupMenu(
                        menu,
                        TPM_RIGHTBUTTON,
                        point.x + window_rect.left,
                        point.y + window_rect.top,
                        0,
                        hwnd,
                        None,
                    );
                    LRESULT(0)
                }
                WM_COMMAND => {
                    let window = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Self);
                    match wparam.0 as usize {
                        Self::COMAMND_BTCUSDT => {
                            if window.trade_pair != api::TradePair::BTCUSDT {
                                window.trade_pair = api::TradePair::BTCUSDT;
                                window
                                    .sender
                                    .blocking_send(api::TradePair::BTCUSDT)
                                    .unwrap();
                            }
                        }
                        Self::COMAMND_ETHUSDT => {
                            if window.trade_pair != api::TradePair::ETHUSDT {
                                window.trade_pair = api::TradePair::ETHUSDT;
                                window
                                    .sender
                                    .blocking_send(api::TradePair::ETHUSDT)
                                    .unwrap();
                            }
                        }
                        Self::COMAMND_SOLUSDT => {
                            if window.trade_pair != api::TradePair::SOLUSDT {
                                window.trade_pair = api::TradePair::SOLUSDT;
                                window
                                    .sender
                                    .blocking_send(api::TradePair::SOLUSDT)
                                    .unwrap();
                            }
                        }
                        Self::COMAMND_EXIT => {
                            std::process::exit(0);
                        }
                        _ => {}
                    }
                    LRESULT(0)
                }
                WM_TIMER => {
                    let window = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Self);
                    let (mut window_base_pos, window_height) = Self::get_window_base_pos().unwrap();
                    window_base_pos.x -= window.width;
                    if window_base_pos != window.pos || window_height != window.height {
                        window.pos = window_base_pos;
                        window.height = window_height;
                        let _ = SetWindowPos(
                            HWND(window.hwnd as *mut c_void),
                            None,
                            window.pos.x,
                            window.pos.y,
                            window.width,
                            window.height,
                            SWP_NOREDRAW,
                        );
                    }
                    LRESULT(0)
                }
                Self::WM_FRESH => {
                    let _ = Self::fresh_window(&hwnd, &wparam);
                    LRESULT(0)
                }
                WM_DESTROY => {
                    PostQuitMessage(0);
                    LRESULT(0)
                }
                _ => DefWindowProcW(hwnd, message, wparam, lparam),
            }
        }
    }

    fn init_gdi_plus() -> Result<()> {
        let mut gdiplus_token: usize = 0;
        let mut gdiplus_startup_input = GdiplusStartupInput::default();
        gdiplus_startup_input.GdiplusVersion = 1;
        unsafe {
            let status = GdiplusStartup(
                &mut gdiplus_token,
                &gdiplus_startup_input,
                std::ptr::null_mut(),
            );
            if status != GdiPlus::Ok {
                let err = WindowError {
                    erro_msg: format!("init gdi+ fail:{}", status.0),
                };
                return Err(err.into());
            }
        }
        Ok(())
    }

    pub fn init_window(&mut self) -> Result<()> {
        Self::init_gdi_plus()?;
        let taskbar_hwnd = Self::get_taskbar_hwnd()?;
        let (window_base_pos, height) = Self::get_window_base_pos()?;
        unsafe {
            let instance = GetModuleHandleW(None)?;
            let wc = WNDCLASSW {
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                hInstance: instance.into(),
                lpszClassName: Self::string_to_pwcstr(&self.class_name),
                lpfnWndProc: Some(Self::wndproc),
                ..Default::default()
            };
            let atom = RegisterClassW(&wc);
            if atom == 0 {
                let err = WindowError {
                    erro_msg: "registe window fail".to_string(),
                };
                return Err(err.into());
            }
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
                Self::string_to_pwcstr(&self.class_name),
                Self::string_to_pwcstr(&self.title),
                WS_POPUP,
                0,
                0,
                0,
                0,
                taskbar_hwnd,
                None,
                wc.hInstance,
                None,
            )?;
            if hwnd.is_invalid() {
                let err = WindowError {
                    erro_msg: "hwnd is invalid".to_string(),
                };
                return Err(err.into());
            }
            self.hwnd = hwnd.0 as usize;
            SetParent(HWND(self.hwnd as *mut c_void), taskbar_hwnd)?;
            self.pos = POINT {
                x: window_base_pos.x - self.width,
                y: window_base_pos.y,
            };
            self.height = height;
            SetWindowPos(
                HWND(self.hwnd as *mut c_void),
                None,
                self.pos.x,
                self.pos.y,
                self.width,
                self.height,
                SET_WINDOW_POS_FLAGS(0),
            )?;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, self as *mut Self as isize);
            SetTimer(hwnd, 1, 200, None);
        }
        Ok(())
    }

    fn get_taskbar_hwnd() -> Result<HWND> {
        unsafe { Ok(FindWindowW(w!("Shell_TrayWnd"), None)?) }
    }

    fn get_window_base_pos() -> Result<(POINT, i32)> {
        unsafe {
            let parent_hwnd = Self::get_taskbar_hwnd()?;
            if parent_hwnd.is_invalid() {
                let err = WindowError {
                    erro_msg: "can not find Shell_TrayWnd window".to_string(),
                };
                return Err(err.into());
            }
            let mut child_hwnd = FindWindowExW(parent_hwnd, None, w!("ReBarWindow32"), None)?;
            if child_hwnd.is_invalid() {
                let err = WindowError {
                    erro_msg: "can not find ReBarWindow32 window".to_string(),
                };
                return Err(err.into());
            }
            child_hwnd = FindWindowExW(child_hwnd, None, w!("MSTaskSwWClass"), None)?;
            if child_hwnd.is_invalid() {
                let err = WindowError {
                    erro_msg: "can not find MSTaskSwWClass window".to_string(),
                };
                return Err(err.into());
            }
            let mut child_rect = RECT::default();
            GetWindowRect(child_hwnd, &mut child_rect)?;
            let mut parent_rect = RECT::default();
            GetWindowRect(parent_hwnd, &mut parent_rect)?;
            let pos = POINT {
                x: child_rect.left - parent_rect.left,
                y: child_rect.top - parent_rect.top,
            };
            Ok((pos, child_rect.bottom - child_rect.top))
        }
    }

    pub fn run_window(&mut self) -> Result<()> {
        unsafe {
            let _ = ShowWindow(HWND(self.hwnd as *mut c_void), SW_SHOW);
            {
                let message = api::ApiMessage::Notify("启动...".to_string());
                let message_p = Box::into_raw(Box::new(message)) as *mut c_void;
                PostMessageW(
                    HWND(self.hwnd as *mut c_void),
                    Self::WM_FRESH,
                    WPARAM(message_p as usize),
                    LPARAM::default(),
                )
                .unwrap();
                let message = api::ApiMessage::Notify("启动...".to_string());
                let message_p = Box::into_raw(Box::new(message)) as *mut c_void;
                PostMessageW(
                    HWND(self.hwnd as *mut c_void),
                    Self::WM_FRESH,
                    WPARAM(message_p as usize),
                    LPARAM::default(),
                )
                .unwrap();
            }
            let mut message = MSG::default();
            while GetMessageW(&mut message, None, 0, 0).into() {
                DispatchMessageW(&message);
            }
        }
        Ok(())
    }
}
