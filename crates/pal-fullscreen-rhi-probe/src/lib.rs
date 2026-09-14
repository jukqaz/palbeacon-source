use std::{
    ffi::c_void,
    mem::{self, ManuallyDrop},
    panic::{self, AssertUnwindSafe},
    ptr,
    sync::atomic::{AtomicPtr, AtomicUsize, Ordering},
    thread,
    time::{Duration, Instant},
};

use hudhook::{
    hooks::DummyHwnd,
    mh::{MH_ApplyQueued, MH_Initialize, MH_Uninitialize, MhHook},
    util,
    windows::{
        Win32::{
            Foundation::{HMODULE, RECT},
            Graphics::{
                Direct3D::{
                    D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_10_0, D3D_FEATURE_LEVEL_11_0,
                },
                Direct3D11::{
                    D3D11_CREATE_DEVICE_FLAG, D3D11_SDK_VERSION, D3D11CreateDeviceAndSwapChain,
                    ID3D11Device, ID3D11DeviceContext,
                },
                Direct3D12::{
                    D3D12_COMMAND_LIST_TYPE_DIRECT, D3D12_COMMAND_QUEUE_DESC,
                    D3D12_COMMAND_QUEUE_FLAG_NONE, D3D12CreateDevice, ID3D12CommandQueue,
                    ID3D12Device,
                },
                Dxgi::{
                    Common::{
                        DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_MODE_DESC, DXGI_MODE_SCALING_UNSPECIFIED,
                        DXGI_MODE_SCANLINE_ORDER_UNSPECIFIED, DXGI_RATIONAL, DXGI_SAMPLE_DESC,
                    },
                    CreateDXGIFactory2, DXGI_CREATE_FACTORY_FLAGS, DXGI_SWAP_CHAIN_DESC,
                    DXGI_SWAP_CHAIN_FLAG_ALLOW_MODE_SWITCH, DXGI_SWAP_EFFECT_DISCARD,
                    DXGI_SWAP_EFFECT_FLIP_DISCARD, DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIFactory2,
                    IDXGISwapChain,
                },
            },
            System::{
                LibraryLoader::{DisableThreadLibraryCalls, FreeLibraryAndExitThread},
                SystemServices::DLL_PROCESS_ATTACH,
                Threading::GetCurrentProcessId,
            },
            UI::WindowsAndMessaging::{GetClientRect, GetWindowThreadProcessId, IsWindowVisible},
        },
        core::{BOOL, HRESULT, Interface},
    },
};
use pal_fullscreen_rhi_protocol::{DetectedRhi, ProbeControl, ProbeState, probe_mapping_id};
use shared_memory::{Shmem, ShmemConf};

const MAPPING_OPEN_TIMEOUT: Duration = Duration::from_secs(3);
const PROBE_LIFETIME_LIMIT: Duration = Duration::from_secs(30);
const MIN_GAME_CLIENT_WIDTH: i32 = 640;
const MIN_GAME_CLIENT_HEIGHT: i32 = 360;
const ERROR_PANIC: u32 = 1;
const ERROR_HOOK_INITIALIZATION: u32 = 2;

type PresentFn = unsafe extern "system" fn(*mut c_void, u32, u32) -> HRESULT;

static CONTROL: AtomicPtr<ProbeControl> = AtomicPtr::new(ptr::null_mut());
static CALLBACKS_IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);
static PRESENT_TRAMPOLINE_A: AtomicUsize = AtomicUsize::new(0);
static PRESENT_TRAMPOLINE_B: AtomicUsize = AtomicUsize::new(0);

/// Windows loader entry point for the temporary RHI probe.
///
/// # Safety
///
/// This function must only be called by the Windows loader with the module
/// handle and notification values assigned to this loaded DLL. The attach path
/// disables thread notifications and immediately moves all probe work to a new
/// thread; it does not access the shared mapping or install hooks under the
/// loader lock.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllMain(module: HMODULE, reason: u32, _: *mut c_void) -> BOOL {
    if reason == DLL_PROCESS_ATTACH {
        let _ = unsafe { DisableThreadLibraryCalls(module) };
        let module_raw = module.0 as usize;
        thread::spawn(move || probe_thread(module_raw));
    }
    BOOL(1)
}

