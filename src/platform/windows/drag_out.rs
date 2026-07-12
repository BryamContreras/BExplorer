use std::path::PathBuf;

use crate::utils::errors::{BExplorerError, Result};

pub fn start_file_drag(paths: Vec<PathBuf>) -> Result<()> {
    run_file_drag(paths)
}

pub fn release_mouse_capture() {
    unsafe {
        let _ = windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture();
    }
}

fn run_file_drag(paths: Vec<PathBuf>) -> Result<()> {
    use windows::Win32::Foundation::{DRAGDROP_S_CANCEL, DRAGDROP_S_DROP};
    use windows::Win32::System::Com::IDataObject;
    use windows::Win32::System::Ole::{DROPEFFECT_COPY, DROPEFFECT_NONE, DoDragDrop, IDropSource};

    if paths.is_empty() {
        return Err(BExplorerError::Shell("No files to drag".into()));
    }

    let _ole = OleDragGuard::new()?;
    let data_object: IDataObject = NativeFileDataObject {
        paths: paths.clone(),
    }
    .into();
    let drop_source: IDropSource = NativeFileDropSource.into();
    let mut effect = DROPEFFECT_NONE;

    let result = unsafe { DoDragDrop(&data_object, &drop_source, DROPEFFECT_COPY, &mut effect) };
    if result == DRAGDROP_S_CANCEL {
        return Ok(());
    }
    if result != DRAGDROP_S_DROP && result.is_err() {
        return Err(BExplorerError::Shell(format!(
            "Native drag failed: {}",
            result.message()
        )));
    }

    Ok(())
}

#[windows::core::implement(windows::Win32::System::Com::IDataObject)]
struct NativeFileDataObject {
    paths: Vec<PathBuf>,
}

#[allow(non_snake_case)]
impl windows::Win32::System::Com::IDataObject_Impl for NativeFileDataObject_Impl {
    fn GetData(
        &self,
        format: *const windows::Win32::System::Com::FORMATETC,
    ) -> windows::core::Result<windows::Win32::System::Com::STGMEDIUM> {
        use core::mem::ManuallyDrop;
        use windows::Win32::System::Com::{STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL};

        native_file_data_query(format).ok()?;
        let hglobal = native_file_hdrop_memory(&self.paths)?;
        Ok(STGMEDIUM {
            tymed: TYMED_HGLOBAL.0 as u32,
            u: STGMEDIUM_0 { hGlobal: hglobal },
            pUnkForRelease: ManuallyDrop::new(None),
        })
    }

    fn GetDataHere(
        &self,
        _format: *const windows::Win32::System::Com::FORMATETC,
        _medium: *mut windows::Win32::System::Com::STGMEDIUM,
    ) -> windows::core::Result<()> {
        Err(windows::core::Error::from(
            windows::Win32::Foundation::E_NOTIMPL,
        ))
    }

    fn QueryGetData(
        &self,
        format: *const windows::Win32::System::Com::FORMATETC,
    ) -> windows::core::HRESULT {
        native_file_data_query(format)
    }

    fn GetCanonicalFormatEtc(
        &self,
        _input: *const windows::Win32::System::Com::FORMATETC,
        _output: *mut windows::Win32::System::Com::FORMATETC,
    ) -> windows::core::HRESULT {
        windows::Win32::Foundation::E_NOTIMPL
    }

    fn SetData(
        &self,
        _format: *const windows::Win32::System::Com::FORMATETC,
        _medium: *const windows::Win32::System::Com::STGMEDIUM,
        _release: windows::Win32::Foundation::BOOL,
    ) -> windows::core::Result<()> {
        Err(windows::core::Error::from(
            windows::Win32::Foundation::E_NOTIMPL,
        ))
    }

    fn EnumFormatEtc(
        &self,
        direction: u32,
    ) -> windows::core::Result<windows::Win32::System::Com::IEnumFORMATETC> {
        use windows::Win32::Foundation::E_NOTIMPL;
        use windows::Win32::System::Com::{DATADIR_GET, FORMATETC};
        use windows::Win32::UI::Shell::SHCreateStdEnumFmtEtc;

        if direction != DATADIR_GET.0 as u32 {
            return Err(windows::core::Error::from(E_NOTIMPL));
        }

        let format = native_file_format_etc();
        let formats: [FORMATETC; 1] = [format];
        unsafe { SHCreateStdEnumFmtEtc(&formats) }
    }

    fn DAdvise(
        &self,
        _format: *const windows::Win32::System::Com::FORMATETC,
        _advf: u32,
        _sink: Option<&windows::Win32::System::Com::IAdviseSink>,
    ) -> windows::core::Result<u32> {
        Err(windows::core::Error::from(
            windows::Win32::Foundation::OLE_E_ADVISENOTSUPPORTED,
        ))
    }

    fn DUnadvise(&self, _connection: u32) -> windows::core::Result<()> {
        Err(windows::core::Error::from(
            windows::Win32::Foundation::OLE_E_ADVISENOTSUPPORTED,
        ))
    }

