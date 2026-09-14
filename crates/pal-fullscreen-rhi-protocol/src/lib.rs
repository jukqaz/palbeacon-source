use std::sync::atomic::{AtomicU32, Ordering};

pub const PROBE_DLL: &str = "pal-fullscreen-rhi-probe.dll";
pub const PROBE_MAPPING_BYTES: usize = 64;
const MAGIC: u64 = u64::from_le_bytes(*b"PALRHI01");
const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ProbeState {
    Initializing = 1,
    Running = 2,
    Detected = 3,
    Failed = 4,
    Stopped = 5,
}

impl ProbeState {
    pub const fn from_raw(value: u32) -> Option<Self> {
        match value {
            1 => Some(Self::Initializing),
            2 => Some(Self::Running),
            3 => Some(Self::Detected),
            4 => Some(Self::Failed),
            5 => Some(Self::Stopped),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum DetectedRhi {
    DirectX11 = 11,
    DirectX12 = 12,
}

impl DetectedRhi {
    pub const fn from_raw(value: u32) -> Option<Self> {
        match value {
            11 => Some(Self::DirectX11),
            12 => Some(Self::DirectX12),
            _ => None,
        }
    }
}

#[repr(C, align(64))]
pub struct ProbeControl {
    magic: u64,
    schema_version: u32,
    process_id: u32,
    state: AtomicU32,
    detected_rhi: AtomicU32,
    stop_requested: AtomicU32,
    error_code: AtomicU32,
    reserved: [u32; 8],
}

impl ProbeControl {
    pub const fn new(process_id: u32) -> Self {
        Self {
            magic: MAGIC,
            schema_version: SCHEMA_VERSION,
            process_id,
            state: AtomicU32::new(ProbeState::Initializing as u32),
            detected_rhi: AtomicU32::new(0),
            stop_requested: AtomicU32::new(0),
            error_code: AtomicU32::new(0),
            reserved: [0; 8],
        }
    }

    pub fn is_compatible(&self, expected_process_id: u32) -> bool {
        self.magic == MAGIC
            && self.schema_version == SCHEMA_VERSION
            && self.process_id == expected_process_id
    }

    pub fn state(&self) -> Option<ProbeState> {
        ProbeState::from_raw(self.state.load(Ordering::Acquire))
    }

    pub fn detected_rhi(&self) -> Option<DetectedRhi> {
        DetectedRhi::from_raw(self.detected_rhi.load(Ordering::Acquire))
    }

    pub fn error_code(&self) -> u32 {
        self.error_code.load(Ordering::Acquire)
    }

    pub fn mark_running(&self) {
        let _ = self.state.compare_exchange(
            ProbeState::Initializing as u32,
            ProbeState::Running as u32,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }

    pub fn publish_detection(&self, rhi: DetectedRhi) {
        self.detected_rhi.store(rhi as u32, Ordering::Relaxed);
        self.state
            .store(ProbeState::Detected as u32, Ordering::Release);
    }

    pub fn mark_failed(&self, error_code: u32) {
        self.error_code.store(error_code, Ordering::Relaxed);
        self.state
            .store(ProbeState::Failed as u32, Ordering::Release);
    }

    pub fn request_stop(&self) {
        self.stop_requested.store(1, Ordering::Release);
    }

    pub fn stop_requested(&self) -> bool {
        self.stop_requested.load(Ordering::Acquire) != 0
    }

    pub fn mark_stopped(&self) {
        self.state
            .store(ProbeState::Stopped as u32, Ordering::Release);
    }
}

pub fn probe_mapping_id(process_id: u32) -> String {
    format!("/pal_companion_rhi_probe_v1_{process_id}")
}

const _: () = assert!(size_of::<ProbeControl>() == PROBE_MAPPING_BYTES);

const fn size_of<T>() -> usize {
    std::mem::size_of::<T>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_publishes_one_detected_backend() {
        let control = ProbeControl::new(42);
        assert!(control.is_compatible(42));
        assert_eq!(control.state(), Some(ProbeState::Initializing));
        control.mark_running();
        assert_eq!(control.state(), Some(ProbeState::Running));
        control.publish_detection(DetectedRhi::DirectX12);
        assert_eq!(control.detected_rhi(), Some(DetectedRhi::DirectX12));
        assert_eq!(control.state(), Some(ProbeState::Detected));
        control.request_stop();
        assert!(control.stop_requested());
    }

    #[test]
    fn mappings_are_scoped_to_the_game_process() {
        assert_eq!(probe_mapping_id(123), "/pal_companion_rhi_probe_v1_123");
        assert!(!ProbeControl::new(7).is_compatible(8));
    }
}
