use std::time::Duration;

const COOLDOWN_SECONDS: f64 = 95.0;

#[derive(Default)]
pub struct Bkb {
    started_at: Option<f64>,
    restore_maximized: Option<bool>,
    uses: u8,
}

impl Bkb {
    pub fn active(&self) -> bool {
        self.restore_maximized.is_some()
    }

    fn effect_seconds(&self) -> f64 {
        match self.uses {
            0 | 1 => 9.0,
            2 => 8.0,
            _ => 7.0,
        }
    }

    pub fn activate(&mut self, now: f64, was_maximized: bool) -> bool {
        if self.started_at.is_some() {
            return false;
        }
        self.started_at = Some(now);
        self.restore_maximized = Some(was_maximized);
        self.uses = (self.uses + 1).min(3);
        true
    }

    pub fn advance(&mut self, now: f64) -> Option<bool> {
        let start = self.started_at?;
        if now < start + self.effect_seconds() {
            return None;
        }
        if now >= start + COOLDOWN_SECONDS {
            self.started_at = None;
        }
        self.restore_maximized.take()
    }

    pub fn countdown(&self, now: f64) -> Option<(u64, Duration)> {
        let length = if self.active() {
            self.effect_seconds()
        } else {
            COOLDOWN_SECONDS
        };
        let left = (self.started_at? + length - now).max(0.0);
        let seconds = left.ceil() as u64;
        let next_change = left - seconds.saturating_sub(1) as f64;
        Some((seconds, Duration::from_secs_f64(next_change)))
    }
}

#[cfg(test)]
mod tests {
    use super::Bkb;

    #[test]
    fn successive_uses_last_nine_eight_then_seven_on_a_ninety_five_second_cooldown() {
        for was_maximized in [false, true] {
            let mut bkb = Bkb::default();
            for (use_index, duration) in [9, 8, 7, 7].into_iter().enumerate() {
                let start = 10.0 + use_index as f64 * 95.0;
                let end = start + duration as f64;
                assert!(bkb.activate(start, was_maximized));
                assert_eq!(bkb.countdown(start).unwrap().0, duration);
                assert_eq!(bkb.advance(end - 0.001), None);
                assert!(bkb.active());
                assert!(!bkb.activate(end - 0.001, !was_maximized));
                assert_eq!(bkb.advance(end), Some(was_maximized));
                assert!(!bkb.active());
                assert_eq!(bkb.countdown(end).unwrap().0, 95 - duration);
                assert_eq!(bkb.advance(start + 94.999), None);
                assert!(!bkb.activate(start + 94.999, false));
                assert_eq!(bkb.advance(start + 95.0), None);
                assert!(bkb.countdown(start + 95.0).is_none());
            }
        }
    }

    #[test]
    fn overdue_frame_restores_once_and_rearms_immediately() {
        let mut bkb = Bkb::default();
        assert!(bkb.activate(3.0, true));
        assert_eq!(bkb.advance(175.0), Some(true));
        assert_eq!(bkb.advance(175.0), None);
        assert!(bkb.countdown(175.0).is_none());
        assert!(bkb.activate(175.0, false));
    }
}
