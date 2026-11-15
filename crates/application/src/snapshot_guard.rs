// SPDX-License-Identifier: MIT OR Apache-2.0

//! Process-wide order: this guard is acquired before journal `running` and
//! before any snapshot/workcopy filesystem mutation. Poisoning fails closed.

use std::sync::{Mutex, MutexGuard};

static GUARD: Mutex<()> = Mutex::new(());

pub(crate) fn lock() -> Result<MutexGuard<'static, ()>, ()> {
    GUARD.lock().map_err(|_| ())
}
