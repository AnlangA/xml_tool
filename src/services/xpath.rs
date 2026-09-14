//! XPath 1.0 evaluation over the document DOM.
//!
//! The engine resolves prefixes with the namespaces in scope at each node,
//! so the root's visible declarations work without extra setup. Results
//! carry node identities (for Outline/Source/Inspector sync) plus rendered
//! labels, or a typed scalar.

use uppsala::dom::NodeId as EngineNodeId;
use uppsala::xpath::{XPathEvaluator, XPathValue};

use crate::core::document::{NodeId, XmlDocument};

/// A node-set entry: node id plus display label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XPathNode {
    pub node: NodeId,
    /// Rendered element name, or a preview of text/comment/PI content.
    pub label: String,
}

/// Typed XPath result.
#[derive(Debug, Clone, PartialEq)]
pub enum XPathOutcome {
    NodeSet(Vec<XPathNode>),
    String(String),
    Number(f64),
    Boolean(bool),
}

/// Evaluates `expr` with the document root as context. Prefixes resolve
/// through the namespaces visible at the root element, collected
/// automatically; extra bindings can be registered through
/// [`query_with_namespaces`].
pub fn query(document: &XmlDocument, expr: &str) -> Result<XPathOutcome, crate::xml::XmlError> {
    query_with_namespaces(document, expr, &[])
}

/// Evaluates `expr` with the root-visible namespaces plus explicit
/// `(prefix, uri)` overrides from the query panel.
pub fn query_with_namespaces(
    document: &XmlDocument,
    expr: &str,
    extra_bindings: &[(String, String)],
) -> Result<XPathOutcome, crate::xml::XmlError> {
    let dom = document.dom();
    let mut prepared = dom.clone();
    prepared.prepare_xpath();
    let mut evaluator = XPathEvaluator::new();
    if let Some(root) = document.root_element() {
        for (prefix, uri) in document.namespace_declarations(root) {
            evaluator.add_namespace(prefix, uri);
        }
    }
    for (prefix, uri) in extra_bindings {
        evaluator.add_namespace(prefix.clone(), uri.clone());
    }
    let root = prepared.root();
    let value = evaluator
        .evaluate(&prepared, root, expr)
        .map_err(crate::xml::XmlError::from_uppsala)?;
    Ok(match value {
        XPathValue::NodeSet(ids) => XPathOutcome::NodeSet(
            ids.iter()
                .map(|id| {
                    let engine = EngineNodeId::new(id.index());
                    XPathNode {
                        node: NodeId(id.index() as u64),
                        label: label_for(&prepared, engine),
                    }
                })
                .collect(),
        ),
        XPathValue::String(text) => XPathOutcome::String(text),
        XPathValue::Number(number) => XPathOutcome::Number(number),
        XPathValue::Boolean(flag) => XPathOutcome::Boolean(flag),
    })
}

fn label_for(dom: &uppsala::dom::Document<'_>, id: EngineNodeId) -> String {
    use uppsala::dom::NodeKind;
    match dom.node_kind(id) {
        Some(NodeKind::Element(element)) => match &element.name.prefix {
            Some(prefix) => format!("{prefix}:{}", element.name.local_name),
            None => element.name.local_name.to_string(),
        },
        Some(NodeKind::Text(text)) => {
            let shown: String = text.chars().take(40).collect();
            shown
        }
        Some(NodeKind::CData(text)) => {
            format!("CDATA {}", text.chars().take(32).collect::<String>())
        }
        Some(NodeKind::Comment(text)) => {
            format!("<!-- {} -->", text.chars().take(32).collect::<String>())
        }
        Some(NodeKind::ProcessingInstruction(pi)) => format!("<?{}?>", pi.target),
        Some(NodeKind::Attribute(name, value)) => {
            format!(
                "{}=\"{}\"",
                name.local_name,
                value.chars().take(32).collect::<String>()
            )
        }
        _ => String::from("(document)"),
    }
}
