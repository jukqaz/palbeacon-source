use std::sync::{Arc, Mutex};

pub struct LatestMailbox<T> {
    state: Mutex<MailboxState<T>>,
}

struct MailboxState<T> {
    generation: u64,
    latest: Option<(u64, Arc<T>)>,
}

impl<T> LatestMailbox<T> {
    pub const fn new() -> Self {
        Self {
            state: Mutex::new(MailboxState {
                generation: 0,
                latest: None,
            }),
        }
    }

    pub fn publish(&self, value: Arc<T>) -> u64 {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.generation = state
            .generation
            .checked_add(1)
            .expect("mailbox generation exhausted");
        let generation = state.generation;
        state.latest = Some((generation, value));
        generation
    }

    pub fn take_latest(&self) -> Option<(u64, Arc<T>)> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .latest
            .take()
    }
}

impl<T> Default for LatestMailbox<T> {
    fn default() -> Self {
        Self::new()
    }
}
