#![cfg(windows)]

use pal_domain::InputMode;
use pal_overlay_win::{
    HitShape, HitTest, InputController, OverlayLayout, PhysicalPoint, PhysicalRect, PhysicalSize,
};
use windows_sys::Win32::Graphics::Gdi::{CreateRoundRectRgn, DeleteObject, PtInRegion};
use windows_sys::Win32::System::Threading::{GR_GDIOBJECTS, GetCurrentProcess, GetGuiResources};

#[test]
fn native_round_rect_region_and_input_hit_test_agree_on_every_client_pixel() {
    let cases: [(u32, u32, u32, u32); 5] = [
        (288, 288, 18, 18),
        (820, 520, 18, 18),
        (432, 432, 27, 27),
        (1_230, 780, 27, 27),
        (40, 20, 999, 10),
    ];
    for (width, height, requested_corner_radius_px, effective_corner_radius_px) in cases {
        let screen_bounds = PhysicalRect::new(-984, 56, width, height);
        let layout = OverlayLayout::new(
            screen_bounds,
            HitShape::RoundedRectangle {
                bounds: screen_bounds,
                corner_radius_px: requested_corner_radius_px,
            },
            screen_bounds,
            None,
            PhysicalSize::new(width, height),
        );
        let input = InputController::new(InputMode::PinnedInteractive, layout);
        let native_right = i32::try_from(width).expect("fixture width") + 1;
        let native_bottom = i32::try_from(height).expect("fixture height") + 1;
        let ellipse = i32::try_from(effective_corner_radius_px * 2).expect("fixture radius");
        // SAFETY: finite fixture coordinates create a process-owned region released below.
        let region =
            unsafe { CreateRoundRectRgn(0, 0, native_right, native_bottom, ellipse, ellipse) };
        assert!(!region.is_null(), "CreateRoundRectRgn must succeed");
        // SAFETY: pseudo process handles are valid for the current process lifetime.
        let gdi_objects_before_hits =
            unsafe { GetGuiResources(GetCurrentProcess(), GR_GDIOBJECTS) };

        let mut mismatch_count = 0;
        let mut mismatch_sample = Vec::new();
        for local_y in 0..i32::try_from(height).expect("fixture height") {
            for local_x in 0..i32::try_from(width).expect("fixture width") {
                // SAFETY: region remains valid for the duration of the loop.
                let native_contains = unsafe { PtInRegion(region, local_x, local_y) } != 0;
                let screen =
                    PhysicalPoint::new(screen_bounds.left + local_x, screen_bounds.top + local_y);
                let input_contains = input.wm_nchittest(screen) == HitTest::Client;
                if input_contains != native_contains {
                    mismatch_count += 1;
                    if mismatch_sample.len() < 12 {
                        mismatch_sample.push((
                            PhysicalPoint::new(local_x, local_y),
                            input_contains,
                            native_contains,
                        ));
                    }
                }
            }
        }
        assert_eq!(
            mismatch_count, 0,
            "{width}x{height} requested r{requested_corner_radius_px} / effective \
             r{effective_corner_radius_px} native/input mismatches: {mismatch_sample:?}"
        );
        // PtInRegion reuses InputController's cached HRGN. Exhaustive hit testing must not allocate
        // or leak a GDI object per point.
        assert_eq!(
            unsafe { GetGuiResources(GetCurrentProcess(), GR_GDIOBJECTS) },
            gdi_objects_before_hits
        );

        // The +1 native compensation restores the intended last in-client pixel but must not
        // create an input area beyond the half-open HWND client bounds.
        assert_eq!(
            unsafe {
                PtInRegion(
                    region,
                    i32::try_from(width).expect("fixture width"),
                    i32::try_from(height / 2).expect("fixture height"),
                )
            },
            0
        );
        assert_eq!(
            unsafe {
                PtInRegion(
                    region,
                    i32::try_from(width / 2).expect("fixture width"),
                    i32::try_from(height).expect("fixture height"),
                )
            },
            0
        );

        // SAFETY: the region is still owned by this test.
        assert_ne!(unsafe { DeleteObject(region) }, 0);
    }
}
