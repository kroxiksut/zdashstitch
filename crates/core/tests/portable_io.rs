// SPDX-License-Identifier: MIT OR Apache-2.0

use std::{
    fs,
    io::{Cursor, Write},
    path::{Path, PathBuf},
};

use serde::Deserialize;
use tempfile::tempdir;
use zdashstitch_core::formats::{
    MAX_ARCHIVE_ENTRIES, MAX_FILE_BYTES, build_package_bytes, build_typed_package_bytes,
    load_project_file, load_typed_project_file, read_package, read_package_bytes,
    read_typed_package, read_typed_package_bytes, write_package, write_typed_package,
};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap_or_else(|| panic!("core crate must be nested under the workspace root"))
        .to_path_buf()
}

fn project_path() -> PathBuf {
    root().join("examples/db-pilot.yaml")
}

fn make_zip(entries: &[(&str, &[u8])], compression: CompressionMethod) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(compression);
    for (name, content) in entries {
        writer
            .start_file(*name, options)
            .unwrap_or_else(|error| panic!("cannot start ZIP entry {name}: {error}"));
        writer
            .write_all(content)
            .unwrap_or_else(|error| panic!("cannot write ZIP entry {name}: {error}"));
    }
    writer
        .finish()
        .unwrap_or_else(|error| panic!("cannot finish test ZIP: {error}"))
        .into_inner()
}

fn make_symlink_zip() -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .add_symlink(
            "project.yaml",
            "../outside",
            SimpleFileOptions::default().unix_permissions(0o120777),
        )
        .unwrap_or_else(|error| panic!("cannot add ZIP symlink: {error}"));
    writer
        .finish()
        .unwrap_or_else(|error| panic!("cannot finish symlink ZIP: {error}"))
        .into_inner()
}

#[test]
fn directory_and_data_only_zip_round_trip_without_network_or_extraction() {
    let bundle = load_project_file(&project_path())
        .unwrap_or_else(|error| panic!("DB Pilot directory must load: {error}"));
    let raw = build_package_bytes(&bundle)
        .unwrap_or_else(|error| panic!("DB Pilot package must build: {error}"));
    assert!(raw.starts_with(b"PK"));
    let decoded = read_package_bytes(&raw)
        .unwrap_or_else(|error| panic!("built package must reopen: {error}"));
    assert_eq!(bundle, decoded);

    let existing = read_package(&root().join("examples/db-pilot-package.zip"))
        .unwrap_or_else(|error| panic!("reference package must load: {error}"));
    assert_eq!(bundle, existing);

    let output = tempdir().unwrap_or_else(|error| panic!("cannot create test directory: {error}"));
    let package = output.path().join("db-pilot.zip");
    write_package(&bundle, &package)
        .unwrap_or_else(|error| panic!("cannot publish package: {error}"));
    assert_eq!(
        bundle,
        read_package(&package).unwrap_or_else(|error| panic!("cannot reopen package: {error}"))
    );
    let second = write_package(&bundle, &package)
        .err()
        .unwrap_or_else(|| panic!("existing output must not be overwritten"));
    assert_eq!(second.code(), "destination_exists");
    let leftovers = fs::read_dir(output.path())
        .unwrap_or_else(|error| panic!("cannot inspect test directory: {error}"))
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".zdashstitch-")
        })
        .count();
    assert_eq!(leftovers, 0);
}

#[test]
fn typed_dashboard_and_page_packages_keep_root_kind() {
    for relative in [
        "examples/dashboard-v2.yaml",
        "examples/page-v2.yaml",
        "examples/clock-page-v2.yaml",
        "examples/global-view-v2.yaml",
        "fixtures/formats/v2/positive/unknown-widget.yaml",
    ] {
        let bundle = load_typed_project_file(&root().join(relative))
            .unwrap_or_else(|error| panic!("typed project must load: {error}"));
        let kind = bundle.project.kind;
        let project_root = bundle.project.root.clone();
        let raw = build_typed_package_bytes(&bundle)
            .unwrap_or_else(|error| panic!("typed package must build: {error}"));
        let reopened = read_typed_package_bytes(&raw)
            .unwrap_or_else(|error| panic!("typed package must reopen: {error}"));
        assert_eq!(reopened.project.kind, kind);
        assert_eq!(reopened.project.root, project_root);
        assert_eq!(reopened, bundle);
    }
}

