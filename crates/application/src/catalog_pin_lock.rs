// SPDX-License-Identifier: MIT OR Apache-2.0

//! Serializes changes to local catalog pins with catalog-version deletion.

use std::sync::{Mutex, MutexGuard};

use crate::workcopy::WorkcopyError;

static LOCK: Mutex<()> = Mutex::new(());

pub(crate) fn lock() -> Result<MutexGuard<'static, ()>, WorkcopyError> {
    LOCK.lock()
        .map_err(|_| WorkcopyError::new("storage_write_failed"))
}
