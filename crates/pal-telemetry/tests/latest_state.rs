use std::time::Duration;

use pal_protocol::v2::{CoordinateSpace, HeadingSource, ResumeCursor, SubscribeResponse};
use pal_telemetry::{
    LatestConnectOutcome, LatestPublishOutcome, LatestTelemetry, SequenceCursor, SequenceDecision,
};

const SUBJECT: [u8; 32] = [0x42; 32];
const BOOT_A: [u8; 16] = [0x11; 16];
const BOOT_B: [u8; 16] = [0x22; 16];
const PROFILE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn sample(boot_id: &[u8], sequence: u64) -> SubscribeResponse {
    SubscribeResponse {
        protocol_version: pal_domain::PROTOCOL_VERSION,
        world_alias: "world-a".to_owned(),
        subject_id: SUBJECT.to_vec(),
        boot_id: boot_id.to_vec(),
        sequence,
        rest_completed_at_unix_ms: 1_700_000_000_000,
        age_at_emit_ms: 15,
        position_x: sequence as f64,
        position_y: 2.0,
        position_z: 3.0,
        heading_degrees: Some(90.0),
        heading_source: HeadingSource::RotationZValidated as i32,
        trace_id: [0x33; 16].to_vec(),
        coordinate_space: CoordinateSpace::OfficialGameDataWorldV1 as i32,
        coordinate_profile_sha256: PROFILE.to_owned(),
    }
}

fn latest() -> LatestTelemetry {
    LatestTelemetry::new("world-a", SUBJECT, PROFILE).unwrap()
}

#[tokio::test]
async fn publishing_ten_thousand_samples_keeps_one_latest_value() {
    let latest = latest();
    assert_eq!(latest.connect(1), LatestConnectOutcome::Accepted);
    for sequence in 1..=10_000 {
        assert!(matches!(
            latest.publish(1, sample(&BOOT_A, sequence)).unwrap(),
            LatestPublishOutcome::Accepted | LatestPublishOutcome::AcceptedWithGap { .. }
        ));
    }

    assert_eq!(latest.buffered_state_count(), 1);
    assert_eq!(latest.current().unwrap().sequence, 10_000);
}

#[tokio::test]
async fn resume_cursor_obeys_old_equal_and_different_boot_semantics() {
    let latest = latest();
    latest.connect(1);
    latest.publish(1, sample(&BOOT_A, 7)).unwrap();

    let old = latest
        .next_after(Some(ResumeCursor {
            boot_id: BOOT_A.to_vec(),
            sequence: 4,
        }))
        .await
        .unwrap();
    assert_eq!(old.sequence, 7);

    let different_boot = latest
        .next_after(Some(ResumeCursor {
            boot_id: BOOT_B.to_vec(),
            sequence: 99,
        }))
        .await
        .unwrap();
    assert_eq!(different_boot.sequence, 7);

    let waiting = latest.next_after(Some(ResumeCursor {
        boot_id: BOOT_A.to_vec(),
        sequence: 7,
    }));
    assert!(
        tokio::time::timeout(Duration::from_millis(25), waiting)
            .await
            .is_err()
    );

    latest.publish(1, sample(&BOOT_A, 9)).unwrap();
    let next = latest
        .next_after(Some(ResumeCursor {
            boot_id: BOOT_A.to_vec(),
            sequence: 7,
        }))
        .await
        .unwrap();
    assert_eq!(next.sequence, 9);
}

#[tokio::test]
async fn same_boot_cursor_ahead_never_receives_an_older_sequence() {
    let latest = latest();
    latest.connect(1);
    latest.publish(1, sample(&BOOT_A, 7)).unwrap();
    let ahead = ResumeCursor {
        boot_id: BOOT_A.to_vec(),
        sequence: 9,
    };
    assert!(
        tokio::time::timeout(
            Duration::from_millis(25),
            latest.next_after(Some(ahead.clone())),
        )
        .await
        .is_err()
    );

    latest.publish(1, sample(&BOOT_A, 10)).unwrap();
    assert_eq!(latest.next_after(Some(ahead)).await.unwrap().sequence, 10);
}

