/// Show the shutdown-countdown dialog.
///
/// * Returns `true`  → proceed with the update (countdown expired or "Do It Now" clicked).
/// * Returns `false` → user clicked "Abort".
///
/// On non-Windows platforms the function is a no-op that always returns `true`.
pub fn show_countdown_dialog() -> bool {
    #[cfg(windows)]
    return imp::show();

    #[cfg(not(windows))]
    return true;
}

// ── Windows implementation ────────────────────────────────────────────────────
#[cfg(windows)]
mod imp {
    use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::GetSysColorBrush;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        BS_PUSHBUTTON, CreateWindowExW, DefWindowProcW, DispatchMessageW, GetDlgItem,
        GetMessageW, GetSystemMetrics, IDC_ARROW, KillTimer, LoadCursorW, MSG, PostMessageW,
        PostQuitMessage, RegisterClassExW, SetTimer, SetWindowTextW, ShowWindow,
        TranslateMessage, WM_CLOSE, WM_COMMAND, WM_DESTROY, WM_TIMER, WNDCLASSEXW,
        WS_CAPTION, WS_CHILD, WS_SYSMENU, WS_VISIBLE, SW_SHOW, SM_CXSCREEN, SM_CYSCREEN,
        CS_HREDRAW, CS_VREDRAW,
    };

    // Raw Win32 constants not separately exported by windows-sys in this config
    const SS_CENTER: u32 = 0x0000_0001;
    const WS_EX_TOPMOST: u32 = 0x0000_0008;

    const TIMER_ID: usize = 1;
    const ID_LABEL: i32 = 102;
    const ID_BTN_DO_NOW: i32 = 100;
    const ID_BTN_ABORT: i32 = 101;

    // COLOR_BTNFACE (SYS_COLOR_INDEX = 15)
    const COLOR_BTNFACE: i32 = 15;

    // Global state shared between the window procedure and the calling thread.
    static SHOULD_PROCEED: AtomicBool = AtomicBool::new(false);
    static COUNTDOWN: AtomicI32 = AtomicI32::new(10);

    fn to_wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_TIMER => {
                let new_count = COUNTDOWN.fetch_sub(1, Ordering::SeqCst) - 1;
                if new_count <= 0 {
                    KillTimer(hwnd, TIMER_ID);
                    SHOULD_PROCEED.store(true, Ordering::SeqCst);
                    PostMessageW(hwnd, WM_CLOSE, 0, 0);
                } else {
                    let label_hwnd = GetDlgItem(hwnd, ID_LABEL);
                    let text =
                        to_wide(&format!("Steam is shutting down in {}...", new_count));
                    SetWindowTextW(label_hwnd, text.as_ptr());
                }
                0
            }
            WM_COMMAND => {
                let ctrl_id = (wparam & 0xFFFF) as i32;
                match ctrl_id {
                    ID_BTN_DO_NOW => {
                        KillTimer(hwnd, TIMER_ID);
                        SHOULD_PROCEED.store(true, Ordering::SeqCst);
                        PostMessageW(hwnd, WM_CLOSE, 0, 0);
                        0
                    }
                    ID_BTN_ABORT => {
                        KillTimer(hwnd, TIMER_ID);
                        SHOULD_PROCEED.store(false, Ordering::SeqCst);
                        PostMessageW(hwnd, WM_CLOSE, 0, 0);
                        0
                    }
                    _ => DefWindowProcW(hwnd, msg, wparam, lparam),
                }
            }
            // WM_CLOSE is sent either by the X button or posted programmatically
            // by the timer / "Do It Now" after they have already set SHOULD_PROCEED.
            // Do NOT touch SHOULD_PROCEED here — it is already correct:
            //   • timer / Do It Now: set it true before posting WM_CLOSE
            //   • X button: left at its initial false value
            WM_CLOSE => {
                KillTimer(hwnd, TIMER_ID);
                // Let DefWindowProcW call DestroyWindow, which posts WM_DESTROY.
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }

    pub fn show() -> bool {
        SHOULD_PROCEED.store(false, Ordering::SeqCst);
        COUNTDOWN.store(10, Ordering::SeqCst);

        unsafe {
            let hinstance = GetModuleHandleW(std::ptr::null());
            let class_name = to_wide("SteamUpdaterDlg");

            // Register the window class (ignore if already registered)
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(wnd_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: hinstance,
                hIcon: std::ptr::null_mut(),
                hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
                hbrBackground: GetSysColorBrush(COLOR_BTNFACE),
                lpszMenuName: std::ptr::null(),
                lpszClassName: class_name.as_ptr(),
                hIconSm: std::ptr::null_mut(),
            };
            RegisterClassExW(&wc);

            // Window size — centred on screen
            let w = 400i32;
            let h = 160i32;
            let screen_w = GetSystemMetrics(SM_CXSCREEN);
            let screen_h = GetSystemMetrics(SM_CYSCREEN);
            let x = (screen_w - w) / 2;
            let y = (screen_h - h) / 2;

            let title = to_wide("steam-updater");
            // Fixed dialog: caption + system menu, no resize/maximize/minimize
            let style: u32 = WS_CAPTION | WS_SYSMENU;

            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST,
                class_name.as_ptr(),
                title.as_ptr(),
                style,
                x,
                y,
                w,
                h,
                std::ptr::null_mut(), // no parent
                std::ptr::null_mut(), // no menu
                hinstance,
                std::ptr::null(),
            );

            // Static label (countdown text), centred horizontally
            let static_class = to_wide("STATIC");
            let label_text = to_wide("Steam is shutting down in 10...");
            let label_style: u32 = WS_CHILD | WS_VISIBLE | SS_CENTER;
            CreateWindowExW(
                0,
                static_class.as_ptr(),
                label_text.as_ptr(),
                label_style,
                20,
                20,
                360,
                50,
                hwnd,
                ID_LABEL as isize as *mut core::ffi::c_void,
                hinstance,
                std::ptr::null(),
            );

            // "Do It Now" button
            let btn_class = to_wide("BUTTON");
            let do_now_text = to_wide("Do It Now");
            let btn_style: u32 = WS_CHILD | WS_VISIBLE | (BS_PUSHBUTTON as u32);
            CreateWindowExW(
                0,
                btn_class.as_ptr(),
                do_now_text.as_ptr(),
                btn_style,
                60,
                82,
                120,
                34,
                hwnd,
                ID_BTN_DO_NOW as isize as *mut core::ffi::c_void,
                hinstance,
                std::ptr::null(),
            );

            // "Abort" button
            let abort_text = to_wide("Abort");
            CreateWindowExW(
                0,
                btn_class.as_ptr(),
                abort_text.as_ptr(),
                btn_style,
                210,
                82,
                120,
                34,
                hwnd,
                ID_BTN_ABORT as isize as *mut core::ffi::c_void,
                hinstance,
                std::ptr::null(),
            );

            // Start the 1-second countdown timer and show the window
            SetTimer(hwnd, TIMER_ID, 1000, None);
            ShowWindow(hwnd, SW_SHOW);

            // Standard Win32 message loop
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        SHOULD_PROCEED.load(Ordering::SeqCst)
    }
}
