//! Non-preemptive, bounded-priority execution. Admission is bounded by the caller.
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};
use tokio::sync::Notify;

#[derive(Default)]
pub(super) struct Scheduler {
    state: Mutex<State>,
    changed: Notify,
}
#[derive(Default)]
struct State {
    sequence: u64,
    active: Option<u64>,
    critical_streak: usize,
    normal: VecDeque<u64>,
    critical: VecDeque<u64>,
}
impl State {
    fn next(&mut self) {
        if self.active.is_some() {
            return;
        }
        self.active =
            if !self.critical.is_empty() && (self.critical_streak < 4 || self.normal.is_empty()) {
                self.critical_streak = (self.critical_streak + 1).min(4);
                self.critical.pop_front()
            } else {
                self.critical_streak = 0;
                self.normal.pop_front()
            };
    }
}
pub(super) struct Turn {
    scheduler: Arc<Scheduler>,
    id: u64,
}
impl Scheduler {
    pub async fn acquire(self: &Arc<Self>, critical: bool) -> Turn {
        let turn = {
            let mut s = self.state.lock().expect("scheduler lock");
            s.sequence += 1;
            let id = s.sequence;
            if critical {
                s.critical.push_back(id);
            } else {
                s.normal.push_back(id);
            }
            s.next();
            Turn {
                scheduler: self.clone(),
                id,
            }
        };
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.state.lock().expect("scheduler lock").active == Some(turn.id) {
                return turn;
            }
            notified.await;
        }
    }
}
impl Drop for Turn {
    fn drop(&mut self) {
        let mut s = self.scheduler.state.lock().expect("scheduler lock");
        if s.active == Some(self.id) {
            s.active = None;
        } else {
            s.normal.retain(|id| *id != self.id);
            s.critical.retain(|id| *id != self.id);
        }
        s.next();
        drop(s);
        self.scheduler.changed.notify_waiters();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn critical_overtakes_normal_but_normal_cannot_starve() {
        let scheduler = Arc::new(Scheduler::default());
        let hold = scheduler.acquire(false).await;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut tasks = vec![];
        for (id, critical) in [
            (0, false),
            (1, false),
            (2, true),
            (3, true),
            (4, true),
            (5, true),
            (6, true),
        ] {
            let scheduler = scheduler.clone();
            let tx = tx.clone();
            tasks.push(tokio::spawn(async move {
                let _turn = scheduler.acquire(critical).await;
                tx.send(id).unwrap();
            }));
            tokio::task::yield_now().await;
        }
        drop(hold);
        let mut order = vec![];
        for _ in 0..7 {
            order.push(rx.recv().await.unwrap());
        }
        assert_eq!(order, [2, 3, 4, 5, 0, 6, 1]);
        for task in tasks {
            task.await.unwrap();
        }
    }
    #[tokio::test]
    async fn cancelled_queue_entry_does_not_wedge_scheduler() {
        let scheduler = Arc::new(Scheduler::default());
        let hold = scheduler.acquire(false).await;
        let s = scheduler.clone();
        let task = tokio::spawn(async move { s.acquire(true).await });
        tokio::task::yield_now().await;
        task.abort();
        let _ = task.await;
        drop(hold);
        tokio::time::timeout(std::time::Duration::from_secs(1), scheduler.acquire(false))
            .await
            .unwrap();
    }
}
