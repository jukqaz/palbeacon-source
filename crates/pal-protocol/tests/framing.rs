use pal_protocol::{FrameCodec, FrameError, MAX_PAYLOAD_LEN};
use prost::Message;

#[derive(Clone, PartialEq, Message)]
struct EmptyMessage {}

#[derive(Clone, PartialEq, Message)]
struct BlobMessage {
    #[prost(bytes = "vec", tag = "1")]
    value: Vec<u8>,
}

fn blob_with_encoded_len(target: usize) -> BlobMessage {
    let mut low = 0;
    let mut high = target;

    while low <= high {
        let middle = low + (high - low) / 2;
        let message = BlobMessage {
            value: vec![0xA5; middle],
        };
        match message.encoded_len().cmp(&target) {
            std::cmp::Ordering::Less => low = middle + 1,
            std::cmp::Ordering::Greater => high = middle.saturating_sub(1),
            std::cmp::Ordering::Equal => return message,
        }
    }

    panic!("no BlobMessage has encoded length {target}");
}

#[test]
fn rejects_an_empty_default_message_before_writing_a_frame() {
    assert_eq!(
        FrameCodec::encode(&EmptyMessage {}).unwrap_err(),
        FrameError::Empty
    );
}

#[test]
fn accepts_a_payload_exactly_at_the_one_mebibyte_limit() {
    let message = blob_with_encoded_len(MAX_PAYLOAD_LEN);
    let frame = FrameCodec::encode(&message).unwrap();

    assert_eq!(frame.len(), MAX_PAYLOAD_LEN + 4);
    assert_eq!(
        u32::from_le_bytes(frame[..4].try_into().unwrap()) as usize,
        MAX_PAYLOAD_LEN
    );
    assert_eq!(FrameCodec::decode::<BlobMessage>(&frame).unwrap(), message);
}

#[test]
fn rejects_a_payload_one_byte_above_the_limit_before_allocating_a_frame() {
    let message = blob_with_encoded_len(MAX_PAYLOAD_LEN + 1);

    assert_eq!(
        FrameCodec::encode(&message).unwrap_err(),
        FrameError::TooLarge
    );
}

#[test]
fn zero_to_three_header_bytes_are_truncated() {
    for header_len in 0..=3 {
        assert_eq!(
            FrameCodec::decode::<BlobMessage>(&[0_u8; 3][..header_len]).unwrap_err(),
            FrameError::Truncated,
            "header length {header_len}"
        );
    }
}

#[test]
fn zero_length_prefix_is_empty_even_when_bytes_follow() {
    let mut frame = 0_u32.to_le_bytes().to_vec();
    frame.extend_from_slice(&[0x08, 0x01]);

    assert_eq!(
        FrameCodec::decode::<BlobMessage>(&frame).unwrap_err(),
        FrameError::Empty
    );
}

#[test]
fn max_plus_one_prefix_is_too_large_before_body_truncation_is_considered() {
    let frame = ((MAX_PAYLOAD_LEN + 1) as u32).to_le_bytes();

    assert_eq!(
        FrameCodec::decode::<BlobMessage>(&frame).unwrap_err(),
        FrameError::TooLarge
    );
}

#[test]
fn body_shorter_than_the_declared_length_is_truncated() {
    let mut frame = 3_u32.to_le_bytes().to_vec();
    frame.extend_from_slice(&[0x08, 0x01]);

    assert_eq!(
        FrameCodec::decode::<BlobMessage>(&frame).unwrap_err(),
        FrameError::Truncated
    );
}

#[test]
fn body_longer_than_the_declared_length_has_trailing_bytes() {
    let mut frame = 2_u32.to_le_bytes().to_vec();
    frame.extend_from_slice(&[0x08, 0x01, 0x00]);

    assert_eq!(
        FrameCodec::decode::<BlobMessage>(&frame).unwrap_err(),
        FrameError::TrailingBytes
    );
}

#[test]
fn concatenated_frames_are_rejected_as_trailing_bytes() {
    let first = FrameCodec::encode(&BlobMessage { value: vec![1] }).unwrap();
    let second = FrameCodec::encode(&BlobMessage { value: vec![2] }).unwrap();
    let mut concatenated = first;
    concatenated.extend_from_slice(&second);

    assert_eq!(
        FrameCodec::decode::<BlobMessage>(&concatenated).unwrap_err(),
        FrameError::TrailingBytes
    );
}

#[test]
fn exact_body_with_invalid_protobuf_is_a_decode_error() {
    let mut frame = 1_u32.to_le_bytes().to_vec();
    frame.push(0x80);

    assert_eq!(
        FrameCodec::decode::<BlobMessage>(&frame).unwrap_err(),
        FrameError::Decode
    );
}

#[test]
fn ordinary_messages_round_trip() {
    let message = BlobMessage {
        value: b"pal-companion".to_vec(),
    };
    let frame = FrameCodec::encode(&message).unwrap();

    assert_eq!(FrameCodec::decode::<BlobMessage>(&frame).unwrap(), message);
}
