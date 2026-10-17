//! The translation pipeline's single-flight gate.
//!
//! Only one translation may run at a time, so a plain "am I running?" flag was
//! used to drop overlapping triggers. The flag alone is not enough: if a run got
//! wedged (a hung HTTP request, a clipboard call that never returned) the flag
//! stayed set and *every* later trigger was silently discarded — the app looked
//! alive but did nothing, which is the worst possible failure mode for a tool
//! whose whole job is "middle-click and get a translation".
//!
//! The gate therefore records when the current run started and lets a later
//! trigger reclaim it after a limit. Reclaiming means the wedged run may still be
//! alive; that is the deliberate trade — a possible duplicate request beats a
//! permanently dead app. Tokens keep a stale run's release from clearing the gate
//! that a newer run now owns.

use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

/// How long one run may hold the gate before a later trigger may reclaim it.
pub const DEFAULT_GATE_LIMIT: Duration = Duration::from_secs(90);

const IDLE: i64 = i64::MIN;

/// Identifies one run's ownership of the gate. A release only takes effect if the
/// gate is still owned by this token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GateToken(i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateOutcome {
    /// The gate was free; this run now owns it.
    Acquired(GateToken),
    /// A live run holds the gate and is within its limit; drop this trigger.
    Busy { held_for_ms: i64 },
    /// A previous run outlived its limit and is presumed wedged. This run has
    /// taken the gate over.
    Reclaimed {
        token: GateToken,
        held_for_ms: i64,
    },
}

#[derive(Debug)]
pub struct PipelineGate {
    /// `IDLE` when free, otherwise the millisecond timestamp of the owning run.
    /// The sentinel is `i64::MIN` rather than `0` because a caller-supplied
    /// timestamp of `0` is legitimate and must not read as a free gate.
    owner_ms: AtomicI64,
    limit_ms: i64,
}

impl Default for PipelineGate {
    fn default() -> Self {
        Self::new(DEFAULT_GATE_LIMIT)
    }
}

impl PipelineGate {
    pub fn new(limit: Duration) -> Self {
        Self {
            owner_ms: AtomicI64::new(IDLE),
            limit_ms: limit.as_millis() as i64,
        }
    }

    /// `now_ms` is passed in rather than read from the clock so the whole
    /// decision is a pure function of its inputs and can be tested without
    /// sleeping.
    pub fn try_acquire(&self, now_ms: i64) -> GateOutcome {
        match self.owner_ms.compare_exchange(
            IDLE,
            now_ms,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            Ok(_) => return GateOutcome::Acquired(GateToken(now_ms)),
            Err(current) => {
                let held_for_ms = now_ms.saturating_sub(current);
                if held_for_ms < self.limit_ms {
                    return GateOutcome::Busy { held_for_ms };
                }
                // Reclaim: only wins if the owner has not released in the meantime.
                match self.owner_ms.compare_exchange(
                    current,
                    now_ms,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                ) {
                    Ok(_) => GateOutcome::Reclaimed {
                        token: GateToken(now_ms),
                        held_for_ms,
                    },
                    // Someone released or replaced it; report as busy and let the
                    // next trigger retry rather than stealing a fresh run's gate.
                    Err(raced) => GateOutcome::Busy {
                        held_for_ms: now_ms.saturating_sub(raced),
                    },
                }
            }
        }
    }

    /// Releases the gate only if `token` still owns it, so a wedged run that
    /// finally unwinds cannot clear a newer run's ownership.
    pub fn release(&self, token: GateToken) {
        let _ = self.owner_ms.compare_exchange(
            token.0,
            IDLE,
            Ordering::SeqCst,
            Ordering::SeqCst,
        );
    }