fn probe_thread(module_raw: usize) {
    let module = HMODULE(module_raw as *mut c_void);
    let process_id = unsafe { GetCurrentProcessId() };
    let Some(mapping) = open_mapping(process_id) else {
        unsafe { FreeLibraryAndExitThread(module, 1) };
    };
    let control = unsafe { &*mapping.as_ptr().cast::<ProbeControl>() };
    if !control.is_compatible(process_id) {
        unsafe { FreeLibraryAndExitThread(module, 2) };
    }
    CONTROL.store(
        control as *const ProbeControl as *mut ProbeControl,
        Ordering::Release,
    );

    let hooks = match panic::catch_unwind(AssertUnwindSafe(install_present_hooks)) {
        Ok(Ok(hooks)) => hooks,
        Ok(Err(())) => {
            control.mark_failed(ERROR_HOOK_INITIALIZATION);
            wait_for_stop(control);
            CONTROL.store(ptr::null_mut(), Ordering::Release);
            drop(mapping);
            unsafe { FreeLibraryAndExitThread(module, 3) };
        }
        Err(_) => {
            control.mark_failed(ERROR_PANIC);
            wait_for_stop(control);
            CONTROL.store(ptr::null_mut(), Ordering::Release);
            drop(mapping);
            unsafe { FreeLibraryAndExitThread(module, 4) };
        }
    };

    control.mark_running();
    wait_for_stop(control);
    uninstall_present_hooks(&hooks);
    CONTROL.store(ptr::null_mut(), Ordering::Release);
    control.mark_stopped();
    drop(mapping);
    unsafe { FreeLibraryAndExitThread(module, 0) };
}

