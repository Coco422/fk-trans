//! Getting the selected text, and where it is on screen.
//!
//! Two strategies, tried in order:
//!
//! 1. **Accessibility** — ask the focused app for its selection. Reads the text
//!    directly, so the pasteboard is never touched and no keystroke is
//!    synthesised. Also the only way to learn where the selection *is*.
//! 2. **Pasteboard** — synthesise Cmd+C and read the result. Works almost
//!    everywhere, but takes over the user's clipboard for a moment and cannot
//!    report geometry.
//!
//! Which one to try, and whether to fall back, is [`should_fall_back_to_pasteboard`]
//! — a pure function, because this decision is the difference between a reliable
//! trigger and a flaky one, and it is worth testing without a GUI in the loop.

mod accessibility;

use crate::clipboard::manager::ClipboardManager;
use crate::macos::ax::{Failure, ScreenRect};
use tokio::time::{sleep, Duration};
use ts_rs::TS;

/// How long to let the frontmost app finish handling the mouse-up before asking
/// it anything.
///
/// Both strategies race the app otherwise: a selection is created as the app
/// processes the mouse event, and asking immediately can legitimately see the
/// previous selection, or none. Measured from the event, not from when the
/// pipeline happened to start.
const SETTLE_DELAY: Duration = Duration::from_millis(120);

/// Which strategy produced the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "types.ts")]
pub enum CaptureStrategy {
    Accessibility,
    Pasteboard,
}

impl CaptureStrategy {
    pub fn label(self) -> &'static str {
        match self {
            Self::Accessibility => "accessibility",
            Self::Pasteboard => "pasteboard",
        }
    }
}

/// What one strategy did, kept whether or not it succeeded so diagnostics can
/// show which strategy an app needed — the only way to build the per-app picture
/// without asking the user to run a separate tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
    pub strategy: CaptureStrategy,
    pub outcome: AttemptOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttemptOutcome {
    Captured { chars: usize, has_bounds: bool },
    /// The strategy ran and could not get a selection.
    Failed { reason: String },
    /// The strategy was not worth trying.
    Skipped { reason: String },
}