    /// How long the current owner has held the gate, or `None` when free.
    pub fn held_for_ms(&self, now_ms: i64) -> Option<i64> {
        match self.owner_ms.load(Ordering::SeqCst) {
            IDLE => None,
            owner => Some(now_ms.saturating_sub(owner)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMIT_MS: i64 = 90_000;

    fn gate() -> PipelineGate {
        PipelineGate::new(Duration::from_millis(LIMIT_MS as u64))
    }

    #[test]
    fn a_free_gate_is_acquired() {
        let gate = gate();
        assert!(matches!(
            gate.try_acquire(1_000),
            GateOutcome::Acquired(_)
        ));
        assert_eq!(gate.held_for_ms(1_050), Some(50));
    }

    /// The behaviour that used to be a silent drop, and still should be while the
    /// running translation is genuinely alive.
    #[test]
    fn a_live_run_keeps_later_triggers_out() {
        let gate = gate();
        let GateOutcome::Acquired(_) = gate.try_acquire(1_000) else {
            panic!("first acquire should succeed");
        };

        let outcome = gate.try_acquire(1_000 + LIMIT_MS - 1);
        assert_eq!(
            outcome,
            GateOutcome::Busy {
                held_for_ms: LIMIT_MS - 1
            }
        );
    }

    /// The regression guard for "a wedged upstream permanently disables the app":
    /// once the limit is exceeded the next trigger must get through.
    #[test]
    fn a_wedged_run_is_reclaimed_after_the_limit() {
        let gate = gate();
        let GateOutcome::Acquired(_) = gate.try_acquire(1_000) else {
            panic!("first acquire should succeed");
        };

        let outcome = gate.try_acquire(1_000 + LIMIT_MS);
        assert_eq!(
            outcome,
            GateOutcome::Reclaimed {
                token: GateToken(1_000 + LIMIT_MS),
                held_for_ms: LIMIT_MS,
            }
        );
    }

    #[test]
    fn releasing_frees_the_gate_for_the_next_run() {
        let gate = gate();
        let GateOutcome::Acquired(token) = gate.try_acquire(1_000) else {
            panic!("first acquire should succeed");
        };

        gate.release(token);

        assert_eq!(gate.held_for_ms(2_000), None);
        assert!(matches!(
            gate.try_acquire(2_000),
            GateOutcome::Acquired(_)
        ));
    }

    /// A wedged run that finally unwinds must not clear the gate a newer run owns.
    #[test]
    fn a_stale_release_does_not_clear_a_newer_run() {
        let gate = gate();
        let GateOutcome::Acquired(stale) = gate.try_acquire(1_000) else {
            panic!("first acquire should succeed");
        };
        let reclaimed_at = 1_000 + LIMIT_MS + 5_000;
        let GateOutcome::Reclaimed { token: current, .. } = gate.try_acquire(reclaimed_at) else {
            panic!("stale run should have been reclaimed");
        };

        gate.release(stale);

        assert_eq!(gate.held_for_ms(reclaimed_at + 10), Some(10));
        assert!(matches!(
            gate.try_acquire(reclaimed_at + 10),
            GateOutcome::Busy { .. }
        ));

        gate.release(current);
        assert_eq!(gate.held_for_ms(reclaimed_at + 20), None);
    }

    /// Regression: `0` is a legitimate timestamp. It must stay distinguishable
    /// from a free gate — an earlier version used `0` as the idle sentinel, so
    /// acquiring at timestamp `0` left the gate reading as free and a second run
    /// walked straight in alongside the first.
    #[test]
    fn a_zero_timestamp_does_not_look_like_a_free_gate() {
        let gate = gate();
        let GateOutcome::Acquired(_) = gate.try_acquire(0) else {
            panic!("first acquire should succeed");
        };

        assert_eq!(gate.held_for_ms(0), Some(0));
        assert!(matches!(gate.try_acquire(1), GateOutcome::Busy { .. }));
    }

    #[test]
    fn the_limit_is_a_strict_boundary() {
        assert_eq!(DEFAULT_GATE_LIMIT, Duration::from_secs(90));
        let gate = gate();

        let GateOutcome::Acquired(_) = gate.try_acquire(0) else {
            panic!("first acquire should succeed");
        };

        // Just under the limit is still busy; exactly at the limit is reclaimable.
        assert!(matches!(
            gate.try_acquire(LIMIT_MS - 1),
            GateOutcome::Busy { .. }
        ));
        assert!(matches!(
            gate.try_acquire(LIMIT_MS),
            GateOutcome::Reclaimed { .. }
        ));
    }
}