    fn EnumDAdvise(&self) -> windows::core::Result<windows::Win32::System::Com::IEnumSTATDATA> {
        Err(windows::core::Error::from(
            windows::Win32::Foundation::OLE_E_ADVISENOTSUPPORTED,
        ))
    }
}

fn native_file_data_query(
    format: *const windows::Win32::System::Com::FORMATETC,
) -> windows::core::HRESULT {
    use windows::Win32::Foundation::{DV_E_FORMATETC, DV_E_TYMED, E_INVALIDARG, S_OK};
    use windows::Win32::System::Com::TYMED_HGLOBAL;

    if format.is_null() {
        return E_INVALIDARG;
    }

    let expected = native_file_format_etc();
    let format = unsafe { *format };
    if format.cfFormat != expected.cfFormat
        || format.dwAspect != expected.dwAspect
        || format.lindex != expected.lindex
    {
        return DV_E_FORMATETC;
    }
    if format.tymed & TYMED_HGLOBAL.0 as u32 == 0 {
        return DV_E_TYMED;
    }

    S_OK
}

fn native_file_format_etc() -> windows::Win32::System::Com::FORMATETC {
    use windows::Win32::System::Com::{DVASPECT_CONTENT, FORMATETC, TYMED_HGLOBAL};
    use windows::Win32::System::Ole::CF_HDROP;

    FORMATETC {
        cfFormat: CF_HDROP.0,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    }
}

fn native_file_hdrop_memory(
    paths: &[PathBuf],
) -> windows::core::Result<windows::Win32::Foundation::HGLOBAL> {
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;

    use windows::Win32::Foundation::{BOOL, GlobalFree, POINT};
    use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
    use windows::Win32::UI::Shell::DROPFILES;

    let mut encoded_paths = Vec::new();
    for path in paths {
        let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        if wide.is_empty() {
            continue;
        }
        wide.push(0);
        encoded_paths.extend(wide);
    }
    encoded_paths.push(0);

    let header_size = size_of::<DROPFILES>();
    let bytes_len = header_size + encoded_paths.len() * size_of::<u16>();
    let hdrop = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes_len) }?;
    let drop_ptr = unsafe { GlobalLock(hdrop) } as *mut u8;
    if drop_ptr.is_null() {
        unsafe {
            let _ = GlobalFree(hdrop);
        }
        return Err(windows::core::Error::from_win32());
    }

    unsafe {
        let drop_files = DROPFILES {
            pFiles: header_size as u32,
            pt: POINT { x: 0, y: 0 },
            fNC: BOOL(0),
            fWide: BOOL(1),
        };
        ptr::write_unaligned(drop_ptr as *mut DROPFILES, drop_files);
        ptr::copy_nonoverlapping(
            encoded_paths.as_ptr() as *const u8,
            drop_ptr.add(header_size),
            encoded_paths.len() * size_of::<u16>(),
        );
        let _ = GlobalUnlock(hdrop);
    }

    Ok(hdrop)
}

#[windows::core::implement(windows::Win32::System::Ole::IDropSource)]
struct NativeFileDropSource;

#[allow(non_snake_case)]
impl windows::Win32::System::Ole::IDropSource_Impl for NativeFileDropSource_Impl {
    fn QueryContinueDrag(
        &self,
        escape_pressed: windows::Win32::Foundation::BOOL,
        key_state: windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS,
    ) -> windows::core::HRESULT {
        use windows::Win32::Foundation::{DRAGDROP_S_CANCEL, DRAGDROP_S_DROP};
        use windows::Win32::System::SystemServices::{MK_LBUTTON, MK_RBUTTON};

        if escape_pressed.as_bool() {
            return DRAGDROP_S_CANCEL;
        }
        if !key_state.contains(MK_LBUTTON) && !key_state.contains(MK_RBUTTON) {
            return DRAGDROP_S_DROP;
        }
        windows::core::HRESULT(0)
    }

    fn GiveFeedback(
        &self,
        _effect: windows::Win32::System::Ole::DROPEFFECT,
    ) -> windows::core::HRESULT {
        windows::Win32::Foundation::DRAGDROP_S_USEDEFAULTCURSORS
    }
}

struct OleDragGuard {
    uninitialize: bool,
}

impl OleDragGuard {
    fn new() -> Result<Self> {
        use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
        use windows::Win32::System::Ole::OleInitialize;

        match unsafe { OleInitialize(None) } {
            Ok(()) => Ok(Self { uninitialize: true }),
            Err(error) if error.code() == RPC_E_CHANGED_MODE => Err(BExplorerError::Shell(
                "Native drag requires a Windows STA thread, but this thread is already using a different COM mode".into(),
            )),
            Err(error) => Err(BExplorerError::Shell(format!(
                "Could not initialize OLE drag and drop: {error}"
            ))),
        }
    }
}

impl Drop for OleDragGuard {
    fn drop(&mut self) {
        if self.uninitialize {
            unsafe {
                windows::Win32::System::Ole::OleUninitialize();
            }
        }
    }
}
