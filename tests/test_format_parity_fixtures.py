"""Shared v1 fixture contract used by both the Python reference and Rust."""

from pathlib import Path
import json
import sys

import pytest
import yaml

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))

from validate import (  # noqa: E402
    assert_valid,
    catalog_normalized_source,
    schema,
    validate_catalog,
)

FIXTURES = ROOT / "fixtures" / "formats" / "v1"


def test_legacy_catalog_fixture_is_input_compatible_and_normalized():
    catalog = yaml.safe_load(
        (FIXTURES / "positive" / "legacy-template-catalog.yaml").read_text("utf8")
    )
    assert_valid(catalog, schema("metrics-v1.schema.json"))
    source = catalog_normalized_source(catalog)
    assert source["export_format_version"] == "6.0"
    assert "zabbix_version" not in source


def test_duplicate_metric_fixture_is_semantically_rejected():
    catalog = yaml.safe_load(
        (FIXTURES / "negative" / "duplicate-metric.yaml").read_text("utf8")
    )
    assert_valid(catalog, schema("metrics-v1.schema.json"))
    with pytest.raises(ValueError, match="Duplicate metric id"):
        validate_catalog(catalog)


@pytest.mark.parametrize(
    ("filename", "schema_name"),
    [
        ("numeric-hostid.json", "local-bindings-v1.schema.json"),
        ("unverified-deployment.json", "deployment-state-v1.schema.json"),
    ],
)
def test_invalid_json_fixture_is_schema_rejected(filename, schema_name):
    data = json.loads((FIXTURES / "negative" / filename).read_text("utf8"))
    with pytest.raises(ValueError, match="Schema"):
        assert_valid(data, schema(schema_name))
