// SPDX-License-Identifier: MIT OR Apache-2.0

use roxmltree::{Document, Error, Node, ParsingOptions};
use serde_json::{Map, Value, json};
use zdashstitch_core::formats::strict::MAX_DOCUMENT_DEPTH;

use super::{
    ImportError, MAX_EXPORT_NODES, METRIC_OMITTED, ROOT_OMITTED, RULE_OMITTED, TEMPLATE_OMITTED,
    shape,
};

pub(super) fn read(text: &str) -> Result<Value, ImportError> {
    let document = Document::parse_with_options(
        text,
        ParsingOptions {
            allow_dtd: false,
            nodes_limit: MAX_EXPORT_NODES as u32,
            entity_resolver: None,
        },
    )
    .map_err(|error| {
        let code = match error {
            Error::DtdDetected => "xml_dtd",
            Error::UnknownEntityReference(..) => "xml_entity",
            Error::NodesLimitReached => "template_export_objects",
            _ => "xml_invalid",
        };
        let position = error.pos();
        ImportError {
            code,
            line: Some(position.row.into()),
            column: Some(position.col.into()),
        }
    })?;
    for node in document.descendants() {
        if node.is_pi()
            || (node.is_element()
                && (node.tag_name().namespace().is_some()
                    || node.attributes().len() != 0
                    || node.namespaces().len() != 0))
        {
            return Err(ImportError::new("xml_unsupported_markup"));
        }
        if node.is_element()
            && node.ancestors().take(MAX_DOCUMENT_DEPTH + 2).count() > MAX_DOCUMENT_DEPTH + 1
        {
            return Err(ImportError::new("xml_depth"));
        }
    }
    let root = document.root_element();
    if !root.has_tag_name("zabbix_export") {
        return Err(shape());
    }
    let mut export = record(root, &["version"], ROOT_OMITTED)?;
    add_list(root, &mut export, "triggers", "trigger", trigger)?;
    add_list(root, &mut export, "templates", "template", template)?;
    Ok(json!({"zabbix_export": export}))
}

fn child<'a, 'input>(
    node: Node<'a, 'input>,
    name: &str,
) -> Result<Option<Node<'a, 'input>>, ImportError> {
    let mut matches = node.children().filter(|child| child.has_tag_name(name));
    let first = matches.next();
    if matches.next().is_some() {
        return Err(ImportError::new("xml_duplicate_element"));
    }
    Ok(first)
}

fn record(
    node: Node<'_, '_>,
    fields: &[&str],
    omitted: &[&str],
) -> Result<Map<String, Value>, ImportError> {
    if node
        .children()
        .any(|child| child.is_text() && child.text().is_some_and(|text| !text.trim().is_empty()))
    {
        return Err(shape());
    }
    let mut result = Map::new();
    for key in fields {
        if let Some(value) = child(node, key)? {
            if value.children().any(|child| child.is_element()) {
                return Err(shape());
            }
            let text = value
                .children()
                .filter_map(|child| child.is_text().then(|| child.text()).flatten())
                .collect();
            result.insert((*key).to_owned(), Value::String(text));
        }
    }
    for key in omitted {
        if let Some(value) = child(node, key)? {
            let present = value.children().any(|child| {
                child.is_element()
                    || (child.is_text() && child.text().is_some_and(|text| !text.is_empty()))
            });
            if present {
                result.insert((*key).to_owned(), Value::Bool(true));
            }
        }
    }
    Ok(result)
}

fn add_list(
    node: Node<'_, '_>,
    record: &mut Map<String, Value>,
    key: &str,
    element: &str,
    parse: impl Fn(Node<'_, '_>) -> Result<Value, ImportError>,
) -> Result<(), ImportError> {
    if let Some(container) = child(node, key)? {
        if container.children().any(|child| {
            child.is_text() && child.text().is_some_and(|text| !text.trim().is_empty())
        }) {
            return Err(shape());
        }
        let values = container
            .children()
            .filter(Node::is_element)
            .map(|entry| {
                if !entry.has_tag_name(element) {
                    return Err(shape());
                }
                parse(entry)
            })
            .collect::<Result<Vec<_>, _>>()?;
        record.insert(key.to_owned(), Value::Array(values));
    }
    Ok(())
}

fn template(node: Node<'_, '_>) -> Result<Value, ImportError> {
    let mut result = record(node, &["uuid", "template", "name"], TEMPLATE_OMITTED)?;
    add_list(node, &mut result, "templates", "template", |entry| {
        record(entry, &["name"], &[]).map(Value::Object)
    })?;
    add_list(node, &mut result, "items", "item", metric)?;
    add_list(node, &mut result, "triggers", "trigger", trigger)?;
    add_list(node, &mut result, "macros", "macro", |entry| {
        record(entry, &["macro", "type", "value"], &[]).map(Value::Object)
    })?;
    add_list(node, &mut result, "valuemaps", "valuemap", value_map)?;
    add_list(
        node,
        &mut result,
        "discovery_rules",
        "discovery_rule",
        discovery,
    )?;
    add_list(node, &mut result, "dashboards", "dashboard", |_| {
        Ok(Value::Null)
    })?;
    Ok(Value::Object(result))
}

fn discovery(node: Node<'_, '_>) -> Result<Value, ImportError> {
    let mut result = record(node, &["uuid", "key"], RULE_OMITTED)?;
    add_list(
        node,
        &mut result,
        "item_prototypes",
        "item_prototype",
        metric,
    )?;
    add_list(
        node,
        &mut result,
        "trigger_prototypes",
        "trigger_prototype",
        trigger,
    )?;
    Ok(Value::Object(result))
}

fn metric(node: Node<'_, '_>) -> Result<Value, ImportError> {
    let mut result = record(
        node,
        &["uuid", "name", "key", "units", "value_type", "delay"],
        METRIC_OMITTED,
    )?;
    if let Some(map) = child(node, "valuemap")? {
        result.insert(
            "valuemap".to_owned(),
            Value::Object(record(map, &["name"], &[])?),
        );
    }
    add_list(node, &mut result, "triggers", "trigger", trigger)?;
    add_list(
        node,
        &mut result,
        "trigger_prototypes",
        "trigger_prototype",
        trigger,
    )?;
    Ok(Value::Object(result))
}

fn trigger(node: Node<'_, '_>) -> Result<Value, ImportError> {
    record(node, &["expression"], &[]).map(Value::Object)
}

fn value_map(node: Node<'_, '_>) -> Result<Value, ImportError> {
    let mut result = record(node, &["name"], &[])?;
    add_list(node, &mut result, "mappings", "mapping", |entry| {
        record(entry, &["type", "value", "newvalue"], &[]).map(Value::Object)
    })?;
    Ok(Value::Object(result))
}
