use std::collections::VecDeque;

use pal_protocol::{MAX_FRAME_LEN, MAX_PAYLOAD_LEN};
use prost::Message;
use thiserror::Error;

const MAX_QUEUED_FRAMES: usize = 1_024;

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum StreamError {
    #[error("frame payload is empty")]
    Empty,
    #[error("frame payload exceeds one mebibyte")]
    TooLarge,
    #[error("frame is truncated")]
    Truncated,
    #[error("frame payload is malformed")]
    Malformed,
    #[error("completed frame queue is full")]
    QueueFull,
}

#[derive(Default)]
pub struct FramedStream {
    buffer: Vec<u8>,
    completed: VecDeque<Vec<u8>>,
    queued_bytes: usize,
}

impl FramedStream {
    pub const fn new() -> Self {
        Self {
            buffer: Vec::new(),
            completed: VecDeque::new(),
            queued_bytes: 0,
        }
    }

    pub fn push(&mut self, mut input: &[u8]) -> Result<(), StreamError> {
        while !input.is_empty() {
            if self.buffer.len() < 4 {
                let needed = 4 - self.buffer.len();
                let take = needed.min(input.len());
                self.buffer.extend_from_slice(&input[..take]);
                input = &input[take..];
                if self.buffer.len() < 4 {
                    return Ok(());
                }
                self.validate_declared_len()?;
            }

            let payload_len = self.declared_len();
            let frame_len = payload_len + 4;
            let needed = frame_len - self.buffer.len();
            let take = needed.min(input.len());
            self.buffer.extend_from_slice(&input[..take]);
            input = &input[take..];

            if self.buffer.len() == frame_len {
                if self.completed.len() == MAX_QUEUED_FRAMES
                    || self
                        .queued_bytes
                        .checked_add(payload_len)
                        .is_none_or(|total| total > MAX_PAYLOAD_LEN)
                {
                    self.buffer.clear();
                    return Err(StreamError::QueueFull);
                }
                let payload = self.buffer.split_off(4);
                self.buffer.clear();
                self.queued_bytes += payload.len();
                self.completed.push_back(payload);
            }
        }
        Ok(())
    }

    pub fn next_payload(&mut self) -> Option<Vec<u8>> {
        let payload = self.completed.pop_front()?;
        self.queued_bytes -= payload.len();
        Some(payload)
    }

    pub fn next_message<M>(&mut self) -> Result<Option<M>, StreamError>
    where
        M: Message + Default,
    {
        let Some(payload) = self.next_payload() else {
            return Ok(None);
        };
        M::decode(payload.as_slice())
            .map(Some)
            .map_err(|_| StreamError::Malformed)
    }

    pub fn finish(&self) -> Result<(), StreamError> {
        if self.buffer.is_empty() {
            Ok(())
        } else {
            Err(StreamError::Truncated)
        }
    }

    pub const fn buffered_len(&self) -> usize {
        self.buffer.len()
    }

    pub const fn queued_bytes(&self) -> usize {
        self.queued_bytes
    }

    fn validate_declared_len(&mut self) -> Result<(), StreamError> {
        let declared = self.declared_len();
        if declared == 0 {
            self.buffer.clear();
            return Err(StreamError::Empty);
        }
        if declared > MAX_PAYLOAD_LEN {
            self.buffer.clear();
            return Err(StreamError::TooLarge);
        }
        debug_assert!(declared + 4 <= MAX_FRAME_LEN);
        Ok(())
    }

    fn declared_len(&self) -> usize {
        u32::from_le_bytes(
            self.buffer[..4]
                .try_into()
                .expect("the prefix length was checked"),
        ) as usize
    }
}
