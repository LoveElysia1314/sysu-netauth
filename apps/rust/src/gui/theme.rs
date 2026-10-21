//! App-theme colors and painting for standard Win32 controls.
//! Controls retain their native input handling and accessibility roles.
use super::{CONNECT, rgb, scaled, text};
use crate::{engine::State, platform::wide};
use std::{mem, ptr};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::{Dwm::*, Gdi::*},
    System::Registry::*,
    UI::{
        Accessibility::*,
        Controls::*,
        Input::KeyboardAndMouse::{GetFocus, IsWindowEnabled},
        Shell::*,
        WindowsAndMessaging::*,
    },
};

pub(super) struct Theme {
    pub dark: bool,
    pub high_contrast: bool,
    pub background: COLORREF,
    pub field: COLORREF,
    pub text: COLORREF,
    pub muted: COLORREF,
    border: COLORREF,
    button: COLORREF,
    hover: COLORREF,
    accent: COLORREF,
    pub background_brush: HBRUSH,
    pub field_brush: HBRUSH,
}

impl Theme {
    pub fn system() -> Self {
        let mut contrast: HIGHCONTRASTW = unsafe { mem::zeroed() };
        contrast.cbSize = mem::size_of_val(&contrast) as u32;
        let mut light = 1u32;
        let mut len = mem::size_of_val(&light) as u32;
        unsafe {
            SystemParametersInfoW(
                SPI_GETHIGHCONTRAST,
                contrast.cbSize,
                (&mut contrast as *mut HIGHCONTRASTW).cast(),
                0,
            );
            RegGetValueW(
                HKEY_CURRENT_USER,
                wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize").as_ptr(),
                wide("AppsUseLightTheme").as_ptr(),
                RRF_RT_REG_DWORD,
                ptr::null_mut(),
                (&mut light as *mut u32).cast(),
                &mut len,
            );
        }
        let high_contrast = contrast.dwFlags & HCF_HIGHCONTRASTON != 0;
        let dark = light == 0 && !high_contrast;
        let (background, field, text, muted, border, button, hover) = if high_contrast {
            unsafe {
                (
                    GetSysColor(COLOR_WINDOW),
                    GetSysColor(COLOR_WINDOW),
                    GetSysColor(COLOR_WINDOWTEXT),
                    GetSysColor(COLOR_GRAYTEXT),
                    GetSysColor(COLOR_WINDOWFRAME),
                    GetSysColor(COLOR_BTNFACE),
                    GetSysColor(COLOR_HIGHLIGHT),
                )
            }
        } else if dark {
            (
                rgb(24, 24, 27),
                rgb(39, 39, 42),
                rgb(228, 228, 231),
                rgb(161, 161, 170),
                rgb(82, 82, 91),
                rgb(47, 47, 51),
                rgb(63, 63, 70),
            )
        } else {
            (
                rgb(255, 255, 255),
                rgb(255, 255, 255),
                rgb(24, 24, 27),
                rgb(113, 113, 122),
                rgb(190, 190, 198),
                rgb(246, 246, 248),
                rgb(233, 239, 250),
            )
        };
        Self {
            dark,
            high_contrast,
            background,
            field,
            text,
            muted,
            border,
            button,
            hover,
            accent: rgb(37, 99, 235),
            background_brush: unsafe { CreateSolidBrush(background) },
            field_brush: unsafe { CreateSolidBrush(field) },
        }
    }

