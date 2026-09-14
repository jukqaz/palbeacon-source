#![cfg(feature = "development-live-agent")]

use pal_domain::{Freshness, OverlaySettings};
use pal_overlay_win::actual_map_preview::authoritative_main_map_world_to_image;
use pal_overlay_win::actual_map_runtime::{ActualMapPreviewRuntime, PreviewTick};
use pal_state::{
    ServerAgentControlOutcome, ServerAgentFrame, ServerAgentIngestOutcome, server_agent_channel,
};
use pal_telemetry::MonotonicEpoch;

const SUBJECT: [u8; 32] = [0x42; 32];

#[test]
fn server_agent_channel_moves_the_actual_map_with_the_shared_epoch() {
    let epoch = MonotonicEpoch::now();
    let (sender, source) = server_agent_channel("world-a", SUBJECT).expect("channel");
    assert_eq!(sender.connect(1), ServerAgentControlOutcome::Accepted);
    assert_eq!(
        sender
            .publish_position(
                ServerAgentFrame::new(
                    "world-a",
                    SUBJECT,
                    [0x11; 16],
                    1,
                    1,
                    -343_155.0,
                    244_585.0,
                    0.0,
                    Some(45.0),
                    0,
                ),
                epoch.elapsed_ms(),
            )
            .expect("publish"),
        ServerAgentIngestOutcome::Accepted
    );
    let mut runtime = ActualMapPreviewRuntime::new(
        source,
        OverlaySettings::default(),
        authoritative_main_map_world_to_image(),
    )
    .expect("runtime");

    let frame = match runtime
        .tick(epoch.elapsed_ms(), 2_048, 2_048)
        .expect("tick")
    {
        PreviewTick::Present(frame) => frame,
        other => panic!("expected live map frame, got {other:?}"),
    };

    assert_eq!(frame.generation(), 1);
    assert_eq!(frame.sequence(), 1);
    assert_eq!(frame.freshness(), Freshness::Live);
}
