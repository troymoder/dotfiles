// SPDX-License-Identifier: GPL-3.0-only

//! A slider sends values faster than the system takes them. Only the latest
//! matters: while one is being set, the ones after it collapse into the
//! last, which goes when the first is done.

use std::sync::Mutex;

pub struct Latest<T> {
    state: Mutex<State<T>>,
}

struct State<T> {
    busy: bool,
    pending: Option<T>,
}

impl<T: Clone + Send + 'static> Latest<T> {
    pub const fn new() -> Self {
        Self { state: Mutex::new(State { busy: false, pending: None }) }
    }

    /// `value` set with `set`, now or once the one being set is done.
    pub async fn send<F, Fut>(&'static self, value: T, set: F)
    where
        F: Fn(T) -> Fut,
        Fut: Future<Output = ()>,
    {
        {
            let mut state = self.state.lock().unwrap();
            if state.busy {
                state.pending = Some(value);
                return;
            }
            state.busy = true;
        }
        let mut next = Some(value);
        while let Some(value) = next.take() {
            set(value).await;
            let mut state = self.state.lock().unwrap();
            next = state.pending.take();
            if next.is_none() {
                state.busy = false;
            }
        }
    }
}