impl Attempt {
    fn describe(&self) -> String {
        let strategy = self.strategy.label();
        match &self.outcome {
            AttemptOutcome::Captured { chars, has_bounds } => format!(
                "{strategy}: read {chars} chars{}",
                if *has_bounds { ", with position" } else { "" }
            ),
            AttemptOutcome::Failed { reason } => format!("{strategy}: {reason}"),
            AttemptOutcome::Skipped { reason } => format!("{strategy}: skipped ({reason})"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Captured {
    pub text: String,
    /// Screen position of the selection, in points. Only Accessibility can
    /// provide this; the trigger point needs it, and without it the caller
    /// positions relative to the mouse instead.
    pub bounds: Option<ScreenRect>,
    pub strategy: CaptureStrategy,
}

#[derive(Debug, Clone)]
pub struct CaptureOutcome {
    pub captured: Option<Captured>,
    pub attempts: Vec<Attempt>,
}

impl CaptureOutcome {
    /// A one-line account of what was tried, for the diagnostics report.
    pub fn summary(&self) -> String {
        if self.attempts.is_empty() {
            return "no strategy attempted".to_string();
        }
        self.attempts
            .iter()
            .map(Attempt::describe)
            .collect::<Vec<_>>()
            .join("; ")
    }

    fn captured(text: String, bounds: Option<ScreenRect>, strategy: CaptureStrategy) -> Self {
        let attempt = Attempt {
            strategy,
            outcome: AttemptOutcome::Captured {
                chars: text.chars().count(),
                has_bounds: bounds.is_some(),
            },
        };
        Self {
            captured: Some(Captured {
                text,
                bounds,
                strategy,
            }),
            attempts: vec![attempt],
        }
    }
}

/// Whether the pasteboard is worth trying after Accessibility failed.
///
/// The distinction that matters is between "this app cannot tell me its
/// selection" — where Cmd+C is exactly the right fallback — and the cases where
/// Cmd+C would be pointless or wrong:
///
/// * **No permission**: synthesising Cmd+C needs the same permission, so the
///   fallback cannot work either. Asking would just fail twice.
/// * **Nothing focused / nothing selected**: there is no selection for Cmd+C to
///   copy. Pressing it anyway can copy something unrelated, or nothing.
/// * **Busy** (`Blocked`): the app is mid-something. Copying into it now is a
///   coin flip, and the next middle-click will re-read cleanly.
///
/// Pure, and deliberately exhaustive: a new `Failure` variant will not compile
/// until someone decides what it means here.
pub fn should_fall_back_to_pasteboard(failure: &Failure) -> bool {
    match failure {
        // The app does not implement the attribute, but it will still handle a
        // copy — this is the case the fallback exists for.
        Failure::Unsupported => true,
        // An error we do not recognise: try, and let the attempt be recorded.
        Failure::Other { .. } => true,
        Failure::NotTrusted | Failure::NoFocusedElement | Failure::NoSelection | Failure::Blocked { .. } => false,
    }
}

/// Read the current selection, trying Accessibility first.
pub async fn capture_selection(
    clipboard: &ClipboardManager,
    pasteboard_allowed: bool,
) -> CaptureOutcome {
    let mut attempts = Vec::new();

    sleep(SETTLE_DELAY).await;

    let ax_failure = match accessibility::read().await {
        Ok(captured) => {
            return CaptureOutcome::captured(
                captured.text,
                captured.bounds,
                CaptureStrategy::Accessibility,
            )
        }
        Err(failure) => {
            let described = describe_failure(&failure);
            attempts.push(Attempt {
                strategy: CaptureStrategy::Accessibility,
                outcome: AttemptOutcome::Failed {
                    reason: described.clone(),
                },
            });
            failure
        }
    };

    if !pasteboard_allowed || !should_fall_back_to_pasteboard(&ax_failure) {
        attempts.push(Attempt {
            strategy: CaptureStrategy::Pasteboard,
            outcome: AttemptOutcome::Skipped {
                reason: if pasteboard_allowed {
                    why_not_pasteboard(&ax_failure).to_string()
                } else {
                    "not allowed for this trigger".to_string()
                },
            },
        });
        return CaptureOutcome {
            captured: None,
            attempts,
        };
    }

    match clipboard.capture_selected_text().await {
        Some(text) => {
            let mut outcome = CaptureOutcome::captured(text, None, CaptureStrategy::Pasteboard);
            // The accessibility attempt came first and is what put us here.
            attempts.push(outcome.attempts.remove(0));
            outcome.attempts = attempts;
            outcome
        }
        None => {
            attempts.push(Attempt {
                strategy: CaptureStrategy::Pasteboard,
                outcome: AttemptOutcome::Failed {
                    reason: "the copy produced no text".to_string(),
                },
            });
            CaptureOutcome {
                captured: None,
                attempts,
            }
        }
    }
}

/// Why the pasteboard was not tried, in words that make sense in a report.
fn why_not_pasteboard(failure: &Failure) -> &'static str {
    match failure {
        Failure::NotTrusted => "without Accessibility permission a synthesised copy cannot work either",
        Failure::NoFocusedElement => "nothing is focused, so there is nothing to copy",
        Failure::NoSelection => "nothing is selected, so there is nothing to copy",
        Failure::Blocked { .. } => "the app was busy; the next trigger will try again",
        // Only reachable if the caller disallowed the fallback; kept exhaustive.
        Failure::Unsupported | Failure::Other { .. } => "the pasteboard was not tried",
    }
}

fn describe_failure(failure: &Failure) -> String {
    match failure.detail() {
        Some(detail) => format!("{} ({detail})", failure.label()),
        None => failure.label().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The case the fallback exists for: the app has a selection it will not
    /// describe to Accessibility, but it will still copy it.
    #[test]
    fn an_unsupported_app_falls_back_to_the_pasteboard() {
        assert!(should_fall_back_to_pasteboard(&Failure::Unsupported));
    }

    /// Cmd+C needs the same permission, so this would fail twice and risk
    /// synthesising a keystroke into an app we cannot even read.
    #[test]
    fn a_missing_permission_does_not_fall_back() {
        assert!(!should_fall_back_to_pasteboard(&Failure::NotTrusted));
    }

    /// Nothing is selected, so there is nothing for Cmd+C to copy — and pressing
    /// it can copy something unrelated instead.
    #[test]
    fn no_selection_does_not_fall_back() {
        assert!(!should_fall_back_to_pasteboard(&Failure::NoSelection));
        assert!(!should_fall_back_to_pasteboard(&Failure::NoFocusedElement));
    }

    /// A busy app is mid-something; copying into it now is a coin flip, and the
    /// next trigger will re-read cleanly.
    #[test]
    fn a_busy_app_is_retried_rather_than_fallen_back_from() {
        let busy = Failure::Blocked {
            code: "kAXErrorCannotComplete",
        };
        assert!(!should_fall_back_to_pasteboard(&busy));
    }

    #[test]
    fn an_unrecognised_failure_still_tries_the_fallback() {
        assert!(should_fall_back_to_pasteboard(&Failure::Other {
            code: "kAXErrorWeird"
        }));
    }

    #[test]
    fn a_successful_capture_has_no_pasteboard_attempt() {
        let outcome = CaptureOutcome::captured(
            "hello".to_string(),
            Some(ScreenRect {
                x: 1.0,
                y: 2.0,
                width: 3.0,
                height: 4.0,
            }),
            CaptureStrategy::Accessibility,
        );

        assert_eq!(outcome.attempts.len(), 1);
        assert_eq!(outcome.attempts[0].strategy, CaptureStrategy::Accessibility);
        assert!(outcome.summary().contains("with position"));
    }

    #[test]
    fn a_capture_without_geometry_says_so() {
        let outcome =
            CaptureOutcome::captured("hi".to_string(), None, CaptureStrategy::Pasteboard);
        assert!(outcome.summary().contains("read 2 chars"));
        assert!(!outcome.summary().contains("with position"));
    }

    #[test]
    fn a_failure_summary_names_the_strategy_and_the_reason() {
        let outcome = CaptureOutcome {
            captured: None,
            attempts: vec![
                Attempt {
                    strategy: CaptureStrategy::Accessibility,
                    outcome: AttemptOutcome::Failed {
                        reason: "unsupported".to_string(),
                    },
                },
                Attempt {
                    strategy: CaptureStrategy::Pasteboard,
                    outcome: AttemptOutcome::Failed {
                        reason: "no text copied".to_string(),
                    },
                },
            ],
        };

        let summary = outcome.summary();
        assert!(summary.contains("accessibility: unsupported"), "got: {summary}");
        assert!(summary.contains("pasteboard: no text copied"), "got: {summary}");
    }

    #[test]
    fn an_empty_outcome_summarises_as_such() {
        let outcome = CaptureOutcome {
            captured: None,
            attempts: Vec::new(),
        };
        assert_eq!(outcome.summary(), "no strategy attempted");
    }
}
