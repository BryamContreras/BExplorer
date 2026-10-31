use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::fs::transfer_queue::TransferState;

const RATE_WINDOW: Duration = Duration::from_secs(2);

/// Samples progress on the existing UI timer, including when the copy worker
/// is waiting inside a disk operation and cannot send another progress event.
#[derive(Default)]
pub(super) struct TransferRate {
    samples: VecDeque<(Instant, u64)>,
    copying: bool,
}

impl TransferRate {
    pub(super) fn sample(&mut self, now: Instant, bytes: u64, state: TransferState) {
        let copying = state == TransferState::Copying;
        if !copying || !self.copying || self.samples.back().is_some_and(|sample| bytes < sample.1) {
            self.samples.clear();
        }
        self.copying = copying;
        self.samples.push_back((now, bytes));
        while self.samples.len() > 2 && now.duration_since(self.samples[1].0) >= RATE_WINDOW {
            self.samples.pop_front();
        }
    }

    pub(super) fn bytes_per_second(&self) -> f64 {
        let (Some(first), Some(last)) = (self.samples.front(), self.samples.back()) else {
            return 0.0;
        };
        let elapsed = last.0.duration_since(first.0).as_secs_f64();
        if !self.copying || elapsed <= 0.0 {
            return 0.0;
        }
        last.1.saturating_sub(first.1) as f64 / elapsed
    }
}

pub(super) fn format_transfer_speed(bytes_per_second: f64, spanish: bool) -> String {
    let mut speed = if bytes_per_second.is_finite() {
        bytes_per_second.max(0.0)
    } else {
        0.0
    };
    let units = ["B/s", "KB/s", "MB/s", "GB/s", "TB/s"];
    let mut unit = 0;
    while speed >= 1024.0 && unit < units.len() - 1 {
        speed /= 1024.0;
        unit += 1;
    }
    let number = if unit == 0 {
        format!("{speed:.0}")
    } else {
        format!("{speed:.1}")
    };
    let number = if spanish {
        number.replace('.', ",")
    } else {
        number
    };
    format!("{number} {}", units[unit])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stalled_disk_reaches_zero_without_another_worker_message() {
        let start = Instant::now();
        let mut rate = TransferRate::default();
        rate.sample(start, 0, TransferState::Copying);
        rate.sample(
            start + Duration::from_secs(1),
            1_048_576,
            TransferState::Copying,
        );
        assert_eq!(rate.bytes_per_second(), 1_048_576.0);
        for second in 2..=4 {
            rate.sample(
                start + Duration::from_secs(second),
                1_048_576,
                TransferState::Copying,
            );
        }
        assert_eq!(rate.bytes_per_second(), 0.0);
        rate.sample(
            start + Duration::from_secs(5),
            2_097_152,
            TransferState::Copying,
        );
        assert_eq!(rate.bytes_per_second(), 524_288.0);
    }

    #[test]
    fn pause_and_resume_do_not_reuse_the_previous_speed() {
        let start = Instant::now();
        let mut rate = TransferRate::default();
        rate.sample(start, 0, TransferState::Copying);
        rate.sample(start + Duration::from_secs(1), 1024, TransferState::Copying);
        rate.sample(start + Duration::from_secs(2), 1024, TransferState::Paused);
        assert_eq!(rate.bytes_per_second(), 0.0);
        rate.sample(
            start + Duration::from_secs(30),
            1024,
            TransferState::Copying,
        );
        assert_eq!(rate.bytes_per_second(), 0.0);
        rate.sample(
            start + Duration::from_secs(31),
            2048,
            TransferState::Copying,
        );
        assert_eq!(rate.bytes_per_second(), 1024.0);
    }

    #[test]
    fn speed_units_and_decimal_separator_follow_the_language() {
        assert_eq!(format_transfer_speed(0.2 * 1_048_576.0, true), "204,8 KB/s");
        assert_eq!(format_transfer_speed(1.2 * 1_048_576.0, true), "1,2 MB/s");
        assert_eq!(format_transfer_speed(1.2 * 1_048_576.0, false), "1.2 MB/s");
        assert_eq!(
            format_transfer_speed(2.0 * 1_073_741_824.0, true),
            "2,0 GB/s"
        );
        assert_eq!(format_transfer_speed(f64::NAN, true), "0 B/s");
    }
}
