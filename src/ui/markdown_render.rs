use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::theme::Theme;
use crate::ui::styles;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockKind {
    Paragraph,
    Heading(HeadingLevel),
    ListItem,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ListState {
    ordered: bool,
    next: u64,
}

#[derive(Debug, Default)]
struct StyleState {
    strong: usize,
    emphasis: usize,
    strikethrough: usize,
    link: usize,
}

struct MarkdownRenderer<'a> {
    theme: &'a Theme,
    lines: Vec<Line<'static>>,
    current: Vec<Span<'static>>,
    block: Option<BlockKind>,
    lists: Vec<ListState>,
    quote_depth: usize,
    style: StyleState,
    link_stack: Vec<String>,
    in_code_block: bool,
    code_block_language: Option<String>,
}

/// Render GitHub/GitLab-style Markdown into ratatui lines without exposing the
/// raw Markdown delimiters. This targets terminal readability rather than a
/// byte-for-byte browser layout: emphasis, headings, links, lists, block
/// quotes, rules, and code blocks are mapped to styled text.
pub fn render_markdown_lines(theme: &Theme, markdown: &str) -> Vec<Line<'static>> {
    let mut renderer = MarkdownRenderer::new(theme);
    let parser = Parser::new_ext(markdown, Options::all());
    for event in parser {
        renderer.handle_event(event);
    }
    renderer.finish()
}

impl<'a> MarkdownRenderer<'a> {
    fn new(theme: &'a Theme) -> Self {
        Self {
            theme,
            lines: Vec::new(),
            current: Vec::new(),
            block: None,
            lists: Vec::new(),
            quote_depth: 0,
            style: StyleState::default(),
            link_stack: Vec::new(),
            in_code_block: false,
            code_block_language: None,
        }
    }