    pub fn title_bar(&self, hwnd: HWND) {
        let dark = self.dark as i32;
        unsafe {
            // Older Windows versions may reject this attribute; client colors still work.
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
                (&dark as *const i32).cast(),
                mem::size_of_val(&dark) as u32,
            );
        }
    }

    pub fn install(&self, hwnd: HWND, class: &str) {
        let kind = match class {
            "BUTTON" => 1,
            "COMBOBOX" => 2,
            _ => return,
        };
        unsafe {
            // The Window containing this Theme stays boxed until its child windows die.
            SetWindowSubclass(hwnd, Some(control_proc), kind, self as *const Self as usize);
        }
    }

    pub fn status(&self, state: &State) -> (COLORREF, COLORREF) {
        if self.high_contrast {
            return (self.text, self.background);
        }
        status_colors(self.dark, state)
    }

    fn rounded(&self, dc: HDC, rect: RECT, fill: COLORREF, border: COLORREF, radius: i32) {
        unsafe {
            let old_brush = SelectObject(dc, GetStockObject(DC_BRUSH));
            let old_pen = SelectObject(dc, GetStockObject(DC_PEN));
            SetDCBrushColor(dc, fill);
            SetDCPenColor(dc, border);
            RoundRect(
                dc,
                rect.left,
                rect.top,
                rect.right,
                rect.bottom,
                radius,
                radius,
            );
            SelectObject(dc, old_brush);
            SelectObject(dc, old_pen);
        }
    }

    fn button(&self, hwnd: HWND, dc: HDC, rect: RECT, dpi: u32) {
        let style = unsafe { GetWindowLongW(hwnd, GWL_STYLE) } as u32 & BS_TYPEMASK as u32;
        let state = unsafe { SendMessageW(hwnd, BM_GETSTATE, 0, 0) } as u32;
        let enabled = unsafe { IsWindowEnabled(hwnd) } != 0;
        let label = wide(text(hwnd));
        let mut label_rect = rect;
        let mut flags = DT_SINGLELINE | DT_VCENTER;
        let mut foreground = if enabled { self.text } else { self.muted };
        let hot = enabled && state & BST_HOT != 0;
        let pressed = enabled && state & BST_PUSHED != 0;
        unsafe {
            if style != BS_GROUPBOX as u32 {
                FillRect(dc, &rect, self.background_brush);
            }
            if style == BS_GROUPBOX as u32 {
                let mut frame = rect;
                frame.top += scaled(8, dpi);
                let old_brush = SelectObject(dc, GetStockObject(HOLLOW_BRUSH));
                let old_pen = SelectObject(dc, GetStockObject(DC_PEN));
                SetDCPenColor(dc, self.border);
                RoundRect(
                    dc,
                    frame.left,
                    frame.top,
                    frame.right,
                    frame.bottom,
                    scaled(6, dpi),
                    scaled(6, dpi),
                );
                SelectObject(dc, old_brush);
                SelectObject(dc, old_pen);
                label_rect.left += scaled(8, dpi);
                label_rect.top = 0;
                DrawTextW(
                    dc,
                    label.as_ptr(),
                    -1,
                    &mut label_rect,
                    DT_CALCRECT | DT_SINGLELINE,
                );
                label_rect.right += scaled(6, dpi);
                FillRect(dc, &label_rect, self.background_brush);
                label_rect.left += scaled(3, dpi);
                flags = DT_SINGLELINE;
            } else if style == BS_AUTOCHECKBOX as u32 {
                let size = scaled(15, dpi);
                let left = rect.left + scaled(1, dpi);
                let top = (rect.bottom - size) / 2;
                let checked = SendMessageW(hwnd, BM_GETCHECK, 0, 0) == BST_CHECKED as isize;
                let fill = if checked && enabled {
                    self.accent
                } else if hot {
                    self.hover
                } else {
                    self.field
                };
                self.rounded(
                    dc,
                    RECT {
                        left,
                        top,
                        right: left + size,
                        bottom: top + size,
                    },
                    fill,
                    if hot { self.accent } else { self.border },
                    scaled(4, dpi),
                );
                if checked {
                    let pen = CreatePen(
                        PS_SOLID,
                        scaled(2, dpi),
                        if enabled {
                            rgb(255, 255, 255)
                        } else {
                            self.muted
                        },
                    );
                    let old = SelectObject(dc, pen);
                    MoveToEx(
                        dc,
                        left + scaled(3, dpi),
                        top + scaled(7, dpi),
                        ptr::null_mut(),
                    );
                    LineTo(dc, left + scaled(6, dpi), top + scaled(10, dpi));
                    LineTo(dc, left + scaled(12, dpi), top + scaled(4, dpi));
                    SelectObject(dc, old);
                    DeleteObject(pen);
                }
                label_rect.left += size + scaled(7, dpi);
            } else {
                let primary = GetDlgCtrlID(hwnd) == CONNECT as i32 && enabled;
                let fill = if primary {
                    self.accent
                } else if pressed {
                    self.field
                } else if hot {
                    self.hover
                } else {
                    self.button
                };
                self.rounded(
                    dc,
                    rect,
                    fill,
                    if primary { self.accent } else { self.border },
                    scaled(6, dpi),
                );
                if primary {
                    foreground = rgb(255, 255, 255);
                }
                flags |= DT_CENTER;
                if pressed {
                    OffsetRect(&mut label_rect, scaled(1, dpi), scaled(1, dpi));
                }
            }
            SetBkMode(dc, TRANSPARENT as i32);
            SetTextColor(dc, foreground);
            let cues = SendMessageW(hwnd, WM_QUERYUISTATE, 0, 0) as u32;
            if cues & UISF_HIDEACCEL != 0 {
                flags |= DT_HIDEPREFIX;
            }
            DrawTextW(dc, label.as_ptr(), -1, &mut label_rect, flags);
            if state & BST_FOCUS != 0 && cues & UISF_HIDEFOCUS == 0 {
                let mut focus = rect;
                InflateRect(&mut focus, -scaled(3, dpi), -scaled(3, dpi));
                DrawFocusRect(dc, &focus);
            }
        }
    }

    fn combo(&self, hwnd: HWND, dc: HDC, rect: RECT, dpi: u32) {
        let enabled = unsafe { IsWindowEnabled(hwnd) } != 0;
        let color = if enabled { self.text } else { self.muted };
        self.rounded(dc, rect, self.field, self.border, scaled(4, dpi));
        let mut label_rect = rect;
        label_rect.left += scaled(7, dpi);
        label_rect.right -= scaled(28, dpi);
        let label = wide(text(hwnd));
        unsafe {
            SetBkMode(dc, TRANSPARENT as i32);
            SetTextColor(dc, color);
            DrawTextW(
                dc,
                label.as_ptr(),
                -1,
                &mut label_rect,
                DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
            );
            let x = rect.right - scaled(13, dpi);
            let y = (rect.bottom - rect.top) / 2;
            let old = SelectObject(dc, GetStockObject(DC_PEN));
            SetDCPenColor(dc, color);
            MoveToEx(dc, x - scaled(4, dpi), y - scaled(2, dpi), ptr::null_mut());
            LineTo(dc, x, y + scaled(2, dpi));
            LineTo(dc, x + scaled(4, dpi), y - scaled(2, dpi));
            SelectObject(dc, old);
            if GetFocus() == hwnd
                && SendMessageW(hwnd, WM_QUERYUISTATE, 0, 0) as u32 & UISF_HIDEFOCUS == 0
            {
                InflateRect(&mut label_rect, -scaled(2, dpi), -scaled(2, dpi));
                DrawFocusRect(dc, &label_rect);
            }
        }
    }

    pub fn combo_item(&self, item: &DRAWITEMSTRUCT, dpi: u32) {
        let selected = item.itemState & ODS_SELECTED != 0;
        let disabled = item.itemState & ODS_DISABLED != 0;
        let (background, foreground) = if self.high_contrast {
            unsafe {
                (
                    GetSysColor(if selected {
                        COLOR_HIGHLIGHT
                    } else {
                        COLOR_WINDOW
                    }),
                    GetSysColor(if selected {
                        COLOR_HIGHLIGHTTEXT
                    } else {
                        COLOR_WINDOWTEXT
                    }),
                )
            }
        } else if selected {
            (self.accent, rgb(255, 255, 255))
        } else {
            (self.field, if disabled { self.muted } else { self.text })
        };
        unsafe {
            let dc = item.hDC;
            let saved = SaveDC(dc);
            SetDCBrushColor(dc, background);
            FillRect(dc, &item.rcItem, GetStockObject(DC_BRUSH) as HBRUSH);
            if item.itemID != u32::MAX {
                let len = SendMessageW(item.hwndItem, CB_GETLBTEXTLEN, item.itemID as usize, 0);
                if len >= 0 {
                    let mut label = vec![0u16; len as usize + 1];
                    SendMessageW(
                        item.hwndItem,
                        CB_GETLBTEXT,
                        item.itemID as usize,
                        label.as_mut_ptr() as isize,
                    );
                    SelectObject(dc, SendMessageW(item.hwndItem, WM_GETFONT, 0, 0) as HFONT);
                    SetTextColor(dc, foreground);
                    SetBkMode(dc, TRANSPARENT as i32);
                    let mut rect = item.rcItem;
                    rect.left += scaled(6, dpi);
                    rect.right -= scaled(6, dpi);
                    DrawTextW(
                        dc,
                        label.as_ptr(),
                        -1,
                        &mut rect,
                        DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX | DT_END_ELLIPSIS,
                    );
                }
            }
            if item.itemState & ODS_FOCUS != 0 {
                DrawFocusRect(dc, &item.rcItem);
            }
            RestoreDC(dc, saved);
        }
    }
}

