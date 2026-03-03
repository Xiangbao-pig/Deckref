//! Windows 原生拖放：支持 CF_HDROP 与 FileGroupDescriptorW + FileContents。
//! 可接收来自常见浏览器的拖入：Chrome、Edge、Brave、Opera 等 Chromium 内核，以及 IE
//! （凡使用上述 OLE 格式的均可）。仅编译于 Windows；通过 channel 将得到的路径发回主线程。

#![cfg(windows)]

use std::path::PathBuf;
use std::sync::mpsc;
use windows::core::implement;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
    IDataObject, IStream, STATFLAG, STATSTG, TYMED_HGLOBAL, TYMED_ISTREAM,
    DVASPECT_CONTENT, FORMATETC,
};
use windows::Win32::System::Ole::{
    DROPEFFECT_COPY, OleInitialize, RegisterDragDrop, RevokeDragDrop, IDropTarget, ReleaseStgMedium,
};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
use windows::Win32::Foundation::POINTL;
use windows::core::PCWSTR;

const CFSTR_FILEDESCRIPTORW: &[u16] = &[
    'F' as u16, 'i' as u16, 'l' as u16, 'e' as u16, 'G' as u16, 'r' as u16, 'o' as u16, 'u' as u16,
    'p' as u16, 'D' as u16, 'e' as u16, 's' as u16, 'c' as u16, 'r' as u16, 'i' as u16, 'p' as u16,
    't' as u16, 'o' as u16, 'r' as u16, 'W' as u16, 0,
];
const CFSTR_FILECONTENTS: &[u16] = &[
    'F' as u16, 'i' as u16, 'l' as u16, 'e' as u16, 'C' as u16, 'o' as u16, 'n' as u16, 't' as u16,
    'e' as u16, 'n' as u16, 't' as u16, 's' as u16, 0,
];

fn cf_hdrop() -> u16 {
    15
}

/// 从 IDataObject 提取 CF_HDROP 的所有路径。
fn extract_hdrop_paths(data_obj: &IDataObject) -> Vec<PathBuf> {
        let format = FORMATETC {
        cfFormat: cf_hdrop(),
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    };
    unsafe {
        let Ok(mut medium) = data_obj.GetData(&format) else {
            return vec![];
        };
        if medium.tymed != TYMED_HGLOBAL.0 as u32 {
            ReleaseStgMedium(&mut medium);
            return vec![];
        }
        let hglobal = medium.u.hGlobal;
        let hdrop = HDROP(hglobal.0 as isize);
        let n = DragQueryFileW(hdrop, 0xFFFFFFFF, None);
        let mut paths = Vec::with_capacity(n as usize);
        for i in 0..n {
            let len = DragQueryFileW(hdrop, i, None);
            if len == 0 {
                continue;
            }
            let mut buf = vec![0u16; (len + 1) as usize];
            let _ = DragQueryFileW(hdrop, i, Some(&mut buf));
            if let Ok(s) = String::from_utf16(&buf[..len as usize]) {
                paths.push(PathBuf::from(s));
            }
        }
        ReleaseStgMedium(&mut medium);
        paths
    }
}

/// FILEGROUPDESCRIPTOR 布局：cItems (u32) + FILEDESCRIPTORW[]。FILEDESCRIPTORW 大小为 592 字节。
const SIZE_OF_FILEDESCRIPTOR: usize = 592;

#[repr(C)]
struct FileDescriptorW {
    dw_flags: u32,
    _clsid: [u8; 16],
    _sizel: [u8; 8],
    _pointl: [u8; 8],
    dw_file_attributes: u32,
    _ft_creation: [u8; 8],
    _ft_last_access: [u8; 8],
    _ft_last_write: [u8; 8],
    n_file_size_high: u32,
    n_file_size_low: u32,
    c_file_name: [u16; 260],
}

