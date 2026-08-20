//! Viewport-bounded diff document rendering.
//!
//! `App::line_annotations` is the persistent row index for the diff document.
//! Rendering resolves only the annotation rows intersecting the terminal
//! viewport. This is the same lazy-window pattern used by Ratatui's `List` and
//! by gitui's diff component: document layout is indexed separately from the
//! expensive construction of styled terminal rows.

use std::collections::{HashMap, HashSet};

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use crate::app::{AnnotatedLine, App, DiffViewMode, FocusedPanel, GapId, InputMode};
use crate::model::{CommentType, DiffLine, LineOrigin, LineRange, LineSide};
use crate::ui::{comment_panel, pr_info_panel, styles};

use super::diff_view::{
    CommentBarAnchor, DiffOverlayPaint, HEADER_RULE, REVIEW_COMMENTS_HEADER_PREFIX,
    REVIEWED_BANNER_TEXT, apply_horizontal_scroll, cursor_indicator, cursor_indicator_spaced,
    diff_stat_title, paint_comment_box_bar, paint_comment_box_right_border,
    paint_cursor_line_highlight, paint_file_header_fill, paint_section_highlight,
    paint_unified_diff_rows_with, paint_visual_selection_overlay, populate_row_to_annotation,
    push_comment_bar, spacing_next_file_hint_text, unified_line_bg_style,
};

struct CommentInput {
    start: usize,
    replaced: usize,
    lines: Vec<Line<'static>>,
    cursor_line_offset: usize,
    cursor_column: u16,
}

impl CommentInput {
    fn end(&self) -> usize {
        self.start + self.lines.len()
    }

    fn total_rows(&self, annotations: usize) -> usize {
        annotations + self.lines.len() - self.replaced.min(annotations)
    }
}

#[derive(Clone, Copy)]
enum DocumentRow {
    Annotation(usize),
    CommentInput(usize),
}

fn document_row(input: Option<&CommentInput>, row: usize) -> Option<DocumentRow> {
    let Some(input) = input else {
        return Some(DocumentRow::Annotation(row));
    };
    if row < input.start {
        Some(DocumentRow::Annotation(row))
    } else if row < input.end() {
        Some(DocumentRow::CommentInput(row - input.start))
    } else {
        Some(DocumentRow::Annotation(
            row - input.lines.len() + input.replaced,
        ))
    }
}

fn build_comment_input(app: &App, width: usize) -> Option<CommentInput> {
    if app.input_mode != InputMode::Comment {
        return None;
    }
    let (start, replaced) = app.comment_input_annotation_anchor?;
    let line_range = if app.comment_is_review_level || app.comment_is_file_level {
        None
    } else {
        app.comment_line_range
            .map(|(range, _)| range)
            .or_else(|| app.comment_line.map(|(line, _)| LineRange::single(line)))
    };
    let (lines, cursor) = comment_panel::format_comment_input_lines(
        &app.theme,
        super::diff_view::comment_type_presentation(app, &app.comment_type),
        &app.comment_buffer,
        app.comment_cursor,
        line_range,
        app.editing_comment_id.is_some(),
        width.saturating_sub(1),
        app.comment_vim_mode_label()
            .as_ref()
            .map(|(text, width)| (text.as_str(), *width)),
    );
    Some(CommentInput {
        start,
        replaced,
        lines,
        cursor_line_offset: cursor.line_offset,
        cursor_column: 1 + cursor.column,
    })
}

fn same_block(a: &AnnotatedLine, b: &AnnotatedLine) -> bool {
    use AnnotatedLine::*;
    match (a, b) {
        (PrInfoLine { .. }, PrInfoLine { .. }) => true,
        (IssueComment { comment_idx: a }, IssueComment { comment_idx: b }) => a == b,
        (ReviewComment { comment_idx: a }, ReviewComment { comment_idx: b }) => a == b,
        (
            RemoteReviewSummaryLine { summary_idx: a },
            RemoteReviewSummaryLine { summary_idx: b },
        ) => a == b,
        (
            FileComment {
                file_idx: af,
                comment_idx: ac,
            },
            FileComment {
                file_idx: bf,
                comment_idx: bc,
            },
        ) => af == bf && ac == bc,
        (
            LineComment {
                file_idx: af,
                line: al,
                side: as_,
                comment_idx: ac,
            },
            LineComment {
                file_idx: bf,
                line: bl,
                side: bs,
                comment_idx: bc,
            },
        ) => af == bf && al == bl && as_ == bs && ac == bc,
        (RemoteThreadLine { thread_idx: a }, RemoteThreadLine { thread_idx: b }) => a == b,
        _ => false,
    }
}