#[test]
fn publication_is_pair_profile_generation_boot_and_sequence_bound() {
    let latest = latest();
    assert_eq!(latest.connect(7), LatestConnectOutcome::Accepted);
    assert_eq!(
        latest.publish(7, sample(&BOOT_A, 5)).unwrap(),
        LatestPublishOutcome::Accepted
    );

    let mut wrong_subject = sample(&BOOT_A, 6);
    wrong_subject.subject_id = [0x99; 32].to_vec();
    assert_eq!(
        latest.publish(7, wrong_subject).unwrap(),
        LatestPublishOutcome::IdentityMismatchDropped
    );

    let mut wrong_profile = sample(&BOOT_A, 6);
    wrong_profile.coordinate_profile_sha256 =
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned();
    assert!(latest.publish(7, wrong_profile).is_err());
    assert_eq!(
        latest.publish(7, sample(&BOOT_A, 5)).unwrap(),
        LatestPublishOutcome::DuplicateDropped
    );
    assert_eq!(
        latest.publish(7, sample(&BOOT_A, 4)).unwrap(),
        LatestPublishOutcome::OutOfOrderDropped
    );
    assert_eq!(
        latest.publish(7, sample(&BOOT_B, 6)).unwrap(),
        LatestPublishOutcome::BootMismatchDropped
    );
    assert_eq!(
        latest.publish(6, sample(&BOOT_A, 6)).unwrap(),
        LatestPublishOutcome::OldGenerationDropped
    );

    assert_eq!(latest.connect(8), LatestConnectOutcome::Accepted);
    assert!(latest.current().is_none());
    assert_eq!(
        latest.publish(8, sample(&BOOT_B, 1)).unwrap(),
        LatestPublishOutcome::Accepted
    );
    assert_eq!(
        latest.publish(8, sample(&BOOT_A, 99)).unwrap(),
        LatestPublishOutcome::RetiredBootDropped
    );
    assert_eq!(
        latest.publish(7, sample(&BOOT_A, 99)).unwrap(),
        LatestPublishOutcome::OldGenerationDropped
    );
    let current = latest.current().unwrap();
    assert_eq!(current.boot_id, BOOT_B);
    assert_eq!(current.sequence, 1);

    let diagnostics = latest.diagnostics();
    assert_eq!(diagnostics.identity_mismatch_drops, 1);
    assert_eq!(diagnostics.duplicate_drops, 1);
    assert_eq!(diagnostics.out_of_order_drops, 1);
    assert_eq!(diagnostics.boot_mismatch_drops, 1);
    assert_eq!(diagnostics.old_generation_drops, 2);
    assert_eq!(diagnostics.retired_boot_drops, 1);
}

#[test]
fn sequence_cursor_accepts_gaps_and_counts_rollbacks() {
    let mut cursor = SequenceCursor::new(BOOT_A);
    assert_eq!(cursor.decide(1), SequenceDecision::Accept);
    assert_eq!(
        cursor.decide(4),
        SequenceDecision::AcceptWithGap { missing: 2 }
    );
    assert_eq!(cursor.decide(4), SequenceDecision::DropDuplicate);
    assert_eq!(cursor.decide(3), SequenceDecision::DropOutOfOrder);
    let diagnostics = cursor.diagnostics();
    assert_eq!(diagnostics.gap_events, 1);
    assert_eq!(diagnostics.missing_sequences, 2);
    assert_eq!(diagnostics.duplicate_drops, 1);
    assert_eq!(diagnostics.out_of_order_drops, 1);
}

#[test]
fn closed_latest_state_is_terminal_and_cannot_be_reopened_by_publish() {
    let latest = latest();
    latest.connect(1);
    latest.publish(1, sample(&BOOT_A, 1)).unwrap();
    latest.close();
    assert!(latest.publish(1, sample(&BOOT_A, 2)).is_err());
    assert!(latest.current().is_none());
}
