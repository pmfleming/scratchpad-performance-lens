//! Opt-in session diagnostics. These consume archived reports or an explicitly
//! selected probe; they do not publish dashboard evidence or relax budgets.
mod compare;
mod dirty_save;

pub(crate) use compare::{compare_session_baseline, CompareSessionBaselineArgs};
pub(crate) use dirty_save::{measure_large_dirty_save, MeasureLargeDirtySaveArgs};