fn block_start(annotations: &[AnnotatedLine], idx: usize) -> usize {
    let Some(annotation) = annotations.get(idx) else {
        return idx;
    };
    let mut start = idx;
    while start > 0 && same_block(&annotations[start - 1], annotation) {
        start -= 1;
    }
    start
}

fn add_indicator(mut line: Line<'static>, row: usize, app: &App) -> Line<'static> {
    line.spans.insert(
        0,
        Span::styled(
            cursor_indicator(row, app.diff_state.cursor_line),
            styles::current_line_indicator_style(&app.theme),
        ),
    );
    line
}

fn comment_block_lines(app: &App, annotation: &AnnotatedLine, width: usize) -> Vec<Line<'static>> {
    let content_width = width.saturating_sub(1);
    match annotation {
        AnnotatedLine::PrInfoLine { .. } => app
            .pr_info
            .as_ref()
            .map(|info| {
                pr_info_panel::build_pr_info_lines(
                    info,
                    pr_info_panel::pr_info_content_width(width),
                    &app.theme,
                )
            })
            .unwrap_or_default(),
        AnnotatedLine::IssueComment { comment_idx } => app
            .pr_info
            .as_ref()
            .and_then(|info| info.issue_comments.get(*comment_idx))
            .map(|comment| {
                let note_type = CommentType::from_id("note");
                let presentation = comment_panel::CommentTypePresentation {
                    label: app.comment_type_label(&note_type),
                    color: app.comment_type_color(&note_type),
                };
                pr_info_panel::format_issue_comment_lines(
                    &app.theme,
                    comment,
                    content_width,
                    &presentation,
                )
            })
            .unwrap_or_default(),
        AnnotatedLine::ReviewComment { comment_idx } => app
            .session
            .review_comments
            .get(*comment_idx)
            .map(|comment| {
                comment_panel::format_comment_lines(
                    &app.theme,
                    super::diff_view::comment_type_presentation(app, &comment.comment_type),
                    &comment.content,
                    None,
                    content_width,
                    (comment.author != app.username).then_some(comment.author.as_str()),
                )
            })
            .unwrap_or_default(),
        AnnotatedLine::RemoteReviewSummaryLine { summary_idx } => app
            .forge_review_summaries
            .get(*summary_idx)
            .map(|summary| {
                comment_panel::format_remote_review_summary_lines(
                    &app.theme,
                    summary,
                    app.forge_kind(),
                )
            })
            .unwrap_or_default(),
        AnnotatedLine::FileComment {
            file_idx,
            comment_idx,
        } => app
            .diff_files
            .get(*file_idx)
            .and_then(|file| app.session.files.get(file.display_path()))
            .and_then(|review| review.file_comments.get(*comment_idx))
            .map(|comment| {
                comment_panel::format_comment_lines(
                    &app.theme,
                    super::diff_view::comment_type_presentation(app, &comment.comment_type),
                    &comment.content,
                    None,
                    content_width,
                    (comment.author != app.username).then_some(comment.author.as_str()),
                )
            })
            .unwrap_or_default(),
        AnnotatedLine::LineComment {
            file_idx,
            line,
            side,
            comment_idx,
        } => app
            .diff_files
            .get(*file_idx)
            .and_then(|file| app.session.files.get(file.display_path()))
            .and_then(|review| review.line_comments.get(line))
            .and_then(|comments| comments.get(*comment_idx))
            .map(|comment| {
                comment_panel::format_comment_lines(
                    &app.theme,
                    super::diff_view::comment_type_presentation(app, &comment.comment_type),
                    &comment.content,
                    comment
                        .line_range
                        .or_else(|| Some(LineRange::single(*line))),
                    content_width,
                    (comment.author != app.username).then_some(comment.author.as_str()),
                )
            })
            .filter(|_| {
                // Annotation indices are absolute into the stored comment Vec.
                app.diff_files
                    .get(*file_idx)
                    .and_then(|file| app.session.files.get(file.display_path()))
                    .and_then(|review| review.line_comments.get(line))
                    .and_then(|comments| comments.get(*comment_idx))
                    .is_some_and(|comment| comment.side.unwrap_or(LineSide::New) == *side)
            })
            .unwrap_or_default(),
        AnnotatedLine::RemoteThreadLine { thread_idx } => app
            .forge_review_threads
            .get(*thread_idx)
            .and_then(|thread| {
                app.session
                    .remote_comments_visibility
                    .render_decision(thread)
                    .map(|muted| (thread, muted))
            })
            .map(|(thread, muted)| {
                comment_panel::format_remote_thread_lines(
                    &app.theme,
                    thread,
                    muted,
                    app.forge_kind(),
                )
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn gap_remaining(app: &App, gap: &GapId) -> usize {
    let Some(file) = app.diff_files.get(gap.file_idx) else {
        return 0;
    };
    let gap_size = if let Some(hunk) = file.hunks.get(gap.hunk_idx) {
        let previous = gap
            .hunk_idx
            .checked_sub(1)
            .and_then(|idx| file.hunks.get(idx));
        crate::vcs::git::calculate_gap(
            previous.map(|hunk| (&hunk.new_start, &hunk.new_count)),
            hunk.new_start,
        ) as usize
    } else if gap.hunk_idx == file.hunks.len() {
        file.hunks
            .last()
            .and_then(|last| {
                app.file_line_count_cache.get(&gap.file_idx).map(|total| {
                    let start = last.new_start + last.new_count;
                    if start <= *total {
                        (*total - start + 1) as usize
                    } else {
                        0
                    }
                })
            })
            .unwrap_or(0)
    } else {
        0
    };
    gap_size.saturating_sub(
        app.expanded_top.get(gap).map_or(0, Vec::len)
            + app.expanded_bottom.get(gap).map_or(0, Vec::len),
    )
}

fn expanded_line<'a>(app: &'a App, gap: &GapId, idx: usize) -> Option<&'a DiffLine> {
    let top = app.expanded_top.get(gap);
    let top_len = top.map_or(0, Vec::len);
    if idx < top_len {
        top.and_then(|lines| lines.get(idx))
    } else {
        app.expanded_bottom
            .get(gap)
            .and_then(|lines| lines.get(idx - top_len))
    }
}

fn unified_diff_line(
    app: &App,
    diff_line: &DiffLine,
    row: usize,
    file_idx: usize,
) -> Line<'static> {
    let lw = app.lineno_width();
    let style = match diff_line.origin {
        LineOrigin::Addition => styles::diff_add_style(&app.theme),
        LineOrigin::Deletion => styles::diff_del_style(&app.theme),
        LineOrigin::Context => styles::diff_context_style(&app.theme),
    };
    let line_num = if app.diff_files[file_idx].is_commit_message {
        " ".repeat(lw + 1)
    } else if app.relative_line_numbers {
        super::diff_view::relative_line_number_field(
            diff_line.new_lineno.or(diff_line.old_lineno),
            row,
            app.diff_state.cursor_line,
            lw,
        )
    } else {
        super::diff_view::unified_line_number_field(diff_line, lw)
    };
    let mut spans = vec![
        Span::styled(
            cursor_indicator(row, app.diff_state.cursor_line),
            styles::current_line_indicator_style(&app.theme),
        ),
        Span::styled(line_num, styles::dim_style(&app.theme)),
        Span::styled(
            format!(
                "{} ",
                super::diff_view::unified_line_origin_marker(diff_line)
            ),
            style,
        ),
    ];
    let content_start = spans.len();
    if let Some(highlighted) = &diff_line.highlighted_spans {
        spans.extend(
            highlighted
                .iter()
                .map(|(style, text)| Span::styled(text.clone(), *style)),
        );
    } else {
        spans.push(Span::styled(diff_line.content.clone(), style));
    }
    let eol = matches!(
        diff_line.origin,
        LineOrigin::Addition | LineOrigin::Deletion
    )
    .then(|| {
        let eol_style = match diff_line.highlighted_spans.as_ref() {
            Some(_) => {
                let background = match diff_line.origin {
                    LineOrigin::Addition => app.theme.syntax_add_bg,
                    LineOrigin::Deletion => app.theme.syntax_del_bg,
                    LineOrigin::Context => app.theme.panel_bg,
                };
                spans
                    .last()
                    .map(|span| span.style)
                    .unwrap_or(style)
                    .bg(background)
            }
            None => spans.last().map(|span| span.style).unwrap_or(style),
        };
        Span::styled(String::new(), eol_style)
    });
    if let Some(needle) = app.search_paint_at(row) {
        let content = spans.split_off(content_start);
        spans.extend(crate::ui::text_utils::apply_search_highlight_spans(
            content,
            needle,
            styles::search_match_style(&app.theme),
        ));
    }
    spans.extend(eol);
    Line::from(spans)
}

fn simple_annotation_line(
    app: &App,
    annotation: &AnnotatedLine,
    row: usize,
    mode: DiffViewMode,
) -> Line<'static> {
    match annotation {
        AnnotatedLine::ReviewCommentsHeader => Line::from(vec![
            Span::styled(
                cursor_indicator_spaced(row, app.diff_state.cursor_line),
                styles::current_line_indicator_style(&app.theme),
            ),
            Span::styled(
                REVIEW_COMMENTS_HEADER_PREFIX,
                styles::file_header_style(&app.theme),
            ),
            Span::styled(HEADER_RULE, styles::file_header_style(&app.theme)),
        ]),
        AnnotatedLine::IssueCommentsHeader => Line::from(vec![
            Span::styled(
                cursor_indicator_spaced(row, app.diff_state.cursor_line),
                styles::current_line_indicator_style(&app.theme),
            ),
            Span::styled(
                format!(
                    "═══ PR #{} Comments ",
                    app.pr_info
                        .as_ref()
                        .map(|info| info.details.number)
                        .unwrap_or_default()
                ),
                styles::file_header_style(&app.theme),
            ),
            Span::styled(HEADER_RULE, styles::file_header_style(&app.theme)),
        ]),
        AnnotatedLine::FileHeader { file_idx } => app
            .diff_files
            .get(*file_idx)
            .map(|file| {
                Line::from(vec![
                    Span::styled(
                        cursor_indicator_spaced(row, app.diff_state.cursor_line),
                        styles::current_line_indicator_style(&app.theme),
                    ),
                    Span::styled(
                        super::diff_view::file_header_prefix_text(app, file),
                        styles::file_header_style(&app.theme),
                    ),
                    Span::styled(HEADER_RULE, styles::file_header_style(&app.theme)),
                ])
            })
            .unwrap_or_default(),
        AnnotatedLine::ReviewedBanner { .. } => Line::from(vec![
            Span::styled(
                cursor_indicator(row, app.diff_state.cursor_line),
                styles::current_line_indicator_style(&app.theme),
            ),
            Span::styled(
                REVIEWED_BANNER_TEXT,
                Style::default()
                    .fg(app.theme.fg_secondary)
                    .add_modifier(Modifier::DIM),
            ),
        ]),
        AnnotatedLine::Expander { gap_id, direction } => Line::from(vec![
            Span::styled(
                cursor_indicator_spaced(row, app.diff_state.cursor_line),
                styles::current_line_indicator_style(&app.theme),
            ),
            Span::styled(
                super::diff_view::expander_body_text(*direction, gap_remaining(app, gap_id)),
                styles::dim_style(&app.theme),
            ),
        ]),
        AnnotatedLine::HiddenLines { count, .. } => Line::from(vec![
            Span::styled(
                cursor_indicator_spaced(row, app.diff_state.cursor_line),
                styles::current_line_indicator_style(&app.theme),
            ),
            Span::styled(
                super::diff_view::hidden_lines_body_text(*count),
                styles::dim_style(&app.theme),
            ),
        ]),
        AnnotatedLine::ExpandedContext { gap_id, line_idx } => {
            expanded_line(app, gap_id, *line_idx)
                .map(|line| {
                    let lw = app.lineno_width();
                    let line_num = if app.relative_line_numbers {
                        super::diff_view::relative_line_number_field(
                            line.new_lineno,
                            row,
                            app.diff_state.cursor_line,
                            lw,
                        )
                    } else {
                        super::diff_view::expanded_context_lineno_field(line, lw)
                    };
                    let style = styles::expanded_context_style(&app.theme);
                    let mut spans = vec![
                        Span::styled(
                            cursor_indicator(row, app.diff_state.cursor_line),
                            styles::current_line_indicator_style(&app.theme),
                        ),
                        Span::styled(line_num, style),
                        Span::styled("  ", style),
                        Span::styled(line.content.clone(), style),
                    ];
                    if let Some(needle) = app.search_paint_at(row) {
                        let content = spans.split_off(3);
                        spans.extend(crate::ui::text_utils::apply_search_highlight_spans(
                            content,
                            needle,
                            styles::search_match_style(&app.theme),
                        ));
                    }
                    Line::from(spans)
                })
                .unwrap_or_default()
        }
        AnnotatedLine::HunkHeader { file_idx, hunk_idx } => app
            .diff_files
            .get(*file_idx)
            .and_then(|file| file.hunks.get(*hunk_idx))
            .map(|hunk| {
                let (text, style) = super::diff_view::hunk_header_text_and_style(
                    &app.theme,
                    hunk,
                    app.is_hunk_reviewed(*file_idx, *hunk_idx),
                );
                Line::from(vec![
                    Span::styled(
                        cursor_indicator_spaced(row, app.diff_state.cursor_line),
                        styles::current_line_indicator_style(&app.theme),
                    ),
                    Span::styled(text, style),
                ])
            })
            .unwrap_or_default(),
        AnnotatedLine::DiffLine {
            file_idx,
            hunk_idx,
            line_idx,
            ..
        } => app
            .diff_files
            .get(*file_idx)
            .and_then(|file| file.hunks.get(*hunk_idx))
            .and_then(|hunk| hunk.lines.get(*line_idx))
            .map(|line| unified_diff_line(app, line, row, *file_idx))
            .unwrap_or_default(),
        AnnotatedLine::BinaryOrEmpty { file_idx } => app
            .diff_files
            .get(*file_idx)
            .map(|file| {
                Line::from(vec![
                    Span::styled(
                        cursor_indicator_spaced(row, app.diff_state.cursor_line),
                        styles::current_line_indicator_style(&app.theme),
                    ),
                    Span::styled(
                        super::diff_view::binary_or_empty_label(file),
                        styles::dim_style(&app.theme),
                    ),
                ])
            })
            .unwrap_or_default(),
        AnnotatedLine::Spacing => {
            let indicator = Span::styled(
                cursor_indicator(row, app.diff_state.cursor_line),
                styles::current_line_indicator_style(&app.theme),
            );
            if mode == DiffViewMode::Unified
                && app.is_single_file_view
                && let Some(next) = app
                    .diff_files
                    .get(app.diff_state.current_file_idx + 1)
                    .map(|file| file.display_path().display().to_string())
            {
                return Line::from(vec![
                    indicator,
                    Span::styled(
                        spacing_next_file_hint_text(&next),
                        Style::default()
                            .fg(app.theme.fg_secondary)
                            .add_modifier(Modifier::DIM),
                    ),
                ]);
            }
            Line::from(indicator)
        }
        _ => Line::default(),
    }
}

