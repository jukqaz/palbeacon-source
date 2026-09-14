use pal_protocol::{MAX_FRAME_LEN, MAX_PAYLOAD_LEN, v2::LocalEnvelope};
use pal_windows_ipc::framed_stream::{FramedStream, StreamError};

fn raw_frame(body: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(body.len() + 4);
    frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
    frame.extend_from_slice(body);
    frame
}

#[test]
fn accepts_partial_prefix_partial_body_and_multiple_frames() {
    let first = raw_frame(&[1, 2, 3]);
    let second = raw_frame(&[4, 5]);
    let joined = [first.as_slice(), second.as_slice()].concat();
    let mut stream = FramedStream::new();

    stream.push(&joined[..2]).unwrap();
    stream.push(&joined[2..6]).unwrap();
    stream.push(&joined[6..]).unwrap();

    assert_eq!(stream.next_payload().unwrap(), vec![1, 2, 3]);
    assert_eq!(stream.next_payload().unwrap(), vec![4, 5]);
    assert!(stream.next_payload().is_none());
}

#[test]
fn rejects_empty_oversized_and_truncated_frames_without_unbounded_buffering() {
    let mut empty = FramedStream::new();
    assert_eq!(empty.push(&0_u32.to_le_bytes()), Err(StreamError::Empty));

    let mut oversized = FramedStream::new();
    let declared = (MAX_PAYLOAD_LEN as u32) + 1;
    assert_eq!(
        oversized.push(&declared.to_le_bytes()),
        Err(StreamError::TooLarge)
    );
    assert_eq!(oversized.buffered_len(), 0);

    let mut truncated = FramedStream::new();
    truncated.push(&raw_frame(&[1, 2, 3])[..6]).unwrap();
    assert_eq!(truncated.finish(), Err(StreamError::Truncated));
    assert!(truncated.buffered_len() <= MAX_FRAME_LEN);
}

#[test]
fn rejects_malformed_protobuf_and_bounds_queued_frames() {
    let mut malformed = FramedStream::new();
    malformed.push(&raw_frame(&[0xff])).unwrap();
    assert_eq!(
        malformed.next_message::<LocalEnvelope>(),
        Err(StreamError::Malformed)
    );

    let mut bounded = FramedStream::new();
    let small = raw_frame(&[1]);
    let many = small.repeat(20_000);
    assert_eq!(bounded.push(&many), Err(StreamError::QueueFull));
    assert!(bounded.queued_bytes() <= MAX_PAYLOAD_LEN);
    assert!(bounded.buffered_len() <= MAX_FRAME_LEN);
}
