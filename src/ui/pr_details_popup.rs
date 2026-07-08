use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::app::{App, DiffSource};
use crate::ui::{markdown_render, styles};

pub fn render_pr_details(frame: &mut Frame, app: &mut App) {
    let theme = &app.theme;
    let anchor = app.diff_area.unwrap_or(frame.area());
    let area = centered_rect(85, 90, anchor);

    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" PR Details (j/k to scroll) - Press q or Esc to close ")
        .borders(Borders::ALL)
        .style(styles::popup_style(theme))
        .border_style(styles::border_style(theme, true));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines = pr_details_lines(app);
    let paragraph = Paragraph::new(lines)
        .style(styles::popup_style(theme))
        .wrap(Wrap { trim: false });

    let total_lines = if inner.width == 0 {
        1
    } else {
        paragraph.line_count(inner.width).max(1)
    };
    let viewport_height = inner.height as usize;
    app.details_state.total_lines = total_lines;
    app.details_state.viewport_height = viewport_height;

    let max_offset = total_lines.saturating_sub(viewport_height);
    app.details_state.scroll_offset = app.details_state.scroll_offset.min(max_offset);

    let can_scroll_up = app.details_state.scroll_offset > 0;
    let can_scroll_down = app.details_state.scroll_offset + viewport_height < total_lines;

    frame.render_widget(
        paragraph.scroll((app.details_state.scroll_offset as u16, 0)),
        inner,
    );

    let indicator_style = styles::help_indicator_style(theme);
    if can_scroll_up {
        let up_indicator = Paragraph::new(Line::from(Span::styled("▲ more", indicator_style)));
        let up_area = Rect {
            x: inner.x + inner.width.saturating_sub(8),
            y: inner.y,
            width: 7,
            height: 1,
        };
        frame.render_widget(up_indicator, up_area);
    }

    if can_scroll_down {
        let down_indicator = Paragraph::new(Line::from(Span::styled("▼ more", indicator_style)));
        let down_area = Rect {
            x: inner.x + inner.width.saturating_sub(8),
            y: inner.y + inner.height.saturating_sub(1),
            width: 7,
            height: 1,
        };
        frame.render_widget(down_indicator, down_area);
    }
}

fn pr_details_lines(app: &App) -> Vec<Line<'static>> {
    let theme = &app.theme;
    let DiffSource::PullRequest(pr) = &app.diff_source else {
        return vec![Line::from(Span::styled(
            "No pull request is open.",
            styles::dim_style(theme),
        ))];
    };

    let label_style = Style::default()
        .fg(theme.fg_primary)
        .add_modifier(Modifier::BOLD);
    let value_style = Style::default().fg(theme.fg_secondary);
    let title_style = Style::default()
        .fg(theme.fg_primary)
        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    let section_style = Style::default()
        .fg(theme.diff_hunk_header)
        .add_modifier(Modifier::BOLD);

    let mut lines = vec![
        Line::from(Span::styled(
            format!(
                "{}#{}: {}",
                pr.key.repository.display_name(),
                pr.key.number,
                pr.title
            ),
            title_style,
        )),
        Line::from(""),
        detail_line("URL", &pr.url, label_style, value_style),
        detail_line(
            "Author",
            pr.author.as_deref().unwrap_or("unknown"),
            label_style,
            value_style,
        ),
        detail_line("State", &pr.state, label_style, value_style),
        detail_line(
            "Branches",
            &format!("{} → {}", pr.head_ref_name, pr.base_ref_name),
            label_style,
            value_style,
        ),
        detail_line("Head", &pr.key.short_head(), label_style, value_style),
        Line::from(""),
        Line::from(Span::styled("Description", section_style)),
        Line::from(""),
    ];

    lines.extend(markdown_render::render_markdown_lines(theme, &pr.body));
    lines
}

fn detail_line(label: &str, value: &str, label_style: Style, value_style: Style) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label}: "), label_style),
        Span::styled(value.to_string(), value_style),
    ])
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([Constraint::Percentage(percent_y)]).flex(Flex::Center);
    let horizontal = Layout::horizontal([Constraint::Percentage(percent_x)]).flex(Flex::Center);
    let [area] = vertical.areas(area);
    let [area] = horizontal.areas(area);
    area
}
