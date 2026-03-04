//! Windows 窗口移动桥接：将“右键拖动”转为系统标题栏拖动。
//! 做法：注入左键按下(SendInput) → ReleaseCapture + WM_SYSCOMMAND SC_MOVE → 主线程在系统模态拖动中阻塞；
//! 后台线程轮询 VK_RBUTTON，一旦松开则注入左键松开，系统结束拖动，主线程返回。由系统接管移动，无抖动。

#![cfg(windows)]

use std::thread;
use std::time::Duration;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, ReleaseCapture, SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEINPUT,
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, VK_RBUTTON,
};
use windows::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_SYSCOMMAND};

/// 系统命令：SC_MOVE | 0x0002（开始“拖标题栏”式移动）
const SC_DRAGMOVE: u32 = 0xF012;

/// 右键拖动超过阈值时调用：注入左键按下 → 发送 SC_MOVE，主线程阻塞直到系统拖动结束；
/// 后台线程在检测到右键松开时注入左键松开，系统结束拖动。
pub fn start_system_window_drag(hwnd: isize) {
    let hwnd = HWND(hwnd as _);

    // 后台线程：轮询右键是否松开，松开则注入左键松开，让系统结束模态拖动
    let handle = thread::spawn(|| {
        loop {
            thread::sleep(Duration::from_millis(10));
            let released = unsafe {
                let state = GetAsyncKeyState(VK_RBUTTON.0 as i32);
                (state as u16) & 0x8000 == 0
            };
            if released {
                unsafe {
                    let input = INPUT {
                        r#type: INPUT_MOUSE,
                        Anonymous: INPUT_0 {
                            mi: MOUSEINPUT {
                                dx: 0,
                                dy: 0,
                                mouseData: 0,
                                dwFlags: MOUSEEVENTF_LEFTUP,
                                time: 0,
                                dwExtraInfo: 0,
                            },
                        },
                    };
                    let _ = SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
                }
                break;
            }
        }
    });

    unsafe {
        // 注入左键按下，使系统认为“左键在标题栏按下”
        let input_down = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: 0,
                    dwFlags: MOUSEEVENTF_LEFTDOWN,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let _ = SendInput(&[input_down], std::mem::size_of::<INPUT>() as i32);

        let _ = ReleaseCapture();
        let _ = SendMessageW(hwnd, WM_SYSCOMMAND, WPARAM(SC_DRAGMOVE as usize), LPARAM(0));
    }

    let _ = handle.join();
}