#[test]
fn typed_package_is_atomically_written_without_overwrite() {
    let bundle = load_typed_project_file(&root().join("examples/page-v2.yaml"))
        .unwrap_or_else(|error| panic!("typed project must load: {error}"));
    let output = tempdir().unwrap_or_else(|error| panic!("cannot create test directory: {error}"));
    let destination = output.path().join("page.zip");
    write_typed_package(&bundle, &destination)
        .unwrap_or_else(|error| panic!("typed package must write: {error}"));
    assert_eq!(
        read_typed_package(&destination)
            .unwrap_or_else(|error| panic!("typed package must reopen: {error}")),
        bundle
    );
    assert_eq!(
        write_typed_package(&bundle, &destination)
            .err()
            .unwrap_or_else(|| panic!("typed package overwrite must fail"))
            .code(),
        "destination_exists"
    );
}

#[derive(Deserialize)]
struct NegativeCase {
    name: String,
    #[serde(rename = "type")]
    case_type: String,
    file: Option<String>,
    source: Option<String>,
    reader: Option<String>,
    code: String,
}

#[test]
fn shared_v2_negative_cases_have_stable_codes() {
    let cases: Vec<NegativeCase> =
        serde_json::from_str(&read_text("fixtures/formats/v2/negative-cases.json"))
            .unwrap_or_else(|error| panic!("negative case manifest must parse: {error}"));
    for case in cases {
        let error = match case.case_type.as_str() {
            "document" => load_typed_project_file(
                &root().join("fixtures/formats/v2").join(
                    case.file
                        .as_deref()
                        .unwrap_or_else(|| panic!("{} has no file", case.name)),
                ),
            )
            .err()
            .unwrap_or_else(|| panic!("{} must fail", case.name)),
            "package" => {
                let source = case
                    .source
                    .as_deref()
                    .unwrap_or_else(|| panic!("{} has no source", case.name));
                let raw = if source.ends_with("db-pilot.yaml") {
                    let bundle = load_project_file(&root().join(source))
                        .unwrap_or_else(|error| panic!("cannot load {source}: {error}"));
                    build_package_bytes(&bundle)
                        .unwrap_or_else(|error| panic!("cannot package {source}: {error}"))
                } else {
                    let bundle = load_typed_project_file(&root().join(source))
                        .unwrap_or_else(|error| panic!("cannot load {source}: {error}"));
                    build_typed_package_bytes(&bundle)
                        .unwrap_or_else(|error| panic!("cannot package {source}: {error}"))
                };
                match case.reader.as_deref() {
                    Some("v1") => read_package_bytes(&raw)
                        .err()
                        .unwrap_or_else(|| panic!("{} must fail", case.name)),
                    Some("v2") => read_typed_package_bytes(&raw)
                        .err()
                        .unwrap_or_else(|| panic!("{} must fail", case.name)),
                    _ => panic!("{} has an unknown reader", case.name),
                }
            }
            _ => panic!("{} has an unknown case type", case.name),
        };
        assert_eq!(error.code(), case.code, "case {}", case.name);
    }
}

#[test]
fn archive_path_type_count_size_and_ratio_policies_fail_closed() {
    let traversal = make_zip(
        &[("../project.yaml", b"invalid")],
        CompressionMethod::Stored,
    );
    assert_eq!(
        read_package_bytes(&traversal)
            .err()
            .unwrap_or_else(|| panic!("traversal archive must fail"))
            .code(),
        "archive_name"
    );

    let unexpected = make_zip(&[("token.txt", b"secret")], CompressionMethod::Stored);
    assert_eq!(
        read_package_bytes(&unexpected)
            .err()
            .unwrap_or_else(|| panic!("unexpected entry must fail"))
            .code(),
        "archive_name"
    );

    assert_eq!(
        read_package_bytes(&make_symlink_zip())
            .err()
            .unwrap_or_else(|| panic!("symlink entry must fail"))
            .code(),
        "archive_file_type"
    );

    let names: Vec<String> = (0..=MAX_ARCHIVE_ENTRIES)
        .map(|index| format!("metrics/a{index}.yaml"))
        .collect();
    let entries: Vec<(&str, &[u8])> = names
        .iter()
        .map(|name| (name.as_str(), b"x".as_slice()))
        .collect();
    let too_many = make_zip(&entries, CompressionMethod::Stored);
    assert_eq!(
        read_package_bytes(&too_many)
            .err()
            .unwrap_or_else(|| panic!("entry-count overflow must fail"))
            .code(),
        "archive_entries"
    );

    let oversized_content = vec![b'x'; MAX_FILE_BYTES + 1];
    let oversized = make_zip(
        &[("project.yaml", oversized_content.as_slice())],
        CompressionMethod::Stored,
    );
    assert_eq!(
        read_package_bytes(&oversized)
            .err()
            .unwrap_or_else(|| panic!("oversized entry must fail"))
            .code(),
        "entry_size"
    );

    let repetitive_content = vec![b'x'; 1024 * 1024];
    let ratio = make_zip(
        &[("project.yaml", repetitive_content.as_slice())],
        CompressionMethod::Deflated,
    );
    assert_eq!(
        read_package_bytes(&ratio)
            .err()
            .unwrap_or_else(|| panic!("extreme compression ratio must fail"))
            .code(),
        "compression_ratio"
    );
}

