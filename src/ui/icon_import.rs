//! Shell integration for staged image imports and inspector previews.
use super::icon_converter::IconImport;
use super::shell::{AppShell, FocusPane};
use crate::core::Severity;
use crate::services::image_conversion::{ConversionMode, is_esi_icon};
use crate::services::image_import::{
    IconTarget, editable_image_target, import_command, selected_image_target,
};
use std::sync::Arc;

pub(crate) struct IconDraft {
    target: IconTarget,
    text: Arc<str>,
}

impl AppShell {
    pub(crate) fn current_icon_target(&self) -> Option<IconTarget> {
        self.workspace.active().and_then(editable_image_target)
    }

    pub fn open_icon_converter(&mut self, ctx: &egui::Context) {
        self.icon_converter.open(self.current_icon_target(), ctx);
    }

    pub(crate) fn sync_icon_target(&mut self) {
        let target = self.current_icon_target();
        self.icon_converter.sync_target(target.as_ref());
        if self
            .workspace
            .active()
            .and_then(selected_image_target)
            .is_none()
        {
            self.image_preview.clear();
        }
        if self
            .icon_draft
            .as_ref()
            .is_some_and(|draft| Some(&draft.target) != target.as_ref())
        {
            self.icon_draft = None;
            self.image_preview.clear();
        }
    }

    pub(crate) fn show_icon_converter(&mut self, ctx: &egui::Context) {
        self.sync_icon_target();
        self.image_preview.poll(ctx);
        if let Some(import) =
            self.icon_converter
                .show(ctx, self.icon_draft.is_some(), &self.localization)
        {
            self.fill_icon_draft(import);
        }
    }

    pub(crate) fn fill_icon_draft(&mut self, import: IconImport) {
        if self.current_icon_target().as_ref() != Some(&import.target)
            || (is_esi_icon(&import.target.name) && import.mode != ConversionMode::EsiHex)
        {
            self.push_problem(
                Severity::Warning,
                "icon-import",
                self.localization.msg("icon-target-changed"),
            );
            return;
        }
        self.icon_draft = Some(IconDraft {
            target: import.target,
            text: import.text,
        });
        self.image_preview.clear();
        self.focus = FocusPane::Inspector;
        self.show_inspector_drawer = true;
    }

    pub(crate) fn apply_icon_draft(&mut self) -> bool {
        let Some(draft) = &self.icon_draft else {
            return false;
        };
        let command = self
            .workspace
            .active()
            .and_then(|session| import_command(session, &draft.target, &draft.text));
        let Some(command) = command else {
            self.sync_icon_target();
            return false;
        };
        if !self.commit(command) {
            return false;
        }
        self.reset_icon_draft();
        true
    }

    pub(crate) fn reset_icon_draft(&mut self) {
        self.icon_draft = None;
        self.image_preview.clear();
    }

