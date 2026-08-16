//! XML syntax highlighting for the source view.
//!
//! The tokenizer produces `(color, text)` segments colored from the
//! per-frame [`Palette`], so highlighting follows the active light/dark
//! theme. `highlight_layout_job` packages the segments into a single
//! [`LayoutJob`] suitable for `TextEdit::layouter`.

use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontId};

use super::theme::Palette;

/// Syntax highlighter for XML code.
pub struct SyntaxHighlighter;

impl Default for SyntaxHighlighter {
    fn default() -> Self {
        Self::new()
    }
}

impl SyntaxHighlighter {
    pub fn new() -> Self {
        Self
    }

    /// Highlight XML text and return styled segments using the app's single tokenizer path.
    pub fn highlight_xml(&self, palette: &Palette, text: &str) -> Vec<(Color32, String)> {
        tokenize_xml(palette, text)
    }

    /// Highlight XML and split it into per-line layout jobs for code-style viewing.
    pub fn highlight_xml_lines(&self, palette: &Palette, text: &str) -> Vec<LayoutJob> {
        let font_id = FontId::monospace(12.0);
        let mut lines = vec![LayoutJob::default()];

        for (color, segment) in self.highlight_xml(palette, text) {
            let format = TextFormat {
                font_id: font_id.clone(),
                color,
                ..Default::default()
            };

            let mut parts = segment.split('\n').peekable();
            while let Some(part) = parts.next() {
                if !part.is_empty() {
                    lines.last_mut().expect("at least one layout line").append(
                        part,
                        0.0,
                        format.clone(),
                    );
                }

                if parts.peek().is_some() {
                    lines.push(LayoutJob::default());
                }
            }
        }

        lines
    }

    /// Highlight XML into a single [`LayoutJob`] for `TextEdit::layouter`.
    pub fn highlight_layout_job(
        &self,
        palette: &Palette,
        text: &str,
        font_id: FontId,
        wrap_width: f32,
    ) -> LayoutJob {
        let mut job = LayoutJob::default();
        job.wrap.max_width = wrap_width;

        for (color, segment) in self.highlight_xml(palette, text) {
            let format = TextFormat {
                font_id: font_id.clone(),
                color,
                ..Default::default()
            };
            job.append(&segment, 0.0, format);
        }

        job
    }
}

fn tokenize_xml(palette: &Palette, text: &str) -> Vec<(Color32, String)> {
    let mut result = Vec::new();
    let mut index = 0;

    while index < text.len() {
        let rest = &text[index..];

        if rest.starts_with("<!--") {
            let end = rest
                .find("-->")
                .map(|offset| offset + 3)
                .unwrap_or(rest.len());
            push_colored(&mut result, palette.syntax_comment, &rest[..end]);
            index += end;
            continue;
        }

        if rest.starts_with("<![CDATA[") {
            let end = rest
                .find("]]>")
                .map(|offset| offset + 3)
                .unwrap_or(rest.len());
            let cdata = &rest[..end];
            push_colored(&mut result, palette.syntax_keyword, "<![CDATA[");

            // Only a real closing delimiter trims 3 bytes; an unterminated
            // section keeps every character visible.
            let content_end = if cdata.ends_with("]]>") {
                cdata.len() - 3
            } else {
                cdata.len()
            };
            if content_end > 9 {
                push_entity_aware(
                    &mut result,
                    &cdata[9..content_end],
                    palette.syntax_text,
                    palette.syntax_keyword,
                );
            }

            if cdata.ends_with("]]>") {
                push_colored(&mut result, palette.syntax_keyword, "]]>");
            }
            index += end;
            continue;
        }

        if rest.starts_with("<?") {
            let end = find_processing_instruction_end(rest);
            highlight_processing_instruction(palette, &mut result, &rest[..end]);
            index += end;
            continue;
        }

        if rest.starts_with("<!") {
            let end = find_tag_end(rest);
            push_colored(&mut result, palette.syntax_keyword, &rest[..end]);
            index += end;
            continue;
        }

        if rest.starts_with('<') {
            let end = find_tag_end(rest);
            highlight_tag_like_markup(palette, &mut result, &rest[..end]);
            index += end;
            continue;
        }

        let next_markup = rest.find('<').unwrap_or(rest.len());
        push_entity_aware(
            &mut result,
            &rest[..next_markup],
            palette.syntax_text,
            palette.syntax_keyword,
        );
        index += next_markup;
    }

    result
}

