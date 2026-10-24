// SPDX-License-Identifier: MIT OR Apache-2.0

//! Portable and local data formats for `zdashstitch.* /v1`.
//!
//! This module reads and writes in-memory YAML/JSON. Safe filesystem and ZIP
//! access intentionally belongs to P0-03.

mod deployment_state;
mod error;
mod io;
mod local_bindings;
mod metrics;
mod portable_io;
mod project;
mod validation;

pub use deployment_state::{
    DeployedDashboard, DeployedPage, DeployedWidget, DeploymentFormat, DeploymentState,
    DeploymentStatus, DeploymentStorageScope,
};
pub use error::{FormatError, Result};
pub use io::{from_json_str, from_yaml_str, to_json_string, to_yaml_string};
pub use local_bindings::{
    BindingEntry, LocalBindings, LocalBindingsFormat, MetricOverride, StorageScope,
};
pub use metrics::{
    CatalogSource, Diagnostic, ExportProfile, Metric, MetricCatalog, MetricOrigin, MetricSelector,
    MetricsFormat, SourceKind, ValueType,
};
pub use portable_io::{
    MAX_ARCHIVE_ENTRIES, MAX_COMPRESSION_RATIO, MAX_FILE_BYTES, MAX_TOTAL_BYTES, PortableBundle,
    PortableIoError, build_package_bytes, load_project_file, read_package, read_package_bytes,
    write_package,
};
pub use project::{
    Cardinality, CatalogReference, Dashboard, DashboardKind, GraphExpansion, GraphSeries,
    HostSelection, ItemExpansion, Layout, Page, PortableProject, ProjectFormat, Role, Selection,
    Widget,
};
pub use validation::{Validate, validate_project};