fn open_mapping(process_id: u32) -> Option<Shmem> {
    let mapping_id = probe_mapping_id(process_id);
    let deadline = Instant::now() + MAPPING_OPEN_TIMEOUT;
    loop {
        if let Ok(mapping) = ShmemConf::new().os_id(&mapping_id).open() {
            return Some(mapping);
        }
        if Instant::now() >= deadline {
            return None;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn wait_for_stop(control: &ProbeControl) {
    let deadline = Instant::now() + PROBE_LIFETIME_LIMIT;
    while !control.stop_requested() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
}

fn install_present_hooks() -> Result<Vec<MhHook>, ()> {
    unsafe { MH_Initialize().ok() }.map_err(|_| ())?;
    let dx11_target = dx11_present_target();
    let dx12_target = dx12_present_target();
    let mut hooks = Vec::with_capacity(2);
    let installation: Result<(), ()> = (|| {
        let hook_a =
            unsafe { MhHook::new(dx11_target, present_probe_a as *mut c_void) }.map_err(|_| ())?;
        PRESENT_TRAMPOLINE_A.store(hook_a.trampoline() as usize, Ordering::Release);
        hooks.push(hook_a);

        if dx12_target != dx11_target {
            let hook_b = unsafe { MhHook::new(dx12_target, present_probe_b as *mut c_void) }
                .map_err(|_| ())?;
            PRESENT_TRAMPOLINE_B.store(hook_b.trampoline() as usize, Ordering::Release);
            hooks.push(hook_b);
        }

        for hook in &hooks {
            unsafe { hook.queue_enable() }.map_err(|_| ())?;
        }
        unsafe { MH_ApplyQueued().ok() }.map_err(|_| ())?;
        Ok(())
    })();
    if installation.is_err() {
        uninstall_present_hooks(&hooks);
        return Err(());
    }
    Ok(hooks)
}

fn uninstall_present_hooks(hooks: &[MhHook]) {
    for hook in hooks {
        let _ = unsafe { hook.queue_disable() };
    }
    let _ = unsafe { MH_ApplyQueued().ok() };
    while CALLBACKS_IN_FLIGHT.load(Ordering::Acquire) != 0 {
        thread::yield_now();
    }
    PRESENT_TRAMPOLINE_A.store(0, Ordering::Release);
    PRESENT_TRAMPOLINE_B.store(0, Ordering::Release);
    let _ = unsafe { MH_Uninitialize().ok() };
}

unsafe extern "system" fn present_probe_a(
    swap_chain: *mut c_void,
    sync_interval: u32,
    flags: u32,
) -> HRESULT {
    let _guard = CallbackGuard::new();
    inspect_swap_chain(swap_chain);
    unsafe {
        call_trampoline(
            PRESENT_TRAMPOLINE_A.load(Ordering::Acquire),
            swap_chain,
            sync_interval,
            flags,
        )
    }
}

unsafe extern "system" fn present_probe_b(
    swap_chain: *mut c_void,
    sync_interval: u32,
    flags: u32,
) -> HRESULT {
    let _guard = CallbackGuard::new();
    inspect_swap_chain(swap_chain);
    unsafe {
        call_trampoline(
            PRESENT_TRAMPOLINE_B.load(Ordering::Acquire),
            swap_chain,
            sync_interval,
            flags,
        )
    }
}

unsafe fn call_trampoline(
    trampoline: usize,
    swap_chain: *mut c_void,
    sync_interval: u32,
    flags: u32,
) -> HRESULT {
    let original = unsafe { mem::transmute::<usize, PresentFn>(trampoline) };
    unsafe { original(swap_chain, sync_interval, flags) }
}

fn inspect_swap_chain(raw_swap_chain: *mut c_void) {
    let control_ptr = CONTROL.load(Ordering::Acquire);
    if control_ptr.is_null() {
        return;
    }
    let control = unsafe { &*control_ptr };
    if matches!(
        control.state(),
        Some(ProbeState::Detected | ProbeState::Failed)
    ) {
        return;
    }

    let swap_chain = ManuallyDrop::new(unsafe { IDXGISwapChain::from_raw(raw_swap_chain) });
    let Ok(desc) = (unsafe { swap_chain.GetDesc() }) else {
        return;
    };
    let mut window_process_id = 0u32;
    unsafe { GetWindowThreadProcessId(desc.OutputWindow, Some(&mut window_process_id)) };
    if window_process_id != unsafe { GetCurrentProcessId() }
        || !unsafe { IsWindowVisible(desc.OutputWindow) }.as_bool()
    {
        return;
    }
    let mut client = RECT::default();
    if unsafe { GetClientRect(desc.OutputWindow, &mut client) }.is_err()
        || client.right - client.left < MIN_GAME_CLIENT_WIDTH
        || client.bottom - client.top < MIN_GAME_CLIENT_HEIGHT
    {
        return;
    }

    if unsafe { swap_chain.GetDevice::<ID3D12Device>() }.is_ok() {
        control.publish_detection(DetectedRhi::DirectX12);
    } else if unsafe { swap_chain.GetDevice::<ID3D11Device>() }.is_ok() {
        control.publish_detection(DetectedRhi::DirectX11);
    }
}

struct CallbackGuard;

impl CallbackGuard {
    fn new() -> Self {
        CALLBACKS_IN_FLIGHT.fetch_add(1, Ordering::AcqRel);
        Self
    }
}

impl Drop for CallbackGuard {
    fn drop(&mut self) {
        CALLBACKS_IN_FLIGHT.fetch_sub(1, Ordering::AcqRel);
    }
}

fn dx11_present_target() -> *mut c_void {
    let mut device: Option<ID3D11Device> = None;
    let mut context: Option<ID3D11DeviceContext> = None;
    let mut swap_chain: Option<IDXGISwapChain> = None;
    let window = DummyHwnd::new();
    unsafe {
        D3D11CreateDeviceAndSwapChain(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE(ptr::null_mut()),
            D3D11_CREATE_DEVICE_FLAG(0),
            Some(&[D3D_FEATURE_LEVEL_10_0, D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&DXGI_SWAP_CHAIN_DESC {
                BufferDesc: DXGI_MODE_DESC {
                    Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                    ScanlineOrdering: DXGI_MODE_SCANLINE_ORDER_UNSPECIFIED,
                    Scaling: DXGI_MODE_SCALING_UNSPECIFIED,
                    Width: 640,
                    Height: 480,
                    RefreshRate: DXGI_RATIONAL {
                        Numerator: 60,
                        Denominator: 1,
                    },
                },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 1,
                OutputWindow: window.hwnd(),
                Windowed: BOOL(1),
                SwapEffect: DXGI_SWAP_EFFECT_DISCARD,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                ..Default::default()
            }),
            Some(&mut swap_chain),
            Some(&mut device),
            None,
            Some(&mut context),
        )
        .expect("D3D11CreateDeviceAndSwapChain failed");
    }
    swap_chain.expect("DX11 swap chain").vtable().Present as *mut c_void
}

fn dx12_present_target() -> *mut c_void {
    let window = DummyHwnd::new();
    let factory: IDXGIFactory2 =
        unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }.expect("DXGI factory");
    let adapter = unsafe { factory.EnumAdapters(0) }.expect("DXGI adapter");
    let device: ID3D12Device = util::try_out_ptr(|output| unsafe {
        D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, output)
    })
    .expect("D3D12CreateDevice failed");
    let queue: ID3D12CommandQueue = unsafe {
        device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC {
            Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
            Priority: 0,
            Flags: D3D12_COMMAND_QUEUE_FLAG_NONE,
            NodeMask: 0,
        })
    }
    .expect("D3D12 command queue");
    let swap_chain: IDXGISwapChain = util::try_out_ptr(|output| unsafe {
        factory
            .CreateSwapChain(
                &queue,
                &DXGI_SWAP_CHAIN_DESC {
                    BufferDesc: DXGI_MODE_DESC {
                        Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                        ScanlineOrdering: DXGI_MODE_SCANLINE_ORDER_UNSPECIFIED,
                        Scaling: DXGI_MODE_SCALING_UNSPECIFIED,
                        Width: 640,
                        Height: 480,
                        RefreshRate: DXGI_RATIONAL {
                            Numerator: 60,
                            Denominator: 1,
                        },
                    },
                    BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                    BufferCount: 2,
                    OutputWindow: window.hwnd(),
                    Windowed: BOOL(1),
                    SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    Flags: DXGI_SWAP_CHAIN_FLAG_ALLOW_MODE_SWITCH.0 as u32,
                },
                output,
            )
            .ok()
    })
    .expect("DX12 swap chain");
    swap_chain.vtable().Present as *mut c_void
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_guard_holds_the_unload_barrier_for_its_entire_scope() {
        assert_eq!(CALLBACKS_IN_FLIGHT.load(Ordering::Acquire), 0);
        {
            let _guard = CallbackGuard::new();
            assert_eq!(CALLBACKS_IN_FLIGHT.load(Ordering::Acquire), 1);
        }
        assert_eq!(CALLBACKS_IN_FLIGHT.load(Ordering::Acquire), 0);
    }

    #[test]
    fn present_detours_hold_the_guard_through_the_trampoline_call() {
        let source = include_str!("lib.rs");
        for (start, end) in [
            (
                "unsafe extern \"system\" fn present_probe_a(",
                "unsafe extern \"system\" fn present_probe_b(",
            ),
            (
                "unsafe extern \"system\" fn present_probe_b(",
                "unsafe fn call_trampoline(",
            ),
        ] {
            let body = source
                .split_once(start)
                .expect("present detour")
                .1
                .split_once(end)
                .expect("next function")
                .0;
            let guard = body
                .find("let _guard = CallbackGuard::new();")
                .expect("guard");
            let inspect = body
                .find("inspect_swap_chain(swap_chain);")
                .expect("inspection");
            let trampoline = body.find("call_trampoline(").expect("trampoline");
            assert!(guard < inspect && inspect < trampoline);
        }

        let inspection = source
            .split_once("fn inspect_swap_chain(raw_swap_chain: *mut c_void) {")
            .expect("inspection function")
            .1
            .split_once("struct CallbackGuard;")
            .expect("callback guard type")
            .0;
        assert!(!inspection.contains("CallbackGuard::new()"));
    }
}