fn status_colors(dark: bool, state: &State) -> (COLORREF, COLORREF) {
    match (dark, state) {
        (false, State::Authenticated) => (rgb(22, 101, 52), rgb(240, 253, 244)),
        (false, State::Authenticating) => (rgb(146, 64, 14), rgb(255, 251, 235)),
        (false, State::Failed) => (rgb(185, 28, 28), rgb(254, 242, 242)),
        (false, _) => (rgb(71, 85, 105), rgb(241, 245, 249)),
        (true, State::Authenticated) => (rgb(74, 222, 128), rgb(5, 46, 22)),
        (true, State::Authenticating) => (rgb(251, 191, 36), rgb(69, 26, 3)),
        (true, State::Failed) => (rgb(248, 113, 113), rgb(69, 10, 10)),
        (true, _) => (rgb(203, 213, 225), rgb(30, 41, 59)),
    }
}

impl Drop for Theme {
    fn drop(&mut self) {
        unsafe {
            DeleteObject(self.background_brush);
            DeleteObject(self.field_brush);
        }
    }
}

unsafe extern "system" fn control_proc(
    hwnd: HWND,
    message: u32,
    wparam: usize,
    lparam: isize,
    kind: usize,
    data: usize,
) -> isize {
    let theme = unsafe { &*(data as *const Theme) };
    if !theme.high_contrast && message == WM_ERASEBKGND {
        // The parent paints behind group frames; other controls paint their full area.
        return 1;
    }
    if !theme.high_contrast && matches!(message, WM_PAINT | WM_PRINTCLIENT) {
        unsafe {
            let mut paint: PAINTSTRUCT = mem::zeroed();
            let dc = if message == WM_PAINT {
                BeginPaint(hwnd, &mut paint)
            } else {
                wparam as HDC
            };
            let saved = SaveDC(dc);
            let font = SendMessageW(hwnd, WM_GETFONT, 0, 0) as HFONT;
            if !font.is_null() {
                SelectObject(dc, font);
            }
            let mut rect: RECT = mem::zeroed();
            GetClientRect(hwnd, &mut rect);
            // GetDpiForWindow also follows monitor changes for the native input control.
            let dpi = windows_sys::Win32::UI::HiDpi::GetDpiForWindow(hwnd).max(96);
            if kind == 1 {
                theme.button(hwnd, dc, rect, dpi);
            } else {
                theme.combo(hwnd, dc, rect, dpi);
            }
            RestoreDC(dc, saved);
            if message == WM_PAINT {
                EndPaint(hwnd, &paint);
            }
            return 0;
        }
    }
    if message == WM_NCDESTROY {
        unsafe {
            RemoveWindowSubclass(hwnd, Some(control_proc), kind);
        }
    }
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(color: COLORREF) -> f64 {
        [0, 8, 16]
            .into_iter()
            .zip([0.2126, 0.7152, 0.0722])
            .map(|(shift, weight)| {
                let channel = ((color >> shift) & 255) as f64 / 255.0;
                weight
                    * if channel <= 0.04045 {
                        channel / 12.92
                    } else {
                        ((channel + 0.055) / 1.055).powf(2.4)
                    }
            })
            .sum()
    }

    #[test]
    fn status_text_is_readable_in_both_themes() {
        for dark in [false, true] {
            for state in [
                State::Idle,
                State::Authenticating,
                State::Authenticated,
                State::Failed,
                State::Stopped,
            ] {
                let (foreground, background) = status_colors(dark, &state);
                let a = luminance(foreground);
                let b = luminance(background);
                assert!(
                    (a.max(b) + 0.05) / (a.min(b) + 0.05) >= 4.5,
                    "{state:?} dark={dark}"
                );
            }
        }
    }
}
