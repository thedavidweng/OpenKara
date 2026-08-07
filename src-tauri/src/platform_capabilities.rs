use std::sync::atomic::{AtomicBool, Ordering};

static DIRECTML_DISABLED_BY_TIMEOUT: AtomicBool = AtomicBool::new(false);

pub fn set_directml_disabled_by_timeout(disabled: bool) {
    DIRECTML_DISABLED_BY_TIMEOUT.store(disabled, Ordering::SeqCst);
}

pub fn directml_disabled_by_timeout() -> bool {
    DIRECTML_DISABLED_BY_TIMEOUT.load(Ordering::SeqCst)
}

#[cfg(target_os = "windows")]
fn probe_directml_available() -> bool {
    use windows::Win32::Graphics::{
        Direct3D::D3D_FEATURE_LEVEL_11_0,
        Direct3D12::{D3D12CreateDevice, ID3D12Device},
        Dxgi::{
            CreateDXGIFactory2, IDXGIAdapter1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE,
            DXGI_CREATE_FACTORY_FLAGS, DXGI_ERROR_NOT_FOUND,
        },
    };

    // SAFETY: DXGI creates and owns the COM interface returned through the
    // typed Windows binding.
    let factory = match unsafe {
        CreateDXGIFactory2::<IDXGIFactory1>(DXGI_CREATE_FACTORY_FLAGS::default())
    } {
        Ok(factory) => factory,
        Err(_) => return false,
    };

    let mut index = 0;
    loop {
        // SAFETY: `factory` is a live IDXGIFactory1 and `index` is an adapter
        // enumeration index owned by this loop.
        let adapter = match unsafe { factory.EnumAdapters1(index) } {
            Ok(adapter) => adapter,
            Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => return false,
            Err(_) => return false,
        };
        index += 1;

        // SAFETY: `adapter` is a live IDXGIAdapter1 returned by DXGI.
        let description = match unsafe { adapter.GetDesc1() } {
            Ok(description) => description,
            Err(_) => continue,
        };
        if description.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
            continue;
        }

        let mut device = None::<ID3D12Device>;
        // SAFETY: `adapter` is a live hardware adapter and `device` points to
        // storage for the interface requested by the Windows API.
        if unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }.is_ok()
            && device.is_some()
        {
            return true;
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn probe_directml_available() -> bool {
    false
}

pub fn directml_available() -> bool {
    !directml_disabled_by_timeout() && probe_directml_available()
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub const fn coreml_available() -> bool {
    true
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
pub const fn coreml_available() -> bool {
    false
}

pub const fn xnnpack_available() -> bool {
    cfg!(any(
        all(target_os = "macos", target_arch = "x86_64"),
        all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    ))
}
