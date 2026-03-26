use egui::Color32;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

use super::theme::Theme as AppTheme;

/// Syntax highlighter for XML code
pub struct SyntaxHighlighter {
    syntax_set: SyntaxSet,
    theme: Theme,
}

impl Default for SyntaxHighlighter {
    fn default() -> Self {
        Self::new()
    }
}

impl SyntaxHighlighter {
    pub fn new() -> Self {
        let syntax_set = SyntaxSet::load_defaults_newlines();
        let theme_set = ThemeSet::load_defaults();
        
        // Use a dark theme that matches Catppuccin Mocha
        let theme = theme_set.themes.get("base16-ocean.dark")
            .or_else(|| theme_set.themes.get("Monokai Extended"))
            .or_else(|| theme_set.themes.values().next())
            .expect("No themes available")
            .clone();
        
        Self {
            syntax_set,
            theme,
        }
    }
    
    /// Highlight XML text and return styled segments
    pub fn highlight_xml(&self, text: &str) -> Vec<(Color32, String)> {
        let syntax = self.syntax_set
            .find_syntax_by_extension("xml")
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text());
        
        let mut highlighter = HighlightLines::new(syntax, &self.theme);
        let mut result = Vec::new();
        
        for line in LinesWithEndings::from(text) {
            let ranges = highlighter
                .highlight_line(line, &self.syntax_set)
                .unwrap_or_default();
            
            for (style, text) in ranges {
                let color = convert_syntect_color(style.foreground);
                result.push((color, text.to_string()));
            }
        }
        
        result
    }
    
    /// Highlight XML with custom Catppuccin colors
    pub fn highlight_xml_custom(&self, text: &str) -> Vec<(Color32, String)> {
        // Simple regex-based highlighter for better color control
        let mut result = Vec::new();
        let mut chars = text.chars().peekable();
        let mut current_text = String::new();
        let mut in_tag = false;
        let mut in_attr_value = false;
        let mut in_comment = false;
        
        while let Some(ch) = chars.next() {
            if in_comment {
                current_text.push(ch);
                if ch == '-' && chars.peek() == Some(&'-') {
                    current_text.push(chars.next().unwrap());
                    if chars.peek() == Some(&'>') {
                        current_text.push(chars.next().unwrap());
                        result.push((AppTheme::COMMENT, current_text.clone()));
                        current_text.clear();
                        in_comment = false;
                    }
                }
            } else if ch == '<' {
                if !current_text.is_empty() {
                    result.push((AppTheme::TEXT_CONTENT, current_text.clone()));
                    current_text.clear();
                }
                
                // Check for comment
                if chars.peek() == Some(&'!') {
                    current_text.push(ch);
                    current_text.push(chars.next().unwrap());
                    if chars.peek() == Some(&'-') {
                        current_text.push(chars.next().unwrap());
                        if chars.peek() == Some(&'-') {
                            current_text.push(chars.next().unwrap());
                            in_comment = true;
                        }
                    }
                } else {
                    current_text.push(ch);
                    in_tag = true;
                }
            } else if ch == '>' && in_tag {
                current_text.push(ch);
                result.push((AppTheme::ELEMENT_NAME, current_text.clone()));
                current_text.clear();
                in_tag = false;
                in_attr_value = false;
            } else if ch == '"' && in_tag {
                if in_attr_value {
                    current_text.push(ch);
                    result.push((AppTheme::ATTRIBUTE_VALUE, current_text.clone()));
                    current_text.clear();
                    in_attr_value = false;
                } else {
                    if !current_text.is_empty() {
                        result.push((AppTheme::ATTRIBUTE_KEY, current_text.clone()));
                        current_text.clear();
                    }
                    current_text.push(ch);
                    in_attr_value = true;
                }
            } else {
                current_text.push(ch);
            }
        }
        
        if !current_text.is_empty() {
            let color = if in_comment {
                AppTheme::COMMENT
            } else if in_tag {
                AppTheme::ELEMENT_NAME
            } else {
                AppTheme::TEXT_CONTENT
            };
            result.push((color, current_text));
        }
        
        result
    }
}

/// Convert syntect color to egui Color32
fn convert_syntect_color(color: syntect::highlighting::Color) -> Color32 {
    Color32::from_rgb(color.r, color.g, color.b)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_highlight_simple_xml() {
        let highlighter = SyntaxHighlighter::new();
        let xml = r#"<root attr="value">text</root>"#;
        let result = highlighter.highlight_xml(xml);
        assert!(!result.is_empty());
    }
    
    #[test]
    fn test_highlight_custom() {
        let highlighter = SyntaxHighlighter::new();
        let xml = r#"<root>text</root>"#;
        let result = highlighter.highlight_xml_custom(xml);
        assert!(!result.is_empty());
    }
}