#[test]
fn yaml_aliases_and_missing_catalogs_are_rejected_before_use() {
    let alias = make_zip(
        &[(
            "project.yaml",
            b"format: &format zdashstitch.project/v1\nproject_id: *format\n",
        )],
        CompressionMethod::Stored,
    );
    assert_eq!(
        read_package_bytes(&alias)
            .err()
            .unwrap_or_else(|| panic!("YAML aliases must fail"))
            .code(),
        "yaml_alias"
    );

    let project = fs::read(project_path())
        .unwrap_or_else(|error| panic!("cannot read project fixture: {error}"));
    let missing_catalog = make_zip(&[("project.yaml", &project)], CompressionMethod::Stored);
    assert_eq!(
        read_package_bytes(&missing_catalog)
            .err()
            .unwrap_or_else(|| panic!("missing catalogs must fail"))
            .code(),
        "archive_contents"
    );
}

#[test]
fn filesystem_links_are_not_followed() {
    let directory =
        tempdir().unwrap_or_else(|error| panic!("cannot create test directory: {error}"));
    let linked = directory.path().join("project.yaml");
    if !create_file_symlink(&project_path(), &linked) {
        return;
    }
    assert!(load_project_file(&linked).is_err());
}

#[cfg(unix)]
fn create_file_symlink(target: &Path, link: &Path) -> bool {
    std::os::unix::fs::symlink(target, link).is_ok()
}

#[cfg(windows)]
fn create_file_symlink(target: &Path, link: &Path) -> bool {
    std::os::windows::fs::symlink_file(target, link).is_ok()
}

#[cfg(not(any(unix, windows)))]
fn create_file_symlink(_target: &Path, _link: &Path) -> bool {
    false
}

fn read_text(relative: &str) -> String {
    fs::read_to_string(root().join(relative))
        .unwrap_or_else(|error| panic!("cannot read {relative}: {error}"))
}

/// P0-11 (2): every public file and package reader rejects duplicate keys and
/// anchors/aliases with the reference codes.
#[test]
fn file_and_package_readers_reject_duplicate_keys_and_aliases() {
    let v1 = read_text("examples/db-pilot.yaml");
    let v2 = read_text("examples/page-v2.yaml");
    let cases = [
        (
            "v1",
            "yaml_duplicate_key",
            format!("{v1}project_id: other\n"),
        ),
        (
            "v1",
            "yaml_alias",
            v1.replacen("project_id: ", "project_id: &id ", 1),
        ),
        (
            "v2",
            "yaml_duplicate_key",
            format!("{v2}project_id: other\n"),
        ),
        (
            "v2",
            "yaml_alias",
            v2.replacen("project_id: ", "project_id: &id ", 1),
        ),
    ];
    let temporary = tempdir().unwrap_or_else(|error| panic!("temp directory: {error}"));
    for (reader, code, text) in cases {
        let path = temporary.path().join("project.yaml");
        fs::write(&path, &text).unwrap_or_else(|error| panic!("write project: {error}"));
        let raw = make_zip(
            &[("project.yaml", text.as_bytes())],
            CompressionMethod::Stored,
        );
        let (file, package) = if reader == "v1" {
            (
                load_project_file(&path).err(),
                read_package_bytes(&raw).err(),
            )
        } else {
            (
                load_typed_project_file(&path).err(),
                read_typed_package_bytes(&raw).err(),
            )
        };
        let file = file.unwrap_or_else(|| panic!("{reader} file must fail with {code}"));
        assert_eq!(file.code(), code, "{reader} file: {file}");
        let package = package.unwrap_or_else(|| panic!("{reader} package must fail with {code}"));
        assert_eq!(package.code(), code, "{reader} package: {package}");
    }
}
