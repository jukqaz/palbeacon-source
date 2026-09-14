use prost::Message;
use thiserror::Error;

pub const MAX_PAYLOAD_LEN: usize = 1_048_576;
pub const MAX_FRAME_LEN: usize = MAX_PAYLOAD_LEN + 4;

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum FrameError {
    #[error("frame is truncated")]
    Truncated,
    #[error("frame payload is empty")]
    Empty,
    #[error("frame payload exceeds one mebibyte")]
    TooLarge,
    #[error("frame contains trailing bytes")]
    TrailingBytes,
    #[error("protobuf encoding failed")]
    Encode,
    #[error("protobuf decoding failed")]
    Decode,
}

pub struct FrameCodec;

impl FrameCodec {
    pub fn encode<M: Message>(message: &M) -> Result<Vec<u8>, FrameError> {
        let payload_len = message.encoded_len();
        if payload_len == 0 {
            return Err(FrameError::Empty);
        }
        if payload_len > MAX_PAYLOAD_LEN {
            return Err(FrameError::TooLarge);
        }

        let mut frame = Vec::with_capacity(payload_len + 4);
        frame.extend_from_slice(&(payload_len as u32).to_le_bytes());
        message.encode(&mut frame).map_err(|_| FrameError::Encode)?;
        Ok(frame)
    }

    pub fn decode<M>(frame: &[u8]) -> Result<M, FrameError>
    where
        M: Message + Default,
    {
        if frame.len() < 4 {
            return Err(FrameError::Truncated);
        }

        let declared_len = u32::from_le_bytes(
            frame[..4]
                .try_into()
                .expect("the four-byte frame prefix was checked above"),
        ) as usize;
        if declared_len == 0 {
            return Err(FrameError::Empty);
        }
        if declared_len > MAX_PAYLOAD_LEN {
            return Err(FrameError::TooLarge);
        }

        let body_len = frame.len() - 4;
        if body_len < declared_len {
            return Err(FrameError::Truncated);
        }
        if body_len > declared_len {
            return Err(FrameError::TrailingBytes);
        }

        M::decode(&frame[4..]).map_err(|_| FrameError::Decode)
    }
}
