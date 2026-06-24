use std::path::PathBuf;

use sxd_document::dom;
use sxd_xpath::{Context, Factory, Value};

use crate::RmlError;
use crate::sources::SourceRow;

/// XML row — wraps a serialized XML element (the text of one selected node).
///
/// References are XPath 1.0 expressions evaluated relative to the root element
/// of the wrapped XML fragment. The fragment is re-parsed on each `get_str` call.
pub struct XmlRow(pub String);

impl SourceRow for XmlRow {
    fn get_str(&self, reference: &str) -> Option<String> {
        let package = sxd_document::parser::parse(&self.0).ok()?;
        let doc = package.as_document();

        // Context node = the root element of the serialized fragment
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

/// Serializes an element and its subtree to XML by deep-copying into a
/// temporary document and formatting it. The resulting string is a
/// self-contained XML document with the element as its root.
pub(crate) fn element_to_xml(element: dom::Element<'_>) -> Result<String, std::io::Error> {
    let pkg = sxd_document::Package::new();
    let doc = pkg.as_document();
    let new_elem = deep_copy_element(element, doc);
    doc.root().append_child(new_elem);
    let mut buf: Vec<u8> = Vec::new();
    sxd_document::writer::format_document(&doc, &mut buf)?;
    Ok(String::from_utf8(buf).unwrap_or_default())
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
}

impl XmlSource {
    pub fn new(path: PathBuf) -> Self {
        XmlSource {
            path,
            iterator: None,
        }
    }

    pub fn with_iterator(mut self, iterator: String) -> Self {
        self.iterator = Some(iterator);
        self
    }

    pub fn rows(&self) -> Box<dyn Iterator<Item = Result<XmlRow, RmlError>> + '_> {
        match self.collect_rows() {
            Ok(rows) => Box::new(rows.into_iter().map(Ok)),
            Err(e) => Box::new(std::iter::once(Err(e))),
        }
    }

    fn collect_rows(&self) -> Result<Vec<XmlRow>, RmlError> {
        let content = std::fs::read_to_string(&self.path)?;
        let package = sxd_document::parser::parse(&content).map_err(|e| RmlError::Xml {
            file: self.path.clone(),
            source: e,
        })?;
        let doc = package.as_document();

        let factory = Factory::new();
        let context = Context::new();

        let iter_expr = self.iterator.as_deref().unwrap_or("/*");
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
                    let xml_str = element_to_xml(elem).map_err(RmlError::Io)?;
                    rows.push(XmlRow(xml_str));
                }
            }
        }
        Ok(rows)
    }
}
