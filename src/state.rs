use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::config::Mode;
use crate::gpu::Status;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Action {
    Free(usize),
    Busy(usize),
    FreeAll,
    BusyAll,
}

#[derive(Debug, Default, Clone)]
struct Slot {
    free: bool,
    sent: Option<Instant>,
}

impl Slot {
    fn busy(&mut self) {
        self.free = false;
        self.sent = None;
    }

    fn ready(&self, now: Instant, interval: Duration) -> bool {
        !self.free
            || self
                .sent
                .is_none_or(|sent| now.duration_since(sent) >= interval)
    }

    fn mark(&mut self, now: Instant) {
        self.free = true;
        self.sent = Some(now);
    }
}

pub struct Engine {
    mode: Mode,
    busy: bool,
    remind: Duration,
    slots: HashMap<String, Slot>,
}

impl Engine {
    pub fn new(mode: Mode, busy: bool, remind_seconds: f64) -> Self {
        Self {
            mode,
            busy,
            remind: Duration::from_secs_f64(remind_seconds),
            slots: HashMap::new(),
        }
    }

    pub fn evaluate(&mut self, statuses: &[Status], now: Instant) -> Vec<Action> {
        match self.mode {
            Mode::Any => self.any(statuses, now),
            Mode::All => self.all(statuses, now),
        }
    }

    fn any(&mut self, statuses: &[Status], now: Instant) -> Vec<Action> {
        let mut actions = Vec::new();
        for (index, status) in statuses.iter().enumerate() {
            let slot = self.slots.entry(status.gpu.index.clone()).or_default();
            if !status.free() {
                if self.busy && slot.free {
                    actions.push(Action::Busy(index));
                }
                slot.busy();
                continue;
            }

            if slot.ready(now, self.remind) {
                actions.push(Action::Free(index));
                slot.mark(now);
            }
        }
        actions
    }

    fn all(&mut self, statuses: &[Status], now: Instant) -> Vec<Action> {
        let slot = self.slots.entry("all".to_string()).or_default();
        if !statuses.iter().all(Status::free) {
            if self.busy && slot.free {
                slot.busy();
                return vec![Action::BusyAll];
            }
            slot.busy();
            return Vec::new();
        }

        if slot.ready(now, self.remind) {
            slot.mark(now);
            vec![Action::FreeAll]
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpu::{Gpu, Process, Status};

    fn free(index: &str) -> Status {
        Status {
            gpu: Gpu {
                index: index.to_string(),
                name: "A100".to_string(),
                uuid: None,
                gpu_util: Some(0),
                memory_util: Some(0),
                memory_total: Some(80),
                memory_used: Some(0),
                memory_free: Some(80),
            },
            active: vec![],
            ignored: vec![],
        }
    }

    fn busy(index: &str) -> Status {
        let mut status = free(index);
        status.active.push(Process {
            gpu: index.to_string(),
            name: "python".to_string(),
            pid: Some(10),
            kind: "C".to_string(),
            memory: Some(1000),
            user: Some("junyong".to_string()),
            command: Some("python train.py".to_string()),
        });
        status
    }

    #[test]
    fn any_reports_busy_after_free() {
        let start = Instant::now();
        let mut engine = Engine::new(Mode::Any, true, 30.0);
        assert_eq!(engine.evaluate(&[free("0")], start), vec![Action::Free(0)]);
        assert_eq!(
            engine.evaluate(&[busy("0")], start + Duration::from_secs(5)),
            vec![Action::Busy(0)]
        );
        assert!(
            engine
                .evaluate(&[busy("0")], start + Duration::from_secs(10))
                .is_empty()
        );
    }

    #[test]
    fn all_waits_for_every_gpu() {
        let start = Instant::now();
        let mut engine = Engine::new(Mode::All, true, 30.0);
        assert!(engine.evaluate(&[free("0"), busy("1")], start).is_empty());
        assert_eq!(
            engine.evaluate(&[free("0"), free("1")], start + Duration::from_secs(1)),
            vec![Action::FreeAll]
        );
        assert!(
            engine
                .evaluate(&[free("0"), free("1")], start + Duration::from_secs(10))
                .is_empty()
        );
        assert_eq!(
            engine.evaluate(&[free("0"), busy("1")], start + Duration::from_secs(20)),
            vec![Action::BusyAll]
        );
    }
}