#[allow(clippy::too_many_arguments)]
fn annotation_line(
    app: &App,
    annotation_idx: usize,
    virtual_row: usize,
    width: usize,
    mode: DiffViewMode,
    sbs_content_width: usize,
    sbs_meta: &mut HashMap<usize, super::diff_side_by_side::SbsRowMeta>,
    blocks: &mut HashMap<usize, Vec<Line<'static>>>,
) -> Line<'static> {
    let Some(annotation) = app.line_annotations.get(annotation_idx) else {
        return Line::default();
    };
    if matches!(
        annotation,
        AnnotatedLine::PrInfoLine { .. }
            | AnnotatedLine::IssueComment { .. }
            | AnnotatedLine::ReviewComment { .. }
            | AnnotatedLine::RemoteReviewSummaryLine { .. }
            | AnnotatedLine::FileComment { .. }
            | AnnotatedLine::LineComment { .. }
            | AnnotatedLine::RemoteThreadLine { .. }
    ) {
        let start = block_start(&app.line_annotations, annotation_idx);
        let lines = blocks
            .entry(start)
            .or_insert_with(|| comment_block_lines(app, annotation, width));
        return lines
            .get(annotation_idx - start)
            .cloned()
            .map(|line| add_indicator(line, virtual_row, app))
            .unwrap_or_default();
    }
    if mode == DiffViewMode::SideBySide
        && let AnnotatedLine::ExpandedContext { gap_id, line_idx } = annotation
        && let Some(line) = expanded_line(app, gap_id, *line_idx)
    {
        let (line, meta) = super::diff_side_by_side::viewport_side_by_side_expanded_line(
            app,
            line,
            virtual_row,
            sbs_content_width,
            app.lineno_width(),
        );
        sbs_meta.insert(virtual_row, meta);
        return line;
    }
    if mode == DiffViewMode::SideBySide
        && let AnnotatedLine::SideBySideLine {
            file_idx,
            hunk_idx,
            del_line_idx,
            add_line_idx,
            ..
        } = annotation
    {
        let (line, meta) = super::diff_side_by_side::viewport_side_by_side_line(
            app,
            *file_idx,
            *hunk_idx,
            *del_line_idx,
            *add_line_idx,
            virtual_row,
            sbs_content_width,
            app.lineno_width(),
        );
        if let Some(meta) = meta {
            sbs_meta.insert(virtual_row, meta);
        }
        return line;
    }
    simple_annotation_line(app, annotation, virtual_row, mode)
}

