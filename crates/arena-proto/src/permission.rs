//! The permission catalogue. Empty until Story 1.11 seeds it.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A catalogue permission. Every intent maps to exactly one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum Permission {}
