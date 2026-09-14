#![cfg(feature = "test-harness")]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::hint::black_box;

use pal_domain::{OverlaySettings, PositionSample, SampleClock};
use pal_overlay_win::actual_map_preview::authoritative_main_map_world_to_image;
use pal_overlay_win::actual_map_runtime::{ActualMapPreviewRuntime, PreviewTick};
use pal_state::{PositionSource, PositionSourceError, PositionSourceEvent};

struct ThreadCountingAllocator;

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

fn record_allocation() {
    COUNTING.with(|counting| {
        if counting.get() {
            ALLOCATIONS.with(|allocations| {
                allocations.set(allocations.get().saturating_add(1));
            });
        }
    });
}

unsafe impl GlobalAlloc for ThreadCountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        record_allocation();
        unsafe { System.realloc(pointer, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: ThreadCountingAllocator = ThreadCountingAllocator;

#[test]
fn unchanged_runtime_ticks_allocate_exactly_zero() {
    let mut runtime = ActualMapPreviewRuntime::new(
        OneSampleSource::new(),
        OverlaySettings::default(),
        authoritative_main_map_world_to_image(),
    )
    .expect("runtime");
    assert!(matches!(
        runtime.tick(0, 2_048, 2_048).expect("initial tick"),
        PreviewTick::Present(_)
    ));

    let (tick, allocations) =
        allocation_count(|| black_box(runtime.tick(500, 2_048, 2_048).expect("unchanged tick")));

    assert_eq!(tick, PreviewTick::Unchanged);
    assert_eq!(allocations, 0);
}

#[test]
fn waiting_for_the_first_sample_allocates_exactly_zero_after_connect() {
    let mut runtime = ActualMapPreviewRuntime::new(
        ConnectedOnlySource::default(),
        OverlaySettings::default(),
        authoritative_main_map_world_to_image(),
    )
    .expect("runtime");
    assert_eq!(
        runtime.tick(0, 2_048, 2_048).expect("connect tick"),
        PreviewTick::Unavailable(pal_domain::Freshness::Stale)
    );

    let (tick, allocations) =
        allocation_count(|| black_box(runtime.tick(16, 2_048, 2_048).expect("waiting tick")));

    assert_eq!(tick, PreviewTick::Unchanged);
    assert_eq!(allocations, 0);
}

#[test]
fn established_present_ticks_allocate_exactly_zero() {
    let mut runtime = ActualMapPreviewRuntime::new(
        TwoSampleSource::new(),
        OverlaySettings::default(),
        authoritative_main_map_world_to_image(),
    )
    .expect("runtime");
    assert!(matches!(
        runtime.tick(0, 2_048, 2_048).expect("first sample"),
        PreviewTick::Present(_)
    ));

    let (tick, allocations) =
        allocation_count(|| black_box(runtime.tick(100, 2_048, 2_048).expect("second sample")));

    assert!(matches!(tick, PreviewTick::Present(_)));
    assert_eq!(allocations, 0);
}

fn allocation_count<T>(operation: impl FnOnce() -> T) -> (T, usize) {
    ALLOCATIONS.with(|allocations| allocations.set(0));
    COUNTING.with(|counting| counting.set(true));
    let output = operation();
    COUNTING.with(|counting| counting.set(false));
    let count = ALLOCATIONS.with(Cell::get);
    (output, count)
}

struct OneSampleSource {
    phase: u8,
    sample: PositionSample,
}

impl OneSampleSource {
    fn new() -> Self {
        Self {
            phase: 0,
            sample: PositionSample::new(
                "preview-world",
                b"preview-subject",
                b"preview-boot",
                1,
                1,
                -343_155.0,
                244_585.0,
                0.0,
                Some(45.0),
                SampleClock::received_with_age(0, 0),
            )
            .expect("valid sample"),
        }
    }
}

impl PositionSource for OneSampleSource {
    fn poll(
        &mut self,
        _now_monotonic_ms: u64,
    ) -> Result<Option<PositionSourceEvent>, PositionSourceError> {
        let event = match self.phase {
            0 => Some(PositionSourceEvent::Connected { generation: 1 }),
            1 => Some(PositionSourceEvent::Sample(self.sample.clone())),
            _ => None,
        };
        self.phase = self.phase.saturating_add(1);
        Ok(event)
    }
}

#[derive(Default)]
struct ConnectedOnlySource {
    emitted: bool,
}

impl PositionSource for ConnectedOnlySource {
    fn poll(
        &mut self,
        _now_monotonic_ms: u64,
    ) -> Result<Option<PositionSourceEvent>, PositionSourceError> {
        if self.emitted {
            Ok(None)
        } else {
            self.emitted = true;
            Ok(Some(PositionSourceEvent::Connected { generation: 1 }))
        }
    }
}

struct TwoSampleSource {
    phase: u8,
    first: Option<PositionSample>,
    second: Option<PositionSample>,
}

impl TwoSampleSource {
    fn new() -> Self {
        Self {
            phase: 0,
            first: Some(sample(1, -343_155.0, 244_585.0, 0)),
            second: Some(sample(2, -338_155.0, 248_585.0, 100)),
        }
    }
}

impl PositionSource for TwoSampleSource {
    fn poll(
        &mut self,
        now_monotonic_ms: u64,
    ) -> Result<Option<PositionSourceEvent>, PositionSourceError> {
        match self.phase {
            0 => {
                self.phase = 1;
                Ok(Some(PositionSourceEvent::Connected { generation: 1 }))
            }
            1 => {
                self.phase = 2;
                Ok(self.first.take().map(PositionSourceEvent::Sample))
            }
            2 if now_monotonic_ms >= 100 => {
                self.phase = 3;
                Ok(self.second.take().map(PositionSourceEvent::Sample))
            }
            _ => Ok(None),
        }
    }
}

fn sample(sequence: u64, x: f64, y: f64, received_at_ms: u64) -> PositionSample {
    PositionSample::new(
        "preview-world",
        b"preview-subject",
        b"preview-boot",
        1,
        sequence,
        x,
        y,
        0.0,
        Some(45.0),
        SampleClock::received_with_age(0, received_at_ms),
    )
    .expect("valid sample")
}
