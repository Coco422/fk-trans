//! Deciding when a translation starts, and where what follows appears.
//!
//! The CGEventTap callback is a bad place to make decisions: it runs on a thread
//! macOS stops delivering events to if it is slow, and nothing about it can be
//! exercised without a live session. So the decisions live here as pure functions
//! over plain data, and the callback only converts events into that data.
//!
//! [`placement`] is the first of those: where a small window goes near a
//! selection. The mouse-event decision logic — drag thresholds, double clicks,
//! cooldowns — belongs here too, and moves across with the rest of the trigger
//! rework.

pub mod placement;