/// 从 IDataObject 提取 FileGroupDescriptorW + FileContents，写入 dest_dir，通过 tx 发送路径。
fn extract_file_contents_paths(
    data_obj: &IDataObject,
    dest_dir: &std::path::Path,
    tx: &mpsc::Sender<PathBuf>,
) {
    unsafe {
        let cf_fgd = RegisterClipboardFormatW(PCWSTR(CFSTR_FILEDESCRIPTORW.as_ptr()));
        let cf_fc = RegisterClipboardFormatW(PCWSTR(CFSTR_FILECONTENTS.as_ptr()));
        if cf_fgd == 0 || cf_fc == 0 {
            return;
        }

        let format_fgd = FORMATETC {
            cfFormat: cf_fgd as u16,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        };
        let Ok(mut medium_fgd) = data_obj.GetData(&format_fgd) else {
            return;
        };
        if medium_fgd.tymed != TYMED_HGLOBAL.0 as u32 {
            ReleaseStgMedium(&mut medium_fgd);
            return;
        }

        let hglobal = medium_fgd.u.hGlobal;
        let ptr = windows::Win32::System::Memory::GlobalLock(hglobal) as *const u8;
        if ptr.is_null() {
            ReleaseStgMedium(&mut medium_fgd);
            return;
        }
        let c_items = (ptr as *const u32).read();
        let _ = windows::Win32::System::Memory::GlobalUnlock(hglobal);

        let base = ptr.add(4);
        for idx in 0..(c_items as isize) {
            let fd_ptr = base.add(idx as usize * SIZE_OF_FILEDESCRIPTOR) as *const FileDescriptorW;
            let name_slice = std::slice::from_raw_parts((*fd_ptr).c_file_name.as_ptr(), 260);
            let len = name_slice.iter().position(|&c| c == 0).unwrap_or(260);
            let _name = String::from_utf16_lossy(&name_slice[..len]);
            let dest_path = crate::image_ops::unique_path_for_drop(dest_dir, idx as u32);

            let mut format_fc = FORMATETC {
                cfFormat: cf_fc as u16,
                ptd: std::ptr::null_mut(),
                dwAspect: DVASPECT_CONTENT.0,
                lindex: idx as i32,
                tymed: TYMED_ISTREAM.0 as u32,
            };
            let Ok(mut medium_fc) = data_obj.GetData(&format_fc) else {
                format_fc.tymed = TYMED_HGLOBAL.0 as u32;
                let Ok(mut medium_fc) = data_obj.GetData(&format_fc) else {
                    continue;
                };
                if medium_fc.tymed != TYMED_HGLOBAL.0 as u32 {
                    ReleaseStgMedium(&mut medium_fc);
                    continue;
                }
                let h = medium_fc.u.hGlobal;
                let p = windows::Win32::System::Memory::GlobalLock(h) as *const u8;
                let size = windows::Win32::System::Memory::GlobalSize(h) as usize;
                let bytes = std::slice::from_raw_parts(p, size).to_vec();
                let _ = windows::Win32::System::Memory::GlobalUnlock(h);
                ReleaseStgMedium(&mut medium_fc);
                if crate::image_ops::save_image_from_bytes(&bytes, &dest_path).is_ok() {
                    let _ = tx.send(dest_path);
                }
                continue;
            };

            let bytes = if medium_fc.tymed == TYMED_ISTREAM.0 as u32 {
                let pstm = std::mem::ManuallyDrop::into_inner(std::ptr::read(&medium_fc.u.pstm));
                let stream = match pstm {
                    Some(s) => s,
                    None => {
                        ReleaseStgMedium(&mut medium_fc);
                        continue;
                    }
                };
                let bytes = read_istream_to_bytes(&stream);
                std::mem::forget(stream);
                bytes
            } else if medium_fc.tymed == TYMED_HGLOBAL.0 as u32 {
                let h = medium_fc.u.hGlobal;
                let p = windows::Win32::System::Memory::GlobalLock(h) as *const u8;
                let size = windows::Win32::System::Memory::GlobalSize(h) as usize;
                let bytes = std::slice::from_raw_parts(p, size).to_vec();
                let _ = windows::Win32::System::Memory::GlobalUnlock(h);
                bytes
            } else {
                ReleaseStgMedium(&mut medium_fc);
                continue;
            };
            ReleaseStgMedium(&mut medium_fc);

            if crate::image_ops::save_image_from_bytes(&bytes, &dest_path).is_ok() {
                let _ = tx.send(dest_path);
            }
        }
        ReleaseStgMedium(&mut medium_fgd);
    }
}

