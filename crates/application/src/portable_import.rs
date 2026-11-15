// SPDX-License-Identifier: MIT OR Apache-2.0

//! In-memory preparation of untrusted portable packages. No local storage or
//! catalogue registration takes place at this stage.

use serde::Serialize;
use zdashstitch_core::formats::{
    LegacyRootSelection, MAX_FILE_BYTES, PortableBundle, PortableIoError, PortableProject,
    TypedPortableBundle, TypedProject, from_yaml_str, read_package_bytes, read_typed_package_bytes,
};
use zdashstitch_zabbix::WidgetCapabilityRegistry;

#[derive(Serialize)]
pub struct ImportCompatibility {
    pub knowledge: &'static str,
    pub calculated: Option<zdashstitch_core::VersionRange>,
    pub declared_outside_registry: bool,
}

/// Registry-derived estimate only; it does not prove server capabilities.
pub fn compatibility(project: &TypedProject) -> ImportCompatibility {
    let report = crate::analyze_project_compatibility(project, &WidgetCapabilityRegistry::v7_4());
    let pages: Vec<_> = match &project.root {
        zdashstitch_core::formats::ProjectRoot::Page(page) => vec![page],
        zdashstitch_core::formats::ProjectRoot::Dashboard(dashboard) => {
            dashboard.pages.iter().collect()
        }
    };
    let has_unknown = pages
        .iter()
        .flat_map(|page| &page.widgets)
        .any(|widget| match widget {
            zdashstitch_core::formats::Widget::Zabbix {
                registry_state,
                fields,
                ..
            } => {
                *registry_state == zdashstitch_core::formats::RegistryState::Unknown
                    || fields.iter().any(|field| {
                        field.registry_state == zdashstitch_core::formats::RegistryState::Unknown
                    })
            }
            _ => false,
        });
    let knowledge = match report.knowledge {
        crate::CompatibilityKnowledge::Known => "known",
        crate::CompatibilityKnowledge::Unknown => "unknown",
        crate::CompatibilityKnowledge::Incompatible => "incompatible",
    };
    let knowledge = if has_unknown && knowledge == "known" {
        "unknown"
    } else {
        knowledge
    };
    let calculated = if knowledge == "known" {
        report.calculated
    } else {
        None
    };
    let declared_outside_registry = calculated.as_ref().is_some_and(|range| {
        project.declared_compatibility.minimum < range.minimum
            || range.maximum_exclusive.as_ref().is_some_and(|max| {
                project
                    .declared_compatibility
                    .maximum_exclusive
                    .as_ref()
                    .is_none_or(|declared| declared > max)
            })
    });
    ImportCompatibility {
        knowledge,
        calculated,
        declared_outside_registry,
    }
}

#[allow(clippy::large_enum_variant)]
pub enum InspectedPackage {
    Typed(TypedPortableBundle),
    Legacy(PortableBundle),
}

pub struct PackageErrors {
    pub typed: &'static str,
    pub legacy: &'static str,
}

/// Both readers enforce the same bounded ZIP, path and data validation policy.
pub fn inspect_package(raw: &[u8]) -> Result<InspectedPackage, PackageErrors> {
    if !raw.starts_with(b"PK") {
        return inspect_standalone(raw);
    }
    match read_typed_package_bytes(raw) {
        Ok(bundle) => Ok(InspectedPackage::Typed(bundle)),
        Err(typed) => match read_package_bytes(raw) {
            Ok(bundle) => Ok(InspectedPackage::Legacy(bundle)),
            Err(legacy) => Err(PackageErrors {
                typed: typed.code(),
                legacy: legacy.code(),
            }),
        },
    }
}

fn inspect_standalone(raw: &[u8]) -> Result<InspectedPackage, PackageErrors> {
    let invalid = || PackageErrors {
        typed: "standalone_invalid",
        legacy: "standalone_invalid",
    };
    if raw.len() > MAX_FILE_BYTES {
        return Err(invalid());
    }
    let text = std::str::from_utf8(raw).map_err(|_| invalid())?;
    if let Ok(project) = from_yaml_str::<TypedProject>(text) {
        let bundle = TypedPortableBundle {
            project,
            catalogs: Default::default(),
        };
        bundle.validate().map_err(|_| invalid())?;
        return Ok(InspectedPackage::Typed(bundle));
    }
    let project = from_yaml_str::<PortableProject>(text).map_err(|_| invalid())?;
    let bundle = PortableBundle {
        project,
        catalogs: Default::default(),
    };
    bundle.validate().map_err(|_| invalid())?;
    Ok(InspectedPackage::Legacy(bundle))
}

