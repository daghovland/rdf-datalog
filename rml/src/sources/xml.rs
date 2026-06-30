use std::fmt;
use std::path::PathBuf;

use sxd_document::dom;
use sxd_xpath::{Context, Factory, Value};

use crate::RmlError;
use crate::sources::SourceRow;

/// Returns `true` if `xpath` contains nested predicate brackets, i.e. a `[`
/// that appears while already inside a `[…]` block.
///
/// Such expressions (e.g. `//a[//a[//a]]`) can cause exponential evaluation
/// complexity and are rejected as unsafe.
/// See [#88](https://github.com/daghovland/rdf-datalog/issues/88).
fn has_nested_predicates(xpath: &str) -> bool {
    let mut depth: u32 = 0;
    for ch in xpath.chars() {
        match ch {
            '[' => {
                if depth > 0 {
                    return true;
                }
                depth += 1;
            }
            ']' => {
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
    }
    false
}

/// XML row — holds a pre-parsed `Package` containing one selected element.
///
/// References are XPath 1.0 expressions evaluated relative to the root element.
/// The XML is parsed once at construction; subsequent `get_str` calls only
/// evaluate XPath against the already-parsed tree. See [#89](https://github.com/daghovland/rdf-datalog/issues/89).
pub struct XmlRow {
    package: sxd_document::Package,
}

impl XmlRow {
    /// Parse `xml` once and return `Some(XmlRow)`, or `None` if the XML is malformed.
    pub fn from_xml(xml: &str) -> Option<Self> {
        let package = sxd_document::parser::parse(xml).ok()?;
        Some(XmlRow { package })
    }
}

impl fmt::Debug for XmlRow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("XmlRow").finish_non_exhaustive()
    }
}

impl SourceRow for XmlRow {
    fn get_str(&self, reference: &str) -> Option<String> {
        let doc = self.package.as_document();

        let root_element = doc.root().children().into_iter().find_map(|c| match c {
            dom::ChildOfRoot::Element(e) => Some(e),
            _ => None,
        })?;

        let factory = Factory::new();
        let xpath = factory.build(reference).ok()??;
        let context = Context::new();

        let value = xpath.evaluate(&context, root_element).ok()?;
        let s = value.string();
        if s.is_empty() { None } else { Some(s) }
    }
}

/// Deep-copies `element` into a fresh `Package` and returns it.
///
/// Used by `collect_rows` to give each row its own owned parsed tree,
/// avoiding a serialize-then-reparse round-trip.
fn element_to_package(element: dom::Element<'_>) -> sxd_document::Package {
    let pkg = sxd_document::Package::new();
    let doc = pkg.as_document();
    let new_elem = deep_copy_element(element, doc);
    doc.root().append_child(new_elem);
    pkg
}

fn deep_copy_element<'s, 'd>(src: dom::Element<'s>, doc: dom::Document<'d>) -> dom::Element<'d> {
    let new_elem = doc.create_element(src.name().local_part());
    for attr in src.attributes() {
        new_elem.set_attribute_value(attr.name().local_part(), attr.value());
    }
    for child in src.children() {
        match child {
            dom::ChildOfElement::Text(t) => {
                new_elem.append_child(doc.create_text(t.text()));
            }
            dom::ChildOfElement::Element(e) => {
                new_elem.append_child(deep_copy_element(e, doc));
            }
            _ => {}
        }
    }
    new_elem
}

/// XML file source.
///
/// `iterator` is an XPath expression (e.g. `/students/student`) that selects
/// the repeating nodes. When absent, defaults to `/*` (the document root
/// element as a single row).
pub struct XmlSource {
    pub path: PathBuf,
    pub iterator: Option<String>,
    /// Override for the default MAX_SOURCE_BYTES limit (used in tests). See [#86](https://github.com/daghovland/rdf-datalog/issues/86).
    pub size_limit: Option<u64>,
}

impl XmlSource {
    pub fn new(path: PathBuf) -> Self {
        XmlSource {
            path,
            iterator: None,
            size_limit: None,
        }
    }

    pub fn with_iterator(mut self, iterator: String) -> Self {
        self.iterator = Some(iterator);
        self
    }

    /// Set a custom byte size limit (overrides [`crate::MAX_SOURCE_BYTES`]).
    pub fn with_size_limit(mut self, bytes: u64) -> Self {
        self.size_limit = Some(bytes);
        self
    }

    pub fn rows(&self) -> Box<dyn Iterator<Item = Result<XmlRow, RmlError>> + '_> {
        match self.collect_rows() {
            Ok(rows) => Box::new(rows.into_iter().map(Ok)),
            Err(e) => Box::new(std::iter::once(Err(e))),
        }
    }

    fn collect_rows(&self) -> Result<Vec<XmlRow>, RmlError> {
        // Enforce file-size limit before reading any content.
        let size_limit = self.size_limit.unwrap_or(crate::MAX_SOURCE_BYTES);
        let file_size = std::fs::metadata(&self.path)?.len();
        if file_size > size_limit {
            return Err(RmlError::SourceTooLarge {
                limit: size_limit,
                actual: file_size,
            });
        }

        let content = std::fs::read_to_string(&self.path)?;
        let package = sxd_document::parser::parse(&content).map_err(|e| {
            log::error!("XML parse error in {}: {e}", self.path.display());
            RmlError::Xml {
                file: self.path.clone(),
                source: e,
            }
        })?;
        let doc = package.as_document();

        let factory = Factory::new();
        let context = Context::new();

        let iter_expr = self.iterator.as_deref().unwrap_or("/*");

        // Reject XPath expressions with nested predicates — they can cause
        // exponential node-set evaluation. See [#88](https://github.com/daghovland/rdf-datalog/issues/88).
        if has_nested_predicates(iter_expr) {
            return Err(RmlError::UnsafeExpression(format!(
                "XPath iterator '{iter_expr}' contains nested predicates, which are not allowed"
            )));
        }

        let xpath = factory
            .build(iter_expr)
            .map_err(|e| {
                RmlError::MappingParse(format!("invalid XPath iterator '{iter_expr}': {e}"))
            })?
            .ok_or_else(|| RmlError::MappingParse(format!("empty XPath iterator '{iter_expr}'")))?;

        let value = xpath
            .evaluate(&context, doc.root())
            .map_err(|e| RmlError::MappingParse(format!("XPath evaluation failed: {e}")))?;

        let mut rows = Vec::new();
        if let Value::Nodeset(ns) = value {
            for node in ns.document_order() {
                if let Some(elem) = node.element() {
                    rows.push(XmlRow {
                        package: element_to_package(elem),
                    });
                }
            }
        }
        Ok(rows)
    }
}
