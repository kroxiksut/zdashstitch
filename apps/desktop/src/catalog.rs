// SPDX-License-Identifier: MIT OR Apache-2.0

//! Native text of the launcher (window title, fallback and close dialogs)
//! from the shared message catalogs `lang/<locale>/`, embedded at build
//! time. Message IDs are the generated, both-catalog-checked
//! [`MessageId`] constants; English is the fallback.

use std::collections::BTreeMap;

use serde::Deserialize;
use zdashstitch_application::MessageId;

/// The supported MVP locales.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Locale {
    En,
    Ru,
}

/// Namespaces the launcher reads, per locale.
const EN: [&str; 2] = [
    include_str!("../../../lang/en/desktop.json"),
    include_str!("../../../lang/en/common.json"),
];
const RU: [&str; 2] = [
    include_str!("../../../lang/ru/desktop.json"),
    include_str!("../../../lang/ru/common.json"),
];

#[derive(Deserialize)]
struct Entry {
    message: String,
}

/// Messages of one locale with the English source as fallback.
#[derive(Debug)]
pub struct Catalog {
    messages: BTreeMap<String, String>,
    source: BTreeMap<String, String>,
}

impl Catalog {
    #[must_use]
    pub fn new(locale: Locale) -> Self {
        let source = parse(&EN);
        let messages = match locale {
            Locale::En => source.clone(),
            Locale::Ru => parse(&RU),
        };
        Self { messages, source }
    }

    /// The message with its `{name}` placeholders replaced by `params`. An
    /// unknown placeholder stays as written; an unknown ID (impossible for a
    /// generated constant) yields the ID itself.
    #[must_use]
    pub fn text(&self, id: MessageId, params: &[(&str, &str)]) -> String {
        let template = self
            .messages
            .get(id.as_str())
            .or_else(|| self.source.get(id.as_str()))
            .map_or(id.as_str(), String::as_str);
        substitute(template, params)
    }
}

fn parse(documents: &[&str]) -> BTreeMap<String, String> {
    let mut messages = BTreeMap::new();
    for document in documents {
        if let Ok(entries) = serde_json::from_str::<BTreeMap<String, Entry>>(document) {
            messages.extend(entries.into_iter().map(|(id, entry)| (id, entry.message)));
        }
    }
    messages
}

/// One pass over `template`; replacement values are never re-scanned.
fn substitute(template: &str, params: &[(&str, &str)]) -> String {
    let mut output = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        output.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let value = after.find('}').and_then(|end| {
            let name = &after[..end];
            params
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value, end))
        });
        match value {
            Some((value, end)) => {
                output.push_str(value);
                rest = &after[end + 1..];
            }
            None => {
                output.push('{');
                rest = after;
            }
        }
    }
    output.push_str(rest);
    output
}

#[cfg(test)]
mod tests {
    use zdashstitch_application::MessageId;

    use super::{Catalog, EN, Locale, RU, parse, substitute};

    #[test]
    fn embedded_catalogs_parse_with_the_same_ids() {
        let en = parse(&EN);
        let ru = parse(&RU);
        assert!(en.contains_key(MessageId::DESKTOP_FALLBACK_TITLE.as_str()));
        assert!(en.contains_key(MessageId::COMMON_PRODUCT_NAME.as_str()));
        assert_eq!(en.keys().collect::<Vec<_>>(), ru.keys().collect::<Vec<_>>());
    }

    #[test]
    fn every_launcher_message_resolves_in_both_locales() {
        let ids = MessageId::ALL
            .iter()
            .filter(|id| id.as_str().starts_with("desktop."));
        for id in ids {
            for locale in [Locale::En, Locale::Ru] {
                let text = Catalog::new(locale).text(
                    *id,
                    &[("url", "https://example.invalid/"), ("command", "true")],
                );
                assert!(!text.is_empty() && !text.contains('{'), "{id} {locale:?}");
                assert_ne!(text, id.as_str(), "{id} {locale:?}");
            }
        }
    }

    #[test]
    fn russian_differs_from_english() {
        let en = Catalog::new(Locale::En).text(MessageId::DESKTOP_FALLBACK_CHOICE_BROWSER, &[]);
        let ru = Catalog::new(Locale::Ru).text(MessageId::DESKTOP_FALLBACK_CHOICE_BROWSER, &[]);
        assert_eq!(en, "Continue in the browser");
        assert_eq!(ru, "Продолжить в браузере");
    }

    #[test]
    fn placeholders_are_replaced_once() {
        assert_eq!(
            substitute("a {x} b", &[("x", "{y}"), ("y", "no")]),
            "a {y} b"
        );
        assert_eq!(substitute("{missing} {", &[]), "{missing} {");
        assert_eq!(substitute("{x}{x}", &[("x", "1")]), "11");
    }
}