impl InspectedPackage {
    /// Legacy packages always require an explicit dashboard or page selection.
    pub fn select_legacy(
        &self,
        selection: &LegacyRootSelection,
    ) -> Option<Result<TypedPortableBundle, PortableIoError>> {
        match self {
            Self::Legacy(bundle) => Some(bundle.migrate(selection)),
            Self::Typed(_) => None,
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::path::Path;
    use zdashstitch_core::formats::{
        ProjectRoot, build_typed_package_bytes, load_typed_project_file,
    };

    #[test]
    fn inspects_v1_and_v2_without_saving_and_requires_explicit_legacy_root() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("workspace");
        let raw = std::fs::read(root.join("examples/db-pilot-package.zip")).expect("fixture");
        let legacy = inspect_package(&raw).ok().expect("valid v1");
        assert!(matches!(&legacy, InspectedPackage::Legacy(_)));
        let page = legacy
            .select_legacy(&LegacyRootSelection::Page {
                dashboard_id: "db-pilot".into(),
                page_id: "clickhouse".into(),
            })
            .expect("legacy requires selection")
            .expect("valid page");
        assert!(matches!(&page.project.root, ProjectRoot::Page(_)));
        assert!(
            legacy
                .select_legacy(&LegacyRootSelection::Dashboard {
                    dashboard_id: "missing".into()
                })
                .expect("legacy")
                .is_err()
        );
        let original =
            load_typed_project_file(&root.join("examples/page-v2.yaml")).expect("v2 fixture");
        let bytes = build_typed_package_bytes(&original).expect("v2 package");
        let InspectedPackage::Typed(reopened) = inspect_package(&bytes).ok().expect("valid v2")
        else {
            panic!("expected v2");
        };
        assert_eq!(reopened, original);
        assert!(inspect_package(b"not a zip").is_err());
    }

    #[test]
    fn standalone_yaml_is_bounded_strict_and_requires_no_catalogs() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("workspace");
        let raw = std::fs::read(root.join("examples/clock-page-v2.yaml")).expect("fixture");
        assert!(matches!(
            inspect_package(&raw).ok(),
            Some(InspectedPackage::Typed(_))
        ));
        let with_catalog = std::fs::read(root.join("examples/page-v2.yaml")).expect("fixture");
        assert!(inspect_package(&with_catalog).is_err());
        assert!(inspect_package(b"format: a\nformat: b\n").is_err());
        assert!(inspect_package(&vec![b'x'; MAX_FILE_BYTES + 1]).is_err());
    }

