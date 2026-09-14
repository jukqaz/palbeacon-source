use std::{
    ffi::c_void,
    io::{self, Write},
    ptr::{self, null_mut},
    thread,
    time::{Duration, Instant},
};

use crate::{GraphicsApi, TestWindowOptions, WindowMode};
use hudhook::{
    util,
    windows::{
        Win32::{
            Foundation::{HMODULE, HWND},
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
                    CreateDXGIFactory2, DXGI_CREATE_FACTORY_FLAGS, DXGI_PRESENT,
                    DXGI_SWAP_CHAIN_DESC, DXGI_SWAP_CHAIN_FLAG_ALLOW_MODE_SWITCH,
                    DXGI_SWAP_EFFECT_DISCARD, DXGI_SWAP_EFFECT_FLIP_DISCARD,
                    DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIFactory2, IDXGISwapChain,
                },
            },
            System::Threading::GetCurrentProcessId,
        },
        core::BOOL,
    },
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, DispatchMessageW, IsWindow, MSG, PM_REMOVE, PeekMessageW,
    TranslateMessage, WM_QUIT, WS_OVERLAPPEDWINDOW, WS_POPUP, WS_VISIBLE,
};

const STATIC_CLASS: [u16; 7] = [
    b'S' as u16,
    b'T' as u16,
    b'A' as u16,
    b'T' as u16,
    b'I' as u16,
    b'C' as u16,
    0,
];
const WINDOW_TITLE: [u16; 38] = [
    b'P' as u16,
    b'a' as u16,
    b'l' as u16,
    b' ' as u16,
    b'C' as u16,
    b'o' as u16,
    b'm' as u16,
    b'p' as u16,
    b'a' as u16,
    b'n' as u16,
    b'i' as u16,
    b'o' as u16,
    b'n' as u16,
    b' ' as u16,
    b'D' as u16,
    b'i' as u16,
    b'r' as u16,
    b'e' as u16,
    b'c' as u16,
    b't' as u16,
    b'X' as u16,
    b' ' as u16,
    b'T' as u16,
    b'e' as u16,
    b's' as u16,
    b't' as u16,
    b' ' as u16,
    b'W' as u16,
    b'i' as u16,
    b'n' as u16,
    b'd' as u16,
    b'o' as u16,
    b'w' as u16,
    b' ' as u16,
    b'v' as u16,
    b'1' as u16,
    0,
    0,
];

pub fn run(options: TestWindowOptions) -> Result<(), Box<dyn std::error::Error>> {
    pal_windows::enable_per_monitor_v2()?;

    let style = match options.window_mode {
        WindowMode::Windowed => WS_OVERLAPPEDWINDOW | WS_VISIBLE,
        WindowMode::Borderless | WindowMode::Exclusive => WS_POPUP | WS_VISIBLE,
    };
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            STATIC_CLASS.as_ptr(),
            WINDOW_TITLE.as_ptr(),
            style,
            160,
            120,
            i32::try_from(options.width)?,
            i32::try_from(options.height)?,
            null_mut(),
            null_mut(),
            null_mut(),
            null_mut(),
        )
    };
    if hwnd.is_null() {
        return Err(io::Error::last_os_error().into());
    }

    let mut surface = match GraphicsSurface::create(hwnd, options) {
        Ok(surface) => surface,
        Err(error) => {
            unsafe { DestroyWindow(hwnd) };
            return Err(error);
        }
    };

    let process_id = unsafe { GetCurrentProcessId() };
    println!("READY {process_id} {}", hwnd as usize as u64);
    println!(
        "CAPABILITIES {{\"schema\":\"pal_companion.test_window.v1\",\"api\":\"{}\",\"window_mode\":\"{}\",\"width\":{},\"height\":{}}}",
        options.graphics_api.as_str(),
        options.window_mode.as_str(),
        options.width,
        options.height,
    );
    io::stdout().flush()?;

    let deadline = Instant::now() + options.duration;
    let mut message = MSG::default();
    while Instant::now() < deadline {
        while unsafe { PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE) } != 0 {
            if message.message == WM_QUIT {
                break;
            }
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        if unsafe { IsWindow(hwnd) } == 0 || message.message == WM_QUIT {
            return Ok(());
        }
        surface.present()?;
        thread::sleep(Duration::from_millis(16));
    }

    drop(surface);
    unsafe { DestroyWindow(hwnd) };
    Ok(())
}

enum GraphicsSurface {
    Gdi,
    DirectX11(DirectXSurface),
    DirectX12(DirectXSurface),
}

impl GraphicsSurface {
    fn create(
        hwnd: *mut c_void,
        options: TestWindowOptions,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        match options.graphics_api {
            GraphicsApi::Gdi => Ok(Self::Gdi),
            GraphicsApi::DirectX11 => {
                DirectXSurface::create_dx11(hwnd, options).map(Self::DirectX11)
            }
            GraphicsApi::DirectX12 => {
                DirectXSurface::create_dx12(hwnd, options).map(Self::DirectX12)
            }
        }
    }