fn highlight_tag_like_markup(palette: &Palette, result: &mut Vec<(Color32, String)>, markup: &str) {
    if let Some(markup) = markup.strip_prefix("</") {
        push_colored(result, palette.syntax_tag_bracket, "</");
        highlight_markup_body(palette, result, markup, palette.syntax_tag, ">");
    } else {
        push_colored(result, palette.syntax_tag_bracket, "<");
        highlight_markup_body(palette, result, &markup[1..], palette.syntax_tag, ">");
    }
}

fn highlight_processing_instruction(
    palette: &Palette,
    result: &mut Vec<(Color32, String)>,
    markup: &str,
) {
    push_colored(result, palette.syntax_tag_bracket, "<?");
    highlight_markup_body(palette, result, &markup[2..], palette.syntax_keyword, "?>");
}

fn highlight_markup_body(
    palette: &Palette,
    result: &mut Vec<(Color32, String)>,
    markup_body: &str,
    name_color: Color32,
    closing_delimiter: &str,
) {
    let closing_start = markup_body
        .strip_suffix(closing_delimiter)
        .map(|body| body.len())
        .unwrap_or(markup_body.len());

    let body = &markup_body[..closing_start];
    let mut index = 0;
    let mut consumed_name = false;

    while index < body.len() {
        let rest = &body[index..];

        if rest.is_empty() {
            break;
        }

        if rest.starts_with('/') && closing_delimiter == ">" {
            push_colored(result, palette.syntax_tag_bracket, "/");
            index += 1;
            continue;
        }

        let ch = rest.chars().next().expect("non-empty string");
        if ch.is_whitespace() {
            let ws_len = leading_whitespace_len(rest);
            push_colored(result, palette.syntax_tag_bracket, &rest[..ws_len]);
            index += ws_len;
            continue;
        }

        if ch == '=' {
            push_colored(result, palette.syntax_tag_bracket, "=");
            index += ch.len_utf8();
            continue;
        }

        if ch == '"' || ch == '\'' {
            let quoted_len = quoted_string_len(rest, ch);
            push_entity_aware(
                result,
                &rest[..quoted_len],
                palette.syntax_attr_value,
                palette.syntax_keyword,
            );
            index += quoted_len;
            continue;
        }

        let name_len = xml_name_len(rest);
        if name_len > 0 {
            let color = if consumed_name {
                palette.syntax_attr_name
            } else {
                consumed_name = true;
                name_color
            };
            push_colored(result, color, &rest[..name_len]);
            index += name_len;
            continue;
        }

        push_colored(result, palette.syntax_tag_bracket, &rest[..ch.len_utf8()]);
        index += ch.len_utf8();
    }

    if markup_body.len() > closing_start {
        push_colored(result, palette.syntax_tag_bracket, closing_delimiter);
    }
}

fn push_entity_aware(
    result: &mut Vec<(Color32, String)>,
    text: &str,
    base_color: Color32,
    entity_color: Color32,
) {
    let mut index = 0;

    while index < text.len() {
        let Some(entity_start) = text[index..].find('&') else {
            push_colored(result, base_color, &text[index..]);
            break;
        };

        let entity_start = index + entity_start;
        if entity_start > index {
            push_colored(result, base_color, &text[index..entity_start]);
        }

        let Some(entity_end) = text[entity_start..].find(';') else {
            push_colored(result, base_color, &text[entity_start..]);
            break;
        };

        let entity_end = entity_start + entity_end + 1;
        let candidate = &text[entity_start..entity_end];
        if looks_like_entity(candidate) {
            push_colored(result, entity_color, candidate);
            index = entity_end;
        } else {
            push_colored(result, base_color, "&");
            index = entity_start + 1;
        }
    }
}

fn push_colored(result: &mut Vec<(Color32, String)>, color: Color32, text: &str) {
    if text.is_empty() {
        return;
    }

    if let Some((last_color, last_text)) = result.last_mut()
        && *last_color == color
    {
        last_text.push_str(text);
        return;
    }

    result.push((color, text.to_string()));
}

fn looks_like_entity(candidate: &str) -> bool {
    candidate.starts_with('&')
        && candidate.ends_with(';')
        && !candidate[1..candidate.len() - 1]
            .chars()
            .any(|ch| ch.is_whitespace() || matches!(ch, '<' | '>' | '&'))
}

fn find_tag_end(text: &str) -> usize {
    let mut quote = None;

    for (index, ch) in text.char_indices() {
        match quote {
            Some(delimiter) if ch == delimiter => quote = None,
            Some(_) => {}
            None if ch == '"' || ch == '\'' => quote = Some(ch),
            None if ch == '>' => return index + 1,
            None => {}
        }
    }

    text.len()
}

