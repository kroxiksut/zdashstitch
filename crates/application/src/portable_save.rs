// SPDX-License-Identifier: MIT OR Apache-2.0

use serde::Serialize;

use crate::{
    offline_dashboards::OfflineDashboardStore,
    offline_pages::OfflinePageStore,
    workcopy::{DraftKind, WorkcopyError, WorkcopyService},
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PortableConversionOutcome {
    pub local_id: String,
    pub kind: DraftKind,
}

impl WorkcopyService {
    /// Convert the current workcopy and atomically save a separate local draft.
    pub fn make_portable(
        &self,
        local_id: &str,
    ) -> Result<PortableConversionOutcome, WorkcopyError> {
        let (file, _, _) = self.load_log(local_id)?;
        let kind = file.kind;
        let root = self.store.root_path();
        let local_id = match kind {
            DraftKind::Page => {
                OfflinePageStore::new(root)
                    .save_converted(self.convert_dependency_free_page(local_id)?)?
                    .local_id
            }
            DraftKind::Dashboard => {
                OfflineDashboardStore::new(root)
                    .save_converted(self.convert_dependency_free_dashboard(local_id)?)?
                    .local_id
            }
        };
        Ok(PortableConversionOutcome { local_id, kind })
    }
}
