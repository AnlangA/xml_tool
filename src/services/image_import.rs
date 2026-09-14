//! Resolve image-bearing leaf elements without flattening mixed XML content.
use super::task_manager::SessionId;
use super::workspace::{DocumentMode, DocumentSession};
use crate::core::command::{ReplaceOp, ReplaceTarget};
use crate::core::{Command, InsertPosition, NewNode, NodeId, Revision, XmlNodeKind};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct IconTarget {
    pub session: SessionId,
    pub revision: Revision,
    pub selection: NodeId,
    pub element: NodeId,
    pub content: Option<NodeId>,
    pub name: String,
}

pub(crate) fn selected_image_target(session: &DocumentSession) -> Option<IconTarget> {
    let selection = session.selection?;
    let doc = &session.document;
    let element = match doc.kind(selection)? {
        XmlNodeKind::Element => selection,
        XmlNodeKind::Text | XmlNodeKind::CData => doc.parent(selection)?,
        _ => return None,
    };
    let children = doc.first_children(element, 2);
    let content = match children.as_slice() {
        [] => None,
        [node]
            if matches!(
                doc.kind(*node),
                Some(XmlNodeKind::Text | XmlNodeKind::CData)
            ) =>
        {
            Some(*node)
        }
        _ => return None,
    };
    Some(IconTarget {
        session: session.id,
        revision: doc.revision(),
        selection,
        element,
        content,
        name: doc.qname(element)?.render(),
    })
}

pub(crate) fn editable_image_target(session: &DocumentSession) -> Option<IconTarget> {
    if session.mode != DocumentMode::Editable || session.source_draft.is_some() {
        return None;
    }
    selected_image_target(session)
}

pub(crate) fn import_command(
    session: &DocumentSession,
    target: &IconTarget,
    text: &str,
) -> Option<Command> {
    if editable_image_target(session).as_ref() != Some(target) {
        return None;
    }
    Some(if let Some(node) = target.content {
        // BatchReplace is a single non-coalescing history entry and preserves
        // Text vs CDATA, unlike flattening/replacing the whole element.
        Command::BatchReplace {
            ops: vec![ReplaceOp {
                node,
                target: ReplaceTarget::NodeText,
                old_value: session.document.node_text(node)?.to_owned(),
                new_value: text.to_owned(),
            }],
        }
    } else {
        Command::InsertNode {
            parent: target.element,
            position: InsertPosition::Last,
            node: NewNode::Text {
                text: text.to_owned(),
            },
        }
    })
}