    #[test]
    fn unknown_widget_never_claims_registry_compatibility() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("workspace");
        let bundle =
            load_typed_project_file(&root.join("fixtures/formats/v2/positive/unknown-widget.yaml"))
                .expect("fixture");
        let report = compatibility(&bundle.project);
        assert_eq!(report.knowledge, "unknown");
        assert!(report.calculated.is_none());
    }

    #[test]
    fn registry_range_warning_distinguishes_narrowing_from_extension() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("workspace");
        let mut bundle =
            load_typed_project_file(&root.join("examples/clock-page-v2.yaml")).expect("fixture");
        let baseline = compatibility(&bundle.project);
        assert_eq!(baseline.knowledge, "known");
        let calculated = baseline.calculated.expect("range");
        bundle.project.declared_compatibility.minimum = calculated.minimum.clone();
        bundle.project.declared_compatibility.maximum_exclusive =
            Some(zdashstitch_core::Version::new(8, 0, 0));
        if calculated
            .maximum_exclusive
            .as_ref()
            .is_none_or(|maximum| *maximum >= zdashstitch_core::Version::new(8, 0, 0))
        {
            assert!(!compatibility(&bundle.project).declared_outside_registry);
        }
        bundle.project.declared_compatibility.minimum = zdashstitch_core::Version::new(1, 0, 0);
        assert!(compatibility(&bundle.project).declared_outside_registry);
    }

    #[test]
    fn imported_roots_reopen_with_embedded_catalogs() {
        use crate::{offline_dashboards::OfflineDashboardStore, offline_pages::OfflinePageStore};
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("workspace");
        let local = tempfile::tempdir().expect("local storage");
        let legacy = std::fs::read(root.join("examples/db-pilot-package.zip")).expect("fixture");
        let source = inspect_package(&legacy).ok().expect("legacy");
        let page = source
            .select_legacy(&LegacyRootSelection::Page {
                dashboard_id: "db-pilot".into(),
                page_id: "clickhouse".into(),
            })
            .expect("legacy")
            .expect("page");
        let expected = page.catalogs.clone();
        let store = OfflinePageStore::new(local.path().to_owned());
        let saved = store.save_imported(page).expect("save page");
        let reopened = store
            .get(&saved.local_id)
            .expect("read page")
            .expect("saved page");
        assert_eq!(reopened.bundle.catalogs, expected);
        assert_eq!(reopened.embedded_catalogs.len(), expected.len());
        assert!(reopened.catalog_versions.is_empty());
        assert!(!crate::offline_pages::catalog_sources_valid(
            &[],
            &[],
            &reopened.bundle
        ));
        let id = reopened.embedded_catalogs[0].clone();
        let overlapping = vec![crate::offline_pages::CatalogChoice {
            catalog_id: id,
            version: 1,
        }];
        assert!(!crate::offline_pages::catalog_sources_valid(
            &overlapping,
            &reopened.embedded_catalogs,
            &reopened.bundle
        ));
        let dashboard = source
            .select_legacy(&LegacyRootSelection::Dashboard {
                dashboard_id: "db-pilot".into(),
            })
            .expect("legacy")
            .expect("dashboard");
        let expected = dashboard.catalogs.clone();
        let store = OfflineDashboardStore::new(local.path().to_owned());
        let saved = store.save_imported(dashboard).expect("save dashboard");
        let reopened = store
            .get(&saved.local_id)
            .expect("read dashboard")
            .expect("saved dashboard");
        assert_eq!(reopened.bundle.catalogs, expected);
        assert_eq!(reopened.embedded_catalogs.len(), expected.len());
        assert!(reopened.catalog_versions.is_empty());
    }

    #[test]
    fn embedded_catalogs_survive_edit_library_copy_and_dashboard_assembly() {
        use crate::{
            offline_dashboards::{CreateDashboard, OfflineDashboardStore},
            offline_pages::{
                CreatePage, MetricChoice, OfflinePageStore, SeriesChoice, UpdatePage, WidgetChoice,
            },
            page_library::PageLibrary,
        };
        use zdashstitch_core::formats::Widget;
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("workspace");
        let local = tempfile::tempdir().expect("storage");
        let bytes =
            std::fs::read(workspace.join("examples/db-pilot-package.zip")).expect("fixture");
        let source = inspect_package(&bytes).ok().expect("legacy");
        let bundle = source
            .select_legacy(&LegacyRootSelection::Page {
                dashboard_id: "db-pilot".into(),
                page_id: "clickhouse".into(),
            })
            .expect("legacy")
            .expect("page");
        let store = OfflinePageStore::new(local.path().to_owned());
        let saved = store.save_imported(bundle).expect("save");
        let ProjectRoot::Page(page) = &saved.bundle.project.root else {
            panic!("page")
        };
        let widgets = page
            .widgets
            .iter()
            .map(|widget| match widget {
                Widget::Graph {
                    id,
                    layout,
                    title,
                    series,
                    ..
                } => WidgetChoice::Graph {
                    widget_id: Some(id.clone()),
                    layout: Some(layout.clone()),
                    title: title.clone(),
                    metrics: series
                        .iter()
                        .map(|item| SeriesChoice {
                            catalog_id: item.metric.catalog.clone(),
                            metric_id: item.metric.metric.clone(),
                            hosts: item.hosts.clone(),
                            color: item.color.clone(),
                        })
                        .collect(),
                },
                Widget::ItemValue {
                    id,
                    layout,
                    title,
                    metric,
                    hosts,
                    expansion,
                } => WidgetChoice::ItemValue {
                    widget_id: Some(id.clone()),
                    layout: Some(layout.clone()),
                    title: title.clone(),
                    metric: MetricChoice {
                        catalog_id: metric.catalog.clone(),
                        metric_id: metric.metric.clone(),
                    },
                    hosts: hosts.clone(),
                    expansion: *expansion,
                },
                Widget::Zabbix { .. } => panic!("unexpected fixture"),
            })
            .collect();
        let changed = store
            .update(
                &saved.local_id,
                UpdatePage {
                    expected_revision: saved.revision,
                    page: CreatePage {
                        name: "Renamed".into(),
                        catalogs: vec![],
                        roles: saved.bundle.project.roles.clone(),
                        widgets,
                    },
                },
            )
            .expect("edit");
        assert_eq!(changed.bundle.catalogs, saved.bundle.catalogs);
        assert_eq!(changed.embedded_catalogs, saved.embedded_catalogs);
        let library = PageLibrary::new(local.path().to_owned());
        let template = library.save_draft(&saved.local_id).expect("library copy");
        let copy = library.take_page(&template.library_id).expect("take copy");
        assert_eq!(copy.bundle.catalogs, saved.bundle.catalogs);
        assert_eq!(copy.embedded_catalogs, saved.embedded_catalogs);
        let dashboard = OfflineDashboardStore::new(local.path().to_owned())
            .create(CreateDashboard {
                name: "Assembled".into(),
                page_ids: vec![copy.local_id],
            })
            .expect("assemble");
        assert_eq!(dashboard.bundle.catalogs, saved.bundle.catalogs);
        assert_eq!(dashboard.embedded_catalogs, saved.embedded_catalogs);
        assert!(dashboard.catalog_versions.is_empty());
    }
}
