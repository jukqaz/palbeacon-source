use pal_fullscreen_bridge::ConsumerBackend;
use pal_fullscreen_render_loop::FullscreenRenderLoop;

hudhook::hudhook!(
    hudhook::hooks::dx11::ImguiDx11Hooks,
    FullscreenRenderLoop::new(ConsumerBackend::DirectX11)
);