fn read_istream_to_bytes(stream: &IStream) -> Vec<u8> {
    use std::ptr::addr_of_mut;
    unsafe {
        let mut stat = STATSTG::default();
        if stream.Stat(addr_of_mut!(stat), STATFLAG(0)).is_err() {
            return vec![];
        }
        let size = stat.cbSize as usize;
        if size == 0 || size > 50 * 1024 * 1024 {
            return vec![];
        }
        let mut buf = vec![0u8; size];
        let mut read = 0u32;
        let ok = stream
            .Read(
                buf.as_mut_ptr() as *mut _,
                size as u32,
                Some(addr_of_mut!(read)),
            )
            .is_ok();
        if ok {
            buf.truncate(read as usize);
            buf
        } else {
            vec![]
        }
    }
}

#[implement(IDropTarget)]
struct WinDropTarget {
    sender: mpsc::Sender<PathBuf>,
    watch_path: PathBuf,
}

impl windows::Win32::System::Ole::IDropTarget_Impl for WinDropTarget {
    fn DragEnter(
        &self,
        _data_obj: Option<&IDataObject>,
        _grf_key_state: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        pdw_effect: *mut windows::Win32::System::Ole::DROPEFFECT,
    ) -> windows::core::Result<()> {
        if !pdw_effect.is_null() {
            unsafe { *pdw_effect = DROPEFFECT_COPY };
        }
        Ok(())
    }

    fn DragOver(
        &self,
        _grf_key_state: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        pdw_effect: *mut windows::Win32::System::Ole::DROPEFFECT,
    ) -> windows::core::Result<()> {
        if !pdw_effect.is_null() {
            unsafe { *pdw_effect = DROPEFFECT_COPY };
        }
        Ok(())
    }

    fn DragLeave(&self) -> windows::core::Result<()> {
        Ok(())
    }

    fn Drop(
        &self,
        data_obj: Option<&IDataObject>,
        _grf_key_state: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        pdw_effect: *mut windows::Win32::System::Ole::DROPEFFECT,
    ) -> windows::core::Result<()> {
        if !pdw_effect.is_null() {
            unsafe { *pdw_effect = DROPEFFECT_COPY };
        }
        let Some(data_obj) = data_obj else {
            return Ok(());
        };

        let hdrop_paths = extract_hdrop_paths(data_obj);
        for path in &hdrop_paths {
            let _ = self.sender.send(path.clone());
        }
        // 若已有 CF_HDROP 路径（如从本窗口拖出又拖回），不再用 FileContents 写新文件，避免重复保存
        if hdrop_paths.is_empty() {
            extract_file_contents_paths(data_obj, &self.watch_path, &self.sender);
        }
        Ok(())
    }
}

/// 注册窗口的原生拖放目标；返回的 guard 在 drop 时撤销注册。
/// 若窗口已被 winit 等注册，先 RevokeDragDrop 再注册本实现（以支持 Chrome 的 FileGroupDescriptorW + FileContents）。
pub fn register_drop_target(
    hwnd: isize,
    sender: mpsc::Sender<PathBuf>,
    watch_path: PathBuf,
) -> Result<DropTargetGuard, windows::core::Error> {
    unsafe {
        let _ = OleInitialize(None);
    }
    let hwnd = HWND(hwnd as _);
    unsafe {
        let _ = RevokeDragDrop(hwnd);
    }
    let target = WinDropTarget {
        sender,
        watch_path,
    };
    let target: IDropTarget = target.into();
    unsafe {
        RegisterDragDrop(hwnd, &target)?;
    }
    Ok(DropTargetGuard {
        hwnd,
        _target: target,
    })
}

pub struct DropTargetGuard {
    hwnd: HWND,
    _target: IDropTarget,
}

impl Drop for DropTargetGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = RevokeDragDrop(self.hwnd);
        }
    }
}