    fn handle_event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start_tag(tag),
            Event::End(tag) => self.end_tag(tag),
            Event::Text(text) => {
                if self.in_code_block {
                    self.push_code_text(&text);
                } else {
                    self.push_text(&text);
                }
            }
            Event::Code(code) => self.push_span(
                code.to_string(),
                self.inline_style().fg(self.theme.diff_hunk_header),
            ),
            Event::InlineMath(math) => self.push_span(
                math.to_string(),
                self.inline_style().fg(self.theme.diff_hunk_header),
            ),
            Event::DisplayMath(math) => {
                self.flush_current();
                for line in math.lines() {
                    self.lines.push(Line::from(Span::styled(
                        format!("    {line}"),
                        self.code_style(),
                    )));
                }
                self.push_blank_if_needed();
            }
            Event::SoftBreak => self.push_text(" "),
            Event::HardBreak => self.flush_current(),
            Event::Rule => {
                self.flush_current();
                self.lines.push(Line::from(Span::styled(
                    "─".repeat(40),
                    styles::dim_style(self.theme),
                )));
                self.push_blank_if_needed();
            }
            Event::Html(html) | Event::InlineHtml(html) => self.push_text(&html),
            Event::FootnoteReference(reference) => {
                self.push_span(format!("[{reference}]"), styles::dim_style(self.theme));
            }
            Event::TaskListMarker(checked) => {
                self.push_span(
                    if checked { "☑ " } else { "☐ " }.to_string(),
                    self.inline_style(),
                );
            }
        }
    }

    fn start_tag(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => self.block = Some(BlockKind::Paragraph),
            Tag::Heading { level, .. } => self.block = Some(BlockKind::Heading(level)),
            Tag::BlockQuote(_) => self.quote_depth += 1,
            Tag::CodeBlock(kind) => {
                self.flush_current();
                self.in_code_block = true;
                self.code_block_language = match kind {
                    CodeBlockKind::Fenced(lang) if !lang.is_empty() => Some(lang.to_string()),
                    _ => None,
                };
                if let Some(lang) = &self.code_block_language {
                    self.lines.push(Line::from(Span::styled(
                        format!("code: {lang}"),
                        styles::dim_style(self.theme),
                    )));
                }
            }
            Tag::List(start) => self.lists.push(ListState {
                ordered: start.is_some(),
                next: start.unwrap_or(1),
            }),
            Tag::Item => self.block = Some(BlockKind::ListItem),
            Tag::Emphasis => self.style.emphasis += 1,
            Tag::Strong => self.style.strong += 1,
            Tag::Strikethrough => self.style.strikethrough += 1,
            Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => {
                self.style.link += 1;
                self.link_stack.push(dest_url.to_string());
            }
            Tag::Table(_)
            | Tag::TableHead
            | Tag::TableRow
            | Tag::TableCell
            | Tag::FootnoteDefinition(_)
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition
            | Tag::HtmlBlock => {}
            Tag::MetadataBlock(_) | Tag::Superscript | Tag::Subscript => {}
        }
    }

    fn end_tag(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                self.flush_current();
                if self.lists.is_empty() {
                    self.push_blank_if_needed();
                }
                self.block = None;
            }
            TagEnd::Heading(_) => {
                self.flush_current();
                self.push_blank_if_needed();
                self.block = None;
            }
            TagEnd::BlockQuote(_) => {
                self.flush_current();
                self.quote_depth = self.quote_depth.saturating_sub(1);
                self.push_blank_if_needed();
            }
            TagEnd::CodeBlock => {
                self.in_code_block = false;
                self.code_block_language = None;
                self.push_blank_if_needed();
            }
            TagEnd::List(_) => {
                self.flush_current();
                self.lists.pop();
                self.push_blank_if_needed();
            }
            TagEnd::Item => {
                self.flush_current();
                if let Some(list) = self.lists.last_mut() {
                    list.next += 1;
                }
                self.block = None;
            }
            TagEnd::Emphasis => self.style.emphasis = self.style.emphasis.saturating_sub(1),
            TagEnd::Strong => self.style.strong = self.style.strong.saturating_sub(1),
            TagEnd::Strikethrough => {
                self.style.strikethrough = self.style.strikethrough.saturating_sub(1)
            }
            TagEnd::Link | TagEnd::Image => {
                self.style.link = self.style.link.saturating_sub(1);
                if let Some(url) = self.link_stack.pop()
                    && !url.is_empty()
                {
                    self.push_span(format!(" ({url})"), styles::dim_style(self.theme));
                }
            }
            TagEnd::Table
            | TagEnd::TableHead
            | TagEnd::TableRow
            | TagEnd::TableCell
            | TagEnd::FootnoteDefinition
            | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition
            | TagEnd::HtmlBlock
            | TagEnd::MetadataBlock(_)
            | TagEnd::Superscript
            | TagEnd::Subscript => {}
        }
    }

    fn push_text(&mut self, text: &str) {
        let mut parts = text.split('\n').peekable();
        while let Some(part) = parts.next() {
            if !part.is_empty() {
                self.push_span(part.to_string(), self.inline_style());
            }
            if parts.peek().is_some() {
                self.flush_current();
            }
        }
    }

    fn push_code_text(&mut self, text: &str) {
        for line in text.lines() {
            self.lines.push(Line::from(Span::styled(
                format!("    {line}"),
                self.code_style(),
            )));
        }
        if text.ends_with('\n') {
            self.lines
                .push(Line::from(Span::styled("    ", self.code_style())));
        }
    }

    fn push_span(&mut self, text: String, style: Style) {
        self.ensure_prefix();
        self.current.push(Span::styled(text, style));
    }

    fn ensure_prefix(&mut self) {
        if !self.current.is_empty() {
            return;
        }
        let mut prefix = String::new();
        if self.quote_depth > 0 {
            prefix.push_str(&"│ ".repeat(self.quote_depth));
        }
        if let Some(BlockKind::ListItem) = self.block {
            let indent = "  ".repeat(self.lists.len().saturating_sub(1));
            prefix.push_str(&indent);
            if let Some(list) = self.lists.last() {
                if list.ordered {
                    prefix.push_str(&format!("{}. ", list.next));
                } else {
                    prefix.push_str("• ");
                }
            } else {
                prefix.push_str("• ");
            }
        }
        if !prefix.is_empty() {
            self.current
                .push(Span::styled(prefix, styles::dim_style(self.theme)));
        }
    }

    fn flush_current(&mut self) {
        if self.current.is_empty() {
            return;
        }
        let block = self.block;
        let spans = std::mem::take(&mut self.current)
            .into_iter()
            .map(|mut span| {
                if let Some(BlockKind::Heading(level)) = block {
                    span.style = span.style.patch(self.heading_style(level));
                }
                span
            })
            .collect::<Vec<_>>();
        self.lines.push(Line::from(spans));
    }

    fn push_blank_if_needed(&mut self) {
        if !self.lines.last().is_some_and(|line| line.spans.is_empty()) {
            self.lines.push(Line::from(""));
        }
    }

    fn inline_style(&self) -> Style {
        let mut style = Style::default().fg(self.theme.fg_secondary);
        if self.style.strong > 0 {
            style = style.add_modifier(Modifier::BOLD);
        }
        if self.style.emphasis > 0 {
            style = style.add_modifier(Modifier::ITALIC);
        }
        if self.style.strikethrough > 0 {
            style = style.add_modifier(Modifier::CROSSED_OUT);
        }
        if self.style.link > 0 {
            style = style
                .fg(self.theme.diff_hunk_header)
                .add_modifier(Modifier::UNDERLINED);
        }
        style
    }

    fn heading_style(&self, level: HeadingLevel) -> Style {
        let base = Style::default()
            .fg(self.theme.fg_primary)
            .add_modifier(Modifier::BOLD);
        match level {
            HeadingLevel::H1 | HeadingLevel::H2 => base.add_modifier(Modifier::UNDERLINED),
            _ => base,
        }
    }

    fn code_style(&self) -> Style {
        Style::default()
            .fg(self.theme.fg_primary)
            .bg(self.theme.section_highlight_bg())
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        self.flush_current();
        while self.lines.last().is_some_and(|line| line.spans.is_empty()) {
            self.lines.pop();
        }
        if self.lines.is_empty() {
            vec![Line::from(Span::styled(
                "No description provided.",
                styles::dim_style(self.theme),
            ))]
        } else {
            self.lines
        }
    }
}

#[cfg(test)]
mod tests {
    use super::render_markdown_lines;
    use crate::theme::Theme;

    fn plain_text(lines: &[ratatui::text::Line<'static>]) -> String {
        lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn renders_common_markdown_without_raw_delimiters() {
        let theme = Theme::default();
        let lines = render_markdown_lines(
            &theme,
            "## Summary\n\n- **Fixes** `panic`\n- [docs](https://example.test)\n\n> quoted",
        );
        let text = plain_text(&lines);

        assert!(text.contains("Summary"));
        assert!(text.contains("• Fixes panic"));
        assert!(text.contains("docs (https://example.test)"));
        assert!(text.contains("│ quoted"));
        assert!(!text.contains("##"));
        assert!(!text.contains("**"));
        assert!(!text.contains('`'));
        assert!(!text.contains("]("));
    }

    #[test]
    fn renders_empty_markdown_as_placeholder() {
        let theme = Theme::default();
        let lines = render_markdown_lines(&theme, "   \n");
        let text = plain_text(&lines);

        assert_eq!(text, "No description provided.");
    }
}
