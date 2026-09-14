use std::sync::{Arc, Barrier};

use pal_overlay_win::LatestMailbox;

#[test]
fn mailbox_keeps_only_the_latest_arc() {
    let mailbox = LatestMailbox::new();
    let first = Arc::new(String::from("first"));
    let latest = Arc::new(String::from("latest"));

    assert_eq!(mailbox.publish(Arc::clone(&first)), 1);
    assert_eq!(mailbox.publish(Arc::clone(&latest)), 2);

    let (generation, value) = mailbox.take_latest().expect("latest value");
    assert_eq!(generation, 2);
    assert!(Arc::ptr_eq(&value, &latest));
    assert!(mailbox.take_latest().is_none());
}

#[test]
fn empty_mailbox_is_valid_for_preview_shell() {
    let mailbox = LatestMailbox::<u64>::new();

    assert!(mailbox.take_latest().is_none());
}

#[test]
fn concurrent_publishers_cannot_overwrite_a_newer_generation_with_an_older_value() {
    const PUBLISHERS: usize = 32;
    const VALUES_PER_PUBLISHER: usize = 2_000;

    let mailbox = Arc::new(LatestMailbox::new());
    let barrier = Arc::new(Barrier::new(PUBLISHERS));
    let published = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..PUBLISHERS)
            .map(|publisher| {
                let mailbox = Arc::clone(&mailbox);
                let barrier = Arc::clone(&barrier);
                scope.spawn(move || {
                    barrier.wait();
                    (0..VALUES_PER_PUBLISHER)
                        .map(|offset| {
                            let value = (publisher * VALUES_PER_PUBLISHER + offset) as u64;
                            let generation = mailbox.publish(Arc::new(value));
                            (generation, value)
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();

        handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("publisher thread"))
            .collect::<Vec<_>>()
    });

    let expected = published
        .into_iter()
        .max_by_key(|(generation, _)| *generation)
        .expect("at least one publication");
    let actual = mailbox.take_latest().expect("latest publication");
    assert_eq!(actual.0, expected.0);
    assert_eq!(*actual.1, expected.1);
}