    fn present(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            Self::Gdi => Ok(()),
            Self::DirectX11(surface) | Self::DirectX12(surface) => surface.present(),
        }
    }
}

struct DirectXSurface {
    swap_chain: IDXGISwapChain,
    _lifetime: DirectXLifetime,
    exclusive: bool,
}

enum DirectXLifetime {
    DirectX11 {
        _device: ID3D11Device,
        _context: ID3D11DeviceContext,
    },
    DirectX12 {
        _factory: IDXGIFactory2,
        _device: ID3D12Device,
        _queue: ID3D12CommandQueue,
    },
}

impl DirectXSurface {
    fn create_dx11(
        raw_hwnd: *mut c_void,
        options: TestWindowOptions,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut device: Option<ID3D11Device> = None;
        let mut context: Option<ID3D11DeviceContext> = None;
        let mut swap_chain: Option<IDXGISwapChain> = None;
        unsafe {
            D3D11CreateDeviceAndSwapChain(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE(ptr::null_mut()),
                D3D11_CREATE_DEVICE_FLAG(0),
                Some(&[D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_10_0]),
                D3D11_SDK_VERSION,
                Some(&swap_chain_desc(
                    raw_hwnd,
                    options,
                    DXGI_SWAP_EFFECT_DISCARD,
                    1,
                )),
                Some(&mut swap_chain),
                Some(&mut device),
                None,
                Some(&mut context),
            )?;
        }
        let mut surface = Self {
            swap_chain: swap_chain.ok_or("D3D11 swap chain was not created")?,
            _lifetime: DirectXLifetime::DirectX11 {
                _device: device.ok_or("D3D11 device was not created")?,
                _context: context.ok_or("D3D11 context was not created")?,
            },
            exclusive: false,
        };
        surface.enable_exclusive_if_requested(options)?;
        Ok(surface)
    }

    fn create_dx12(
        raw_hwnd: *mut c_void,
        options: TestWindowOptions,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let factory: IDXGIFactory2 = unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }?;
        let adapter = unsafe { factory.EnumAdapters(0) }?;
        let device: ID3D12Device = util::try_out_ptr(|output| unsafe {
            D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, output)
        })?;
        let queue: ID3D12CommandQueue = unsafe {
            device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC {
                Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
                Priority: 0,
                Flags: D3D12_COMMAND_QUEUE_FLAG_NONE,
                NodeMask: 0,
            })
        }?;
        let swap_chain: IDXGISwapChain = util::try_out_ptr(|output| unsafe {
            factory
                .CreateSwapChain(
                    &queue,
                    &swap_chain_desc(raw_hwnd, options, DXGI_SWAP_EFFECT_FLIP_DISCARD, 2),
                    output,
                )
                .ok()
        })?;
        let mut surface = Self {
            swap_chain,
            _lifetime: DirectXLifetime::DirectX12 {
                _factory: factory,
                _device: device,
                _queue: queue,
            },
            exclusive: false,
        };
        surface.enable_exclusive_if_requested(options)?;
        Ok(surface)
    }

    fn enable_exclusive_if_requested(
        &mut self,
        options: TestWindowOptions,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if options.window_mode == WindowMode::Exclusive {
            unsafe { self.swap_chain.SetFullscreenState(true, None) }?;
            self.exclusive = true;
        }
        Ok(())
    }

    fn present(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        unsafe { self.swap_chain.Present(1, DXGI_PRESENT(0)).ok() }?;
        Ok(())
    }
}

impl Drop for DirectXSurface {
    fn drop(&mut self) {
        if self.exclusive {
            let _ = unsafe { self.swap_chain.SetFullscreenState(false, None) };
        }
    }
}

fn swap_chain_desc(
    raw_hwnd: *mut c_void,
    options: TestWindowOptions,
    swap_effect: hudhook::windows::Win32::Graphics::Dxgi::DXGI_SWAP_EFFECT,
    buffer_count: u32,
) -> DXGI_SWAP_CHAIN_DESC {
    DXGI_SWAP_CHAIN_DESC {
        BufferDesc: DXGI_MODE_DESC {
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            ScanlineOrdering: DXGI_MODE_SCANLINE_ORDER_UNSPECIFIED,
            Scaling: DXGI_MODE_SCALING_UNSPECIFIED,
            Width: options.width,
            Height: options.height,
            RefreshRate: DXGI_RATIONAL {
                Numerator: 60,
                Denominator: 1,
            },
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: buffer_count,
        OutputWindow: HWND(raw_hwnd),
        Windowed: BOOL((options.window_mode != WindowMode::Exclusive) as i32),
        SwapEffect: swap_effect,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Flags: DXGI_SWAP_CHAIN_FLAG_ALLOW_MODE_SWITCH.0 as u32,
    }
}
