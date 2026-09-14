#![cfg(feature = "test-harness")]

use pal_domain::PoiFilters;
use pal_overlay_win::synthetic_preview::{
    PREVIEW_DIAMETER, SyntheticPreviewFrame, SyntheticSurface,
};

const DEFAULT_PREVIEW_FINGERPRINT: u64 = 0xdda5_1158_8c3b_7efa;
const TERRAIN_ONLY_PREVIEW_FINGERPRINT: u64 = 0x2b42_f5ce_58bb_2a2a;

fn render(filters: PoiFilters) -> SyntheticSurface {
    let frame = SyntheticPreviewFrame::from_fixture(filters).expect("validated preview fixture");
    let mut surface = SyntheticSurface::new(PREVIEW_DIAMETER).expect("bounded preview surface");
    surface.rasterize(&frame);
    surface
}

fn fnv1a_pixels(pixels: &[u32]) -> u64 {
    pixels.iter().fold(0xcbf2_9ce4_8422_2325, |hash, pixel| {
        pixel.to_le_bytes().into_iter().fold(hash, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
    })
}

#[test]
fn default_preview_matches_the_reviewed_pixel_contract() {
    let surface = render(PoiFilters::default());
    let fingerprint = fnv1a_pixels(surface.pixels());

    assert_eq!(surface.diameter(), PREVIEW_DIAMETER);
    assert!(surface.watermark_drawn());
    assert_eq!(fingerprint, DEFAULT_PREVIEW_FINGERPRINT);
}

#[test]
fn disabling_primary_pois_matches_the_reviewed_pixel_contract() {
    let surface = render(PoiFilters {
        fast_travel: false,
        boss: false,
        wanted: false,
        dungeon: false,
        ..PoiFilters::default()
    });
    let fingerprint = fnv1a_pixels(surface.pixels());

    assert_eq!(surface.diameter(), PREVIEW_DIAMETER);
    assert!(surface.watermark_drawn());
    assert_eq!(fingerprint, TERRAIN_ONLY_PREVIEW_FINGERPRINT);
}