fn visible_comment_bars(
    app: &App,
    input: Option<&CommentInput>,
    start: usize,
    end: usize,
) -> Vec<CommentBarAnchor> {
    let mut bars = Vec::new();
    let mut seen = HashSet::new();
    for virtual_row in start.saturating_sub(1)..end {
        let Some(DocumentRow::Annotation(annotation_idx)) = document_row(input, virtual_row) else {
            continue;
        };
        let Some(annotation) = app.line_annotations.get(annotation_idx) else {
            continue;
        };
        let block = block_start(&app.line_annotations, annotation_idx);
        if !seen.insert(block) {
            continue;
        }
        match annotation {
            AnnotatedLine::LineComment { line, .. } => {
                let virtual_start = if let Some(input) = input {
                    if block >= input.start + input.replaced {
                        block + input.lines.len() - input.replaced
                    } else {
                        block
                    }
                } else {
                    block
                };
                push_comment_bar(&mut bars, virtual_start, Some(LineRange::single(*line)));
            }
            AnnotatedLine::RemoteThreadLine { thread_idx } => {
                if let Some(thread) = app.forge_review_threads.get(*thread_idx)
                    && let Some(line) = thread.line
                {
                    let virtual_start = if let Some(input) = input {
                        if block >= input.start + input.replaced {
                            block + input.lines.len() - input.replaced
                        } else {
                            block
                        }
                    } else {
                        block
                    };
                    push_comment_bar(&mut bars, virtual_start, Some(LineRange::single(line)));
                }
            }
            _ => {}
        }
    }
    if let Some(input) = input
        && !app.comment_is_file_level
        && !app.comment_is_review_level
    {
        push_comment_bar(
            &mut bars,
            input.start,
            app.comment_line_range
                .map(|(range, _)| range)
                .or_else(|| app.comment_line.map(|(line, _)| LineRange::single(line))),
        );
    }
    bars
}