    pub(crate) fn show_icon_inspector(&mut self, ui: &mut egui::Ui) {
        self.sync_icon_target();
        let target = self.workspace.active().and_then(selected_image_target);
        let Some(target) = target else {
            self.image_preview.clear();
            return;
        };
        ui.separator();
        let editable = self.current_icon_target().is_some();
        if ui
            .add_enabled(
                editable,
                egui::Button::new(self.localization.msg("icon-import")),
            )
            .clicked()
        {
            self.open_icon_converter(ui.ctx());
        }
        let session = self.workspace.active().expect("selected image session");
        let draft = self.icon_draft.as_ref();
        let text = draft
            .map(|d| d.text.as_ref())
            .or_else(|| {
                target
                    .content
                    .and_then(|node| session.document.node_text(node))
            })
            .unwrap_or_default();
        self.image_preview.show(
            ui,
            (
                target.session.0,
                target.element.0,
                target.revision.0.wrapping_mul(2) + u64::from(draft.is_some()),
            ),
            &target.name,
            text,
            draft.is_some(),
            &self.localization,
        );
        if let Some(draft) = draft {
            ui.label(self.localization.msg("icon-review"));
            ui.collapsing(self.localization.msg("icon-encoded"), |ui| {
                let mut excerpt = &draft.text[..draft.text.len().min(4096)];
                egui::ScrollArea::vertical()
                    .max_height(100.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut excerpt)
                                .desired_width(f32::INFINITY)
                                .desired_rows(3),
                        );
                    });
            });
            ui.horizontal_wrapped(|ui| {
                if ui.button(self.localization.msg("icon-apply")).clicked() {
                    self.apply_icon_draft();
                }
                if ui.button(self.localization.msg("icon-reset")).clicked() {
                    self.reset_icon_draft();
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{Command, NodeContent, XmlDocument, XmlNodeKind};
    use crate::services::image_conversion::{IconSource, convert_icon, esi_test_bmp};
    use crate::services::workspace::{DocumentMode, FileType};

    fn add_document(shell: &mut AppShell, xml: &str) {
        let doc = XmlDocument::parse(xml.as_bytes()).unwrap();
        let root = doc.root_element().unwrap();
        shell.workspace.add_opened(
            "icons.xml".into(),
            FileType::Xml,
            DocumentMode::Editable,
            doc,
        );
        shell.workspace.active_mut().unwrap().selection = Some(root);
    }

    fn import(shell: &AppShell, mode: ConversionMode) -> IconImport {
        let converted = convert_icon(
            &IconSource {
                name: "icon.bmp".into(),
                bytes: esi_test_bmp().into(),
            },
            mode,
            1024,
        )
        .unwrap();
        IconImport {
            target: shell.current_icon_target().unwrap(),
            mode,
            text: converted.text,
        }
    }

    #[test]
    fn import_is_staged_and_applied_as_one_independent_undo_step() {
        let mut shell = AppShell::new();
        add_document(
            &mut shell,
            "<ImageData16x14 attr='keep'>original</ImageData16x14>",
        );
        let content = shell.current_icon_target().unwrap().content.unwrap();
        assert!(shell.commit(Command::SetNodeContent {
            node: content,
            content: NodeContent::Text("manual".into())
        }));
        let before = shell
            .workspace
            .active()
            .unwrap()
            .document
            .source()
            .to_owned();
        let payload = import(&shell, ConversionMode::EsiHex);
        let expected = payload.text.to_string();
        shell.fill_icon_draft(payload);
        assert_eq!(shell.workspace.active().unwrap().document.source(), before);
        assert_eq!(shell.workspace.active().unwrap().history.undo_depth(), 1);
        assert!(shell.apply_icon_draft());
        assert_eq!(shell.workspace.active().unwrap().history.undo_depth(), 2);
        assert_eq!(
            shell
                .workspace
                .active()
                .unwrap()
                .document
                .node_text(content),
            Some(expected.as_str())
        );
        assert!(
            shell
                .workspace
                .active()
                .unwrap()
                .document
                .source()
                .contains("attr='keep'")
        );
        shell.undo();
        assert_eq!(
            shell
                .workspace
                .active()
                .unwrap()
                .document
                .node_text(content),
            Some("manual")
        );
        shell.redo();
        assert_eq!(
            shell
                .workspace
                .active()
                .unwrap()
                .document
                .node_text(content),
            Some(expected.as_str())
        );
    }

    #[test]
    fn import_preserves_cdata_namespace_and_surrounding_xml_bytes() {
        let mut shell = AppShell::new();
        let xml = "<?xml version='1.0'?><!--before--><e:ImageData16x14 xmlns:e='urn:test'><![CDATA[old]]></e:ImageData16x14><!--after-->";
        add_document(&mut shell, xml);
        let target = shell.current_icon_target().unwrap();
        shell.workspace.active_mut().unwrap().selection = target.content;
        let payload = import(&shell, ConversionMode::EsiHex);
        let expected = payload.text.to_string();
        shell.fill_icon_draft(payload);
        assert!(shell.apply_icon_draft());
        let doc = &shell.workspace.active().unwrap().document;
        assert_eq!(doc.kind(target.content.unwrap()), Some(XmlNodeKind::CData));
        assert_eq!(
            doc.source(),
            xml.replace("[CDATA[old]", &format!("[CDATA[{expected}]"))
        );
        shell.undo();
        assert_eq!(shell.workspace.active().unwrap().document.source(), xml);
    }

    #[test]
    fn empty_element_import_inserts_text_and_reset_leaves_it_empty() {
        let mut shell = AppShell::new();
        add_document(&mut shell, "<icon/>");
        shell.fill_icon_draft(import(&shell, ConversionMode::Base64));
        shell.reset_icon_draft();
        assert_eq!(
            shell.workspace.active().unwrap().document.source(),
            "<icon/>"
        );
        assert_eq!(shell.workspace.active().unwrap().history.undo_depth(), 0);
        shell.fill_icon_draft(import(&shell, ConversionMode::DataUri));
        assert!(shell.apply_icon_draft());
        assert!(
            shell
                .workspace
                .active()
                .unwrap()
                .document
                .source()
                .contains("data:image/bmp;base64,")
        );
        shell.undo();
        assert_eq!(
            shell.workspace.active().unwrap().document.source(),
            "<icon/>"
        );
    }

    #[test]
    fn stale_revision_or_tab_never_receives_an_import() {
        let mut shell = AppShell::new();
        add_document(&mut shell, "<icon>one</icon>");
        let payload = import(&shell, ConversionMode::Base64);
        let content = payload.target.content.unwrap();
        shell.commit(Command::SetNodeContent {
            node: content,
            content: NodeContent::Text("newer".into()),
        });
        shell.fill_icon_draft(payload);
        assert!(shell.icon_draft.is_none());
        let payload = import(&shell, ConversionMode::Base64);
        add_document(&mut shell, "<icon>two</icon>");
        shell.fill_icon_draft(payload);
        assert!(shell.icon_draft.is_none());
        shell.fill_icon_draft(import(&shell, ConversionMode::Base64));
        shell.workspace.select(0);
        shell.sync_icon_target();
        assert!(shell.icon_draft.is_none());
        assert_eq!(
            shell
                .workspace
                .active()
                .unwrap()
                .document
                .node_text(content),
            Some("newer")
        );
    }

    #[test]
    fn mixed_content_readonly_and_source_drafts_cannot_be_import_targets() {
        let mut shell = AppShell::new();
        for xml in [
            "<icon>text<child/></icon>",
            "<icon><!--keep-->text</icon>",
            "<icon>text<![CDATA[also]]></icon>",
        ] {
            add_document(&mut shell, xml);
            assert!(shell.current_icon_target().is_none());
        }
        add_document(&mut shell, "<icon/>");
        shell.workspace.active_mut().unwrap().mode = DocumentMode::LargeReadOnly;
        assert!(shell.current_icon_target().is_none());
        shell.workspace.active_mut().unwrap().mode = DocumentMode::Editable;
        let payload = import(&shell, ConversionMode::Base64);
        shell.update_source_text("<icon>unapplied source</icon>".into());
        assert!(shell.current_icon_target().is_none());
        shell.fill_icon_draft(payload);
        assert!(shell.icon_draft.is_none());
    }

    #[test]
    fn esi_target_rejects_base64_and_keeps_document_clean() {
        let mut shell = AppShell::new();
        add_document(&mut shell, "<ImageData16x14/>");
        shell.fill_icon_draft(import(&shell, ConversionMode::Base64));
        assert!(shell.icon_draft.is_none());
        assert_eq!(shell.workspace.active().unwrap().history.undo_depth(), 0);
    }
}
