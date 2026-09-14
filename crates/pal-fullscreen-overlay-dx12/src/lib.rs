use pal_fullscreen_bridge::ConsumerBackend;
use pal_fullscreen_render_loop::FullscreenRenderLoop;

hudhook::hudhook!(
    hudhook::hooks::dx12::ImguiDx12Hooks,
    FullscreenRenderLoop::new(ConsumerBackend::DirectX12)
);
