use pal_overlay_win::{
    HitShape, OverlayLayout, PhysicalPoint, PhysicalRect, PhysicalSize, PreviewPixelRole,
    preview_pixel_role, renderer::RendererKind,
};

#[test]
fn map_interior_has_an_opaque_base_while_exterior_keeps_the_color_key() {
    let bounds = PhysicalRect::new(400, 60, 240, 240);
    let layout = OverlayLayout::new(
        PhysicalRect::new(0, 0, 800, 600),
        HitShape::RoundedRectangle {
            bounds,
            corner_radius_px: 18,
        },
        bounds,
        None,
        PhysicalSize::new(240, 240),
    );
    // This point is deliberately away from the border, center cross, and top-left label.
    let interactive_target = PhysicalPoint::new(580, 240);
    let exterior = PhysicalPoint::new(20, 20);

    let interior_role = preview_pixel_role(layout, interactive_target);
    let exterior_role = preview_pixel_role(layout, exterior);

    assert_eq!(interior_role, PreviewPixelRole::OpaqueMapInterior);
    assert_eq!(exterior_role, PreviewPixelRole::ExteriorColorKey);
    assert_ne!(
        interior_role.base_color_ref(),
        exterior_role.base_color_ref()
    );
}

#[test]
fn gdi_preview_is_explicitly_a_correctness_renderer() {
    assert_eq!(
        RendererKind::GdiCorrectnessPreview.diagnostic_name(),
        "gdi-correctness-preview"
    );
    assert!(!RendererKind::GdiCorrectnessPreview.is_production_acceptance_renderer());
    assert!(RendererKind::D3d11DirectComposition.is_production_acceptance_renderer());
}