pub(super) fn render_unified_diff(frame: &mut Frame, app: &mut App, area: Rect) {
    render_diff(frame, app, area, DiffViewMode::Unified);
}

pub(super) fn render_side_by_side_diff(frame: &mut Frame, app: &mut App, area: Rect) {
    render_diff(frame, app, area, DiffViewMode::SideBySide);
}

fn render_diff(frame: &mut Frame, app: &mut App, area: Rect, mode: DiffViewMode) {
    let focused = app.focused_panel == FocusedPanel::Diff;
    let block = Block::default()
        .title(super::diff_view::diff_title(app, area.width))
        .title_top(diff_stat_title(app).right_aligned())
        .borders(Borders::ALL)
        .style(styles::panel_style(&app.theme))
        .border_style(styles::border_style(&app.theme, focused));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    app.diff_state.viewport_height = inner.height as usize;
    app.diff_inner_area = Some(inner);

    let mut input = build_comment_input(app, inner.width as usize);
    // Legacy callers and a few tests enter Comment mode by assigning the mode
    // directly. They have no editor anchor, so there is no virtual insertion.
    if input
        .as_ref()
        .is_some_and(|editor| editor.start > app.line_annotations.len())
    {
        input = None;
    }
    let total_rows = input.as_ref().map_or(app.line_annotations.len(), |editor| {
        editor.total_rows(app.line_annotations.len())
    });
    if let Some(editor) = &input {
        app.comment_input_annotation_offset =
            Some((editor.start, editor.lines.len(), editor.replaced));
        super::diff_view::scroll_comment_input_into_view(
            &mut app.diff_state.scroll_offset,
            Some((editor.start, editor.end().saturating_sub(1))),
            Some(editor.start + editor.cursor_line_offset),
            inner.height as usize,
            total_rows,
        );
    } else {
        app.comment_input_annotation_offset = None;
    }

    let start = app.diff_state.scroll_offset.min(total_rows);
    let end = start.saturating_add(inner.height as usize).min(total_rows);
    let lw = app.lineno_width();
    let sbs_content_width = inner.width.saturating_sub(crate::app::sbs_overhead(lw)) as usize / 2;
    let mut blocks = HashMap::new();
    let mut sbs_meta = HashMap::new();
    let mut lines = Vec::with_capacity(end - start);
    for virtual_row in start..end {
        let line = match document_row(input.as_ref(), virtual_row) {
            Some(DocumentRow::CommentInput(offset)) => input
                .as_ref()
                .and_then(|editor| editor.lines.get(offset))
                .cloned()
                .map(|line| add_indicator(line, virtual_row, app))
                .unwrap_or_default(),
            Some(DocumentRow::Annotation(annotation_idx)) => annotation_line(
                app,
                annotation_idx,
                virtual_row,
                inner.width as usize,
                mode,
                sbs_content_width,
                &mut sbs_meta,
                &mut blocks,
            ),
            None => Line::default(),
        };
        lines.push(line);
    }

    let line_widths: Vec<usize> = lines
        .iter()
        .map(|line| line.spans.iter().map(|span| span.content.width()).sum())
        .collect();
    let max_content_width = line_widths.iter().copied().max().unwrap_or(0);
    app.sync_viewport_width(inner.width as usize);
    app.diff_state.max_content_width = max_content_width;

    let unscrolled = lines.clone();
    let viewport_width = inner.width as usize;
    let (row_heights, wrapped): (Vec<usize>, Option<Vec<Line>>) =
        if app.diff_state.wrap_lines && viewport_width > 0 {
            let mut heights = Vec::with_capacity(unscrolled.len());
            let mut output = Vec::new();
            let blank_prefixes = super::diff_side_by_side::sbs_blank_prefixes(&app.theme, lw);
            for (offset, line) in unscrolled.iter().enumerate() {
                let logical_row = start + offset;
                if mode == DiffViewMode::SideBySide
                    && sbs_content_width > 0
                    && let Some(meta) = sbs_meta.get(&logical_row)
                {
                    let left_rows = if meta.left_content.is_empty() {
                        vec![Vec::new()]
                    } else {
                        crate::ui::text_utils::wrap_spans(&meta.left_content, sbs_content_width)
                    };
                    let right_rows = if meta.right_content.is_empty() {
                        vec![Vec::new()]
                    } else {
                        crate::ui::text_utils::wrap_spans(&meta.right_content, sbs_content_width)
                    };
                    let count = left_rows.len().max(right_rows.len()).max(1);
                    heights.push(count);
                    for row in 0..count {
                        let left = super::diff_side_by_side::pad_spans_to_width(
                            left_rows.get(row).cloned().unwrap_or_default(),
                            sbs_content_width,
                            meta.left_pad_style,
                        );
                        let right = super::diff_side_by_side::pad_spans_to_width(
                            right_rows.get(row).cloned().unwrap_or_default(),
                            sbs_content_width,
                            meta.right_pad_style,
                        );
                        let (mut left_prefix, right_prefix) = if row == 0 {
                            (meta.left_prefix.clone(), meta.right_prefix.clone())
                        } else {
                            blank_prefixes.clone()
                        };
                        left_prefix.extend(left);
                        left_prefix.extend(right_prefix);
                        left_prefix.extend(right);
                        output.push(Line::from(left_prefix));
                    }
                } else {
                    let rows = crate::ui::text_utils::wrap_spans(&line.spans, viewport_width);
                    heights.push(rows.len());
                    output.extend(rows.into_iter().map(Line::from));
                }
            }
            (heights, Some(output))
        } else {
            (vec![1; unscrolled.len()], None)
        };
    app.diff_state.visible_line_count = populate_row_to_annotation(
        &mut app.diff_row_to_annotation,
        &row_heights,
        viewport_width,
        inner.height as usize,
        app.diff_state.wrap_lines,
        start,
    );
    if let Some(editor) = &input {
        let cursor_row = editor.start + editor.cursor_line_offset;
        if (start..end).contains(&cursor_row) {
            let logical_offset = cursor_row - start;
            let visual_offset = row_heights
                .iter()
                .take(logical_offset)
                .copied()
                .sum::<usize>();
            app.comment_cursor_screen_pos = Some((
                inner.x + editor.cursor_column,
                inner.y + visual_offset as u16,
            ));
        }
    }

    let max_scroll_x = max_content_width.saturating_sub(viewport_width);
    app.diff_state.scroll_x = app.diff_state.scroll_x.min(max_scroll_x);
    if app.diff_state.wrap_lines {
        app.diff_state.scroll_x = 0;
    }
    let visible = wrapped.unwrap_or_else(|| {
        unscrolled
            .iter()
            .cloned()
            .map(|line| apply_horizontal_scroll(line, app.diff_state.scroll_x))
            .collect()
    });
    let comment_bars = visible_comment_bars(app, input.as_ref(), start, end);

    if mode == DiffViewMode::Unified {
        paint_unified_diff_rows_with(frame, inner, &unscrolled, &row_heights, |_idx, line| {
            unified_line_bg_style(line, &app.theme)
        });
    }
    let overlay = DiffOverlayPaint {
        inner,
        visible_lines_unscrolled: &unscrolled,
        line_widths: &line_widths,
        row_heights: &row_heights,
        wrap_lines: app.diff_state.wrap_lines,
        viewport_width,
        scroll_x: app.diff_state.scroll_x,
        scroll_offset: start,
        theme: &app.theme,
        comment_bars: &comment_bars,
    };
    paint_section_highlight(frame, &overlay);
    let paragraph_style = if mode == DiffViewMode::SideBySide {
        styles::panel_style(&app.theme)
    } else {
        Style::default().fg(app.theme.fg_primary)
    };
    frame.render_widget(Paragraph::new(visible).style(paragraph_style), inner);
    paint_cursor_line_highlight(frame, inner, &unscrolled, &row_heights, app);
    if let Some(selection) = app.visual_selection {
        paint_visual_selection_overlay(frame, inner, app, selection, &app.theme);
    }
    paint_file_header_fill(frame, &overlay);
    paint_comment_box_bar(frame, &overlay);
    paint_comment_box_right_border(frame, &overlay);
}