fn find_processing_instruction_end(text: &str) -> usize {
    let mut quote = None;

    for (index, ch) in text.char_indices() {
        match quote {
            Some(delimiter) if ch == delimiter => quote = None,
            Some(_) => {}
            None if ch == '"' || ch == '\'' => quote = Some(ch),
            None if text[index..].starts_with("?>") => return index + 2,
            None => {}
        }
    }

    text.len()
}

fn leading_whitespace_len(text: &str) -> usize {
    text.char_indices()
        .find(|(_, ch)| !ch.is_whitespace())
        .map(|(index, _)| index)
        .unwrap_or(text.len())
}

fn quoted_string_len(text: &str, delimiter: char) -> usize {
    let mut chars = text.char_indices();
    chars.next();

    for (index, ch) in chars {
        if ch == delimiter {
            return index + ch.len_utf8();
        }
    }

    text.len()
}

fn xml_name_len(text: &str) -> usize {
    let mut chars = text.char_indices();
    let Some((_, first)) = chars.next() else {
        return 0;
    };

    if !is_xml_name_start(first) {
        return 0;
    }

    for (index, ch) in chars {
        if !is_xml_name_char(ch) {
            return index;
        }
    }

    text.len()
}

fn is_xml_name_start(ch: char) -> bool {
    ch == '_' || ch == ':' || ch.is_alphabetic()
}

fn is_xml_name_char(ch: char) -> bool {
    is_xml_name_start(ch) || ch.is_ascii_digit() || matches!(ch, '-' | '.')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::Theme as AppTheme;

    fn mocha() -> Palette {
        let ctx = egui::Context::default();
        ctx.set_theme(egui::ThemePreference::Dark);
        Palette::resolve(&ctx)
    }

    fn has_segment(tokens: &[(Color32, String)], color: Color32, fragment: &str) -> bool {
        tokens
            .iter()
            .any(|(token_color, text)| *token_color == color && text.contains(fragment))
    }

    #[test]
    fn test_highlight_simple_xml() {
        let highlighter = SyntaxHighlighter::new();
        let palette = mocha();
        let xml = r#"<root attr="value">text</root>"#;
        let result = highlighter.highlight_xml(&palette, xml);

        assert!(has_segment(&result, palette.syntax_tag, "root"));
        assert!(has_segment(&result, palette.syntax_attr_name, "attr"));
        assert!(has_segment(&result, palette.syntax_attr_value, "\"value\""));
        assert!(has_segment(&result, palette.syntax_text, "text"));
    }

    #[test]
    fn test_highlight_special_xml_constructs() {
        let highlighter = SyntaxHighlighter::new();
        let palette = mocha();
        let xml = r#"<!-- note --><![CDATA[<raw>]]><?xml-stylesheet href="style.xsl"?>&amp;"#;
        let result = highlighter.highlight_xml(&palette, xml);

        assert!(has_segment(
            &result,
            palette.syntax_comment,
            "<!-- note -->"
        ));
        assert!(has_segment(&result, palette.syntax_keyword, "<![CDATA["));
        assert!(has_segment(&result, palette.syntax_text, "<raw>"));
        assert!(has_segment(
            &result,
            palette.syntax_keyword,
            "xml-stylesheet"
        ));
        assert!(has_segment(&result, palette.syntax_keyword, "&amp;"));
    }

    #[test]
    fn test_highlight_lines_preserves_line_count() {
        let highlighter = SyntaxHighlighter::new();
        let palette = mocha();
        let xml = "<root>\n  <child>text</child>\n</root>";
        let result = highlighter.highlight_xml_lines(&palette, xml);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn unterminated_cdata_keeps_every_character() {
        let highlighter = SyntaxHighlighter::new();
        let palette = mocha();
        let xml = "<root><![CDATA[unterminated";
        let result = highlighter.highlight_xml(&palette, xml);
        let rendered: String = result.iter().map(|(_, text)| text.as_str()).collect();
        assert_eq!(rendered, xml, "highlighting must not drop characters");
    }

    #[test]
    fn dark_palette_matches_the_legacy_mocha_tokens() {
        // The legacy `Theme` constants documented Mocha; the resolved dark
        // palette must keep the same accents where the names overlap.
        let palette = mocha();
        assert_eq!(palette.syntax_tag, AppTheme::SYNTAX_TAG);
        assert_eq!(palette.syntax_attr_name, AppTheme::SYNTAX_ATTR_NAME);
        assert_eq!(palette.syntax_attr_value, AppTheme::SYNTAX_ATTR_VALUE);
        assert_eq!(palette.syntax_keyword, AppTheme::SYNTAX_KEYWORD);
    }
}
