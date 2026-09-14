use pal_render::ChromePlan;

use crate::{OverlayLayout, actual_map_preview::CpuMapSurface};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RendererKind {
    /// Opaque color-key GDI renderer used to validate geometry and chrome correctness.
    ///
    /// Its CPU/GPU measurements are not production acceptance evidence.
    GdiCorrectnessPreview,
    D3d11DirectComposition,
}

impl RendererKind {
    pub const fn diagnostic_name(self) -> &'static str {
        match self {
            Self::GdiCorrectnessPreview => "gdi-correctness-preview",
            Self::D3d11DirectComposition => "d3d11-direct-composition",
        }
    }

    pub const fn is_production_acceptance_renderer(self) -> bool {
        matches!(self, Self::D3d11DirectComposition)
    }
}

pub trait OverlayRenderer {
    type Error;

    fn kind(&self) -> RendererKind;
    fn resize(&mut self, layout: OverlayLayout, dpi: u32) -> Result<(), Self::Error>;
    fn present(&mut self, map: &CpuMapSurface, chrome: ChromePlan) -> Result<(), Self::Error>;
}
