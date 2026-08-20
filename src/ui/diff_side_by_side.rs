use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
};
use unicode_width::UnicodeWidthStr;

use crate::app::App;
use crate::model::{DiffLine, LineOrigin};
use crate::theme::Theme;
use crate::ui::diff_view::cursor_indicator;
use crate::ui::styles;
use crate::ui::text_utils::{
    apply_search_highlight_pairs, apply_search_highlight_spans, apply_search_highlight_text,
    truncate_or_pad, truncate_or_pad_pairs_by_chars, truncate_or_pad_spans,
};

#[derive(Clone, Default)]
pub(super) struct SbsRowMeta {
    pub(super) left_content: Vec<Span<'static>>,
    pub(super) right_content: Vec<Span<'static>>,
    pub(super) left_prefix: Vec<Span<'static>>,
    pub(super) right_prefix: Vec<Span<'static>>,
    pub(super) left_pad_style: Style,
    pub(super) right_pad_style: Style,
}

fn content_spans_for_diff_line(
    theme: &Theme,
    dl: &DiffLine,
    origin: LineOrigin,
    search: Option<(&str, Style)>,
) -> Vec<Span<'static>> {
    let base = match origin {
        LineOrigin::Context => styles::diff_context_style(theme),
        LineOrigin::Addition => styles::diff_add_style(theme),
        LineOrigin::Deletion => styles::diff_del_style(theme),
    };
    let spans: Vec<Span<'static>> = if let Some(ref h) = dl.highlighted_spans {
        h.iter().map(|(s, t)| Span::styled(t.clone(), *s)).collect()
    } else {
        vec![Span::styled(dl.content.clone(), base)]
    };
    match search {
        Some((needle, hl)) => apply_search_highlight_spans(spans, needle, hl),
        None => spans,
    }
}

fn searched_cell_spans(
    pairs: &[(Style, String)],
    width: usize,
    pad_style: Style,
    search: Option<(&str, Style)>,
) -> Vec<Span<'static>> {
    if let Some((needle, hl)) = search
        && let Some(highlighted) = apply_search_highlight_pairs(pairs, needle, hl)
    {
        return truncate_or_pad_spans(&highlighted, width, pad_style);
    }
    truncate_or_pad_spans(pairs, width, pad_style)
}

fn plain_cell_spans(
    content: &str,
    style: Style,
    width: usize,
    search: Option<(&str, Style)>,
) -> Vec<Span<'static>> {
    if let Some((needle, hl)) = search
        && let Some(highlighted) = apply_search_highlight_text(content, style, needle, hl)
    {
        return truncate_or_pad_pairs_by_chars(&highlighted, width, style);
    }
    vec![Span::styled(truncate_or_pad(content, width), style)]
}

fn column_pad_style(theme: &Theme, dl: &DiffLine, origin: LineOrigin) -> Style {
    match origin {
        LineOrigin::Context => styles::diff_context_style(theme),
        LineOrigin::Addition => {
            if dl.highlighted_spans.is_some() {
                Style::default().fg(theme.diff_add).bg(theme.syntax_add_bg)
            } else {
                styles::diff_add_style(theme)
            }
        }
        LineOrigin::Deletion => {
            if dl.highlighted_spans.is_some() {
                Style::default().fg(theme.diff_del).bg(theme.syntax_del_bg)
            } else {
                styles::diff_del_style(theme)
            }
        }
    }
}

pub(super) fn pad_spans_to_width(
    mut spans: Vec<Span<'static>>,
    width: usize,
    pad_style: Style,
) -> Vec<Span<'static>> {
    let cur: usize = spans.iter().map(|s| s.content.width()).sum();
    if cur < width {
        spans.push(Span::styled(" ".repeat(width - cur), pad_style));
    }
    spans
}

struct SideSpec {
    lineno: Option<u32>,
    marker: &'static str,
    marker_style: Style,
}

fn sbs_row_prefixes(
    theme: &Theme,
    indicator: &'static str,
    left: SideSpec,
    right: SideSpec,
    lw: usize,
) -> (Vec<Span<'static>>, Vec<Span<'static>>) {
    let dim = styles::dim_style(theme);
    let old_num = left
        .lineno
        .map(|n| format!("{n:>lw$}"))
        .unwrap_or_else(|| " ".repeat(lw));
    let new_num = right
        .lineno
        .map(|n| format!("{n:>lw$}"))
        .unwrap_or_else(|| " ".repeat(lw));

    let left_prefix = vec![
        Span::styled(indicator, styles::current_line_indicator_style(theme)),
        Span::styled(format!("{old_num} "), dim),
        Span::styled(left.marker.to_string(), left.marker_style),
    ];
    let right_prefix = vec![
        Span::styled(" │ ", dim),
        Span::styled(format!("{new_num} "), dim),
        Span::styled(right.marker.to_string(), right.marker_style),
    ];
    (left_prefix, right_prefix)
}

/// Continuation-row prefixes shared by every wrapped line: blank in place of
/// the line numbers (same width, so columns stay aligned) with the center
/// divider preserved.
pub(super) fn sbs_blank_prefixes(
    theme: &Theme,
    lw: usize,
) -> (Vec<Span<'static>>, Vec<Span<'static>>) {
    let dim = styles::dim_style(theme);
    let left = vec![Span::styled(" ".repeat(lw + 3), Style::default())];
    let right = vec![
        Span::styled(" │ ", dim),
        Span::styled(" ".repeat(lw + 2), Style::default()),
    ];
    (left, right)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn viewport_side_by_side_line(
    app: &App,
    file_idx: usize,
    hunk_idx: usize,
    del_line_idx: Option<usize>,
    add_line_idx: Option<usize>,
    row: usize,
    content_width: usize,
    lw: usize,
) -> (Line<'static>, Option<SbsRowMeta>) {
    let Some(hunk) = app
        .diff_files
        .get(file_idx)
        .and_then(|file| file.hunks.get(hunk_idx))
    else {
        return (Line::default(), None);
    };
    let del = del_line_idx.and_then(|idx| hunk.lines.get(idx));
    let add = add_line_idx.and_then(|idx| hunk.lines.get(idx));
    let display_lineno = |line: Option<u32>| {
        line.map(|line| {
            if app.relative_line_numbers {
                row.abs_diff(app.diff_state.cursor_line) as u32
            } else {
                line
            }
        })
    };
    let indicator = cursor_indicator(row, app.diff_state.cursor_line);
    let search = app
        .search_paint_at(row)
        .map(|needle| (needle, styles::search_match_style(&app.theme)));

    if app.diff_files[file_idx].is_commit_message {
        let Some(line) = add.or(del) else {
            return (Line::default(), None);
        };
        return (
            Line::from(vec![
                Span::styled(indicator, styles::current_line_indicator_style(&app.theme)),
                Span::styled("  ", styles::diff_context_style(&app.theme)),
                Span::styled(line.content.clone(), styles::diff_context_style(&app.theme)),
            ]),
            None,
        );
    }

    let is_context =
        del_line_idx == add_line_idx && del.is_some_and(|line| line.origin == LineOrigin::Context);
    if is_context {
        let line = del.expect("context annotation has a source line");
        let old = display_lineno(line.old_lineno)
            .map(|n| format!("{n:>lw$}"))
            .unwrap_or_else(|| " ".repeat(lw));
        let new = display_lineno(line.new_lineno)
            .map(|n| format!("{n:>lw$}"))
            .unwrap_or_else(|| " ".repeat(lw));
        let style = styles::diff_context_style(&app.theme);
        let cell = if let Some(highlighted) = &line.highlighted_spans {
            searched_cell_spans(highlighted, content_width, style, search)
        } else {
            plain_cell_spans(&line.content, style, content_width, search)
        };
        let mut spans = vec![
            Span::styled(indicator, styles::current_line_indicator_style(&app.theme)),
            Span::styled(format!("{old} "), styles::dim_style(&app.theme)),
            Span::styled(" ", style),
        ];
        spans.extend(cell.clone());
        spans.push(Span::styled(" │ ", styles::dim_style(&app.theme)));
        spans.push(Span::styled(
            format!("{new} "),
            styles::dim_style(&app.theme),
        ));
        spans.push(Span::styled(" ", style));
        spans.extend(cell);
        let content = content_spans_for_diff_line(&app.theme, line, LineOrigin::Context, search);
        let (left_prefix, right_prefix) = sbs_row_prefixes(
            &app.theme,
            indicator,
            SideSpec {
                lineno: display_lineno(line.old_lineno),
                marker: " ",
                marker_style: style,
            },
            SideSpec {
                lineno: display_lineno(line.new_lineno),
                marker: " ",
                marker_style: style,
            },
            lw,
        );
        return (
            Line::from(spans),
            Some(SbsRowMeta {
                left_content: content.clone(),
                right_content: content,
                left_prefix,
                right_prefix,
                left_pad_style: style,
                right_pad_style: style,
            }),
        );
    }

    let mut spans = vec![Span::styled(
        indicator,
        styles::current_line_indicator_style(&app.theme),
    )];
    if let Some(line) = del {
        add_deletion_spans(
            &app.theme,
            &mut spans,
            line,
            content_width,
            lw,
            display_lineno(line.old_lineno),
            search,
        );
    } else {
        add_empty_column_spans(&mut spans, content_width, lw);
    }
    spans.push(Span::styled(" │ ", styles::dim_style(&app.theme)));
    if let Some(line) = add {
        add_addition_spans(
            &app.theme,
            &mut spans,
            line,
            content_width,
            lw,
            display_lineno(line.new_lineno),
            search,
        );
    } else {
        add_empty_column_spans(&mut spans, content_width, lw);
    }

    let (left_content, left_pad, left_marker, left_lineno, left_style) = match del {
        Some(line) => (
            content_spans_for_diff_line(&app.theme, line, LineOrigin::Deletion, search),
            column_pad_style(&app.theme, line, LineOrigin::Deletion),
            "▌",
            display_lineno(line.old_lineno),
            styles::diff_del_style(&app.theme),
        ),
        None => (Vec::new(), Style::default(), " ", None, Style::default()),
    };
    let (right_content, right_pad, right_marker, right_lineno, right_style) = match add {
        Some(line) => (
            content_spans_for_diff_line(&app.theme, line, LineOrigin::Addition, search),
            column_pad_style(&app.theme, line, LineOrigin::Addition),
            "▌",
            display_lineno(line.new_lineno),
            styles::diff_add_style(&app.theme),
        ),
        None => (Vec::new(), Style::default(), " ", None, Style::default()),
    };
    let (left_prefix, right_prefix) = sbs_row_prefixes(
        &app.theme,
        indicator,
        SideSpec {
            lineno: left_lineno,
            marker: left_marker,
            marker_style: left_style,
        },
        SideSpec {
            lineno: right_lineno,
            marker: right_marker,
            marker_style: right_style,
        },
        lw,
    );
    (
        Line::from(spans),
        Some(SbsRowMeta {
            left_content,
            right_content,
            left_prefix,
            right_prefix,
            left_pad_style: left_pad,
            right_pad_style: right_pad,
        }),
    )
}

pub(super) fn viewport_side_by_side_expanded_line(
    app: &App,
    line: &DiffLine,
    row: usize,
    content_width: usize,
    lw: usize,
) -> (Line<'static>, SbsRowMeta) {
    let display_lineno = |source: Option<u32>| {
        source.map(|line| {
            if app.relative_line_numbers {
                row.abs_diff(app.diff_state.cursor_line) as u32
            } else {
                line
            }
        })
    };
    let old = display_lineno(line.old_lineno)
        .map(|n| format!("{n:>lw$} "))
        .unwrap_or_else(|| " ".repeat(lw + 1));
    let new = display_lineno(line.new_lineno)
        .map(|n| format!("{n:>lw$} "))
        .unwrap_or_else(|| " ".repeat(lw + 1));
    let indicator = cursor_indicator(row, app.diff_state.cursor_line);
    let style = styles::expanded_context_style(&app.theme);
    let search = app
        .search_paint_at(row)
        .map(|needle| (needle, styles::search_match_style(&app.theme)));
    let cell = plain_cell_spans(&line.content, style, content_width, search);
    let mut spans = vec![
        Span::styled(indicator, styles::current_line_indicator_style(&app.theme)),
        Span::styled(old.clone(), style),
        Span::styled(" ", style),
    ];
    spans.extend(cell.clone());
    spans.push(Span::styled(" │ ", styles::dim_style(&app.theme)));
    spans.push(Span::styled(new.clone(), style));
    spans.push(Span::styled(" ", style));
    spans.extend(cell);
    let mut content = vec![Span::styled(line.content.clone(), style)];
    if let Some((needle, highlight)) = search {
        content = apply_search_highlight_spans(content, needle, highlight);
    }
    (
        Line::from(spans),
        SbsRowMeta {
            left_content: content.clone(),
            right_content: content,
            left_prefix: vec![
                Span::styled(indicator, styles::current_line_indicator_style(&app.theme)),
                Span::styled(old, style),
                Span::styled(" ", style),
            ],
            right_prefix: vec![
                Span::styled(" │ ", styles::dim_style(&app.theme)),
                Span::styled(new, style),
                Span::styled(" ", style),
            ],
            left_pad_style: style,
            right_pad_style: style,
        },
    )
}

pub(super) fn render_side_by_side_diff(frame: &mut Frame, app: &mut App, area: Rect) {
    crate::ui::diff_viewport::render_side_by_side_diff(frame, app, area);
}

/// Add deletion line spans to the spans vector
fn add_deletion_spans(
    theme: &Theme,
    spans: &mut Vec<Span>,
    diff_line: &crate::model::DiffLine,
    content_width: usize,
    lw: usize,
    display_lineno: Option<u32>,
    search: Option<(&str, Style)>,
) {
    let line_num = display_lineno
        .map(|n| format!("{n:>lw$}"))
        .unwrap_or_else(|| " ".repeat(lw));

    spans.push(Span::styled(
        format!("{line_num} "),
        styles::dim_style(theme),
    ));
    spans.push(Span::styled("▌".to_string(), styles::diff_del_style(theme)));

    // Use syntax highlighting if available
    if let Some(ref highlighted) = diff_line.highlighted_spans {
        let syntax_pad_style = Style::default().fg(theme.diff_del).bg(theme.syntax_del_bg);
        let content_spans =
            searched_cell_spans(highlighted, content_width, syntax_pad_style, search);
        spans.extend(content_spans);
    } else {
        spans.extend(plain_cell_spans(
            &diff_line.content,
            styles::diff_del_style(theme),
            content_width,
            search,
        ));
    }
}

/// Add addition line spans to the spans vector
fn add_addition_spans(
    theme: &Theme,
    spans: &mut Vec<Span>,
    diff_line: &crate::model::DiffLine,
    content_width: usize,
    lw: usize,
    display_lineno: Option<u32>,
    search: Option<(&str, Style)>,
) {
    let line_num = display_lineno
        .map(|n| format!("{n:>lw$}"))
        .unwrap_or_else(|| " ".repeat(lw));

    spans.push(Span::styled(
        format!("{line_num} "),
        styles::dim_style(theme),
    ));
    spans.push(Span::styled("▌".to_string(), styles::diff_add_style(theme)));

    // Use syntax highlighting if available
    if let Some(ref highlighted) = diff_line.highlighted_spans {
        let syntax_pad_style = Style::default().fg(theme.diff_add).bg(theme.syntax_add_bg);
        let content_spans =
            searched_cell_spans(highlighted, content_width, syntax_pad_style, search);
        spans.extend(content_spans);
    } else {
        spans.extend(plain_cell_spans(
            &diff_line.content,
            styles::diff_add_style(theme),
            content_width,
            search,
        ));
    }
}

/// Add empty column spans (for when one side has no content)
fn add_empty_column_spans(spans: &mut Vec<Span>, content_width: usize, lw: usize) {
    // line_num(lw) + space(1) + prefix(1) + content
    spans.push(Span::styled(
        " ".repeat(lw + 1 + 1 + content_width),
        Style::default(),
    ));
}

#[cfg(test)]
mod remote_comments_side_by_side_snapshot_tests {
    //! Render-snapshot tests for inline remote review threads in the
    //! side-by-side diff view. Confirms the badge appears at least once
    //! when a thread is active and is hidden under `:comments hide`.
    use crate::app::{App, DiffSource, DiffViewMode, InputMode, PullRequestDiffSource};
    use crate::error::Result as TuicrResult;
    use crate::error::TuicrError;
    use crate::forge::remote_comments::{
        PrCommentsVisibility, RemoteCommentSide, RemoteReviewComment, RemoteReviewThread,
    };
    use crate::forge::traits::{ForgeRepository, PrSessionKey};
    use crate::model::{
        DiffFile, DiffHunk, DiffLine, FileStatus, LineOrigin, ReviewSession, SessionDiffSource,
    };
    use crate::syntax::SyntaxHighlighter;
    use crate::theme::Theme;
    use crate::ui::render;
    use crate::vcs::traits::{VcsBackend, VcsChangeStatus, VcsInfo, VcsType};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use std::path::{Path, PathBuf};

    struct SnapshotVcs {
        info: VcsInfo,
    }

    impl VcsBackend for SnapshotVcs {
        fn info(&self) -> &VcsInfo {
            &self.info
        }
        fn get_working_tree_diff(
            &self,
            _highlighter: &SyntaxHighlighter,
        ) -> TuicrResult<Vec<DiffFile>> {
            Err(TuicrError::NoChanges)
        }
        fn fetch_context_lines(
            &self,
            _file_path: &Path,
            _file_status: FileStatus,
            _ref_commit: Option<&str>,
            _start_line: u32,
            _end_line: u32,
        ) -> TuicrResult<Vec<DiffLine>> {
            Ok(Vec::new())
        }
        fn get_change_status(&self) -> TuicrResult<VcsChangeStatus> {
            Ok(VcsChangeStatus {
                staged: false,
                unstaged: false,
            })
        }
        fn file_line_count(
            &self,
            _file_path: &Path,
            _file_status: FileStatus,
            _ref_commit: Option<&str>,
        ) -> TuicrResult<u32> {
            Ok(0)
        }
    }

    fn repo() -> ForgeRepository {
        ForgeRepository::github("github.com", "agavra", "tuicr")
    }

    fn sample_diff_file() -> DiffFile {
        let lines = vec![
            DiffLine {
                origin: LineOrigin::Context,
                content: "first".to_string(),
                old_lineno: Some(1),
                new_lineno: Some(1),
                highlighted_spans: None,
            },
            DiffLine {
                origin: LineOrigin::Addition,
                content: "second".to_string(),
                old_lineno: None,
                new_lineno: Some(2),
                highlighted_spans: None,
            },
        ];
        let hunk = DiffHunk {
            header: "@@ -1,1 +1,2 @@".to_string(),
            lines,
            old_start: 1,
            old_count: 1,
            new_start: 1,
            new_count: 2,
        };
        let hunks = vec![hunk];
        let content_hash = DiffFile::compute_content_hash(&hunks);
        DiffFile {
            old_path: Some(PathBuf::from("src/lib.rs")),
            new_path: Some(PathBuf::from("src/lib.rs")),
            status: FileStatus::Modified,
            hunks,
            is_binary: false,
            is_too_large: false,
            is_commit_message: false,
            content_hash,
        }
    }

    fn thread() -> RemoteReviewThread {
        RemoteReviewThread {
            id: "T".to_string(),
            path: "src/lib.rs".to_string(),
            line: Some(2),
            side: RemoteCommentSide::Right,
            is_resolved: false,
            is_outdated: false,
            comments: vec![RemoteReviewComment {
                id: "C".to_string(),
                author: Some("alice".to_string()),
                body: "sbs hello".to_string(),
                created_at: None,
                in_reply_to: None,
                url: "https://example.com".to_string(),
            }],
        }
    }

    fn make_pr_app() -> App {
        make_pr_app_with(vec![sample_diff_file()])
    }

    fn make_pr_app_with(diff_files: Vec<DiffFile>) -> App {
        let pr = PullRequestDiffSource {
            key: PrSessionKey::new(repo(), 125, "headsha".to_string()),
            base_sha: "basesha".to_string(),
            title: "test pr".to_string(),
            url: "https://example.com".to_string(),
            head_ref_name: "feat".to_string(),
            base_ref_name: "main".to_string(),
            state: "OPEN".to_string(),
            closed: false,
            merged: false,
        };
        let vcs_info = VcsInfo {
            root_path: PathBuf::from("forge:github.com/agavra/tuicr"),
            head_commit: "headsha".to_string(),
            branch_name: Some("feat".to_string()),
            vcs_type: VcsType::File,
        };
        let mut session = ReviewSession::new(
            vcs_info.root_path.clone(),
            "headsha".to_string(),
            Some("feat".to_string()),
            SessionDiffSource::PullRequest,
        );
        session.pr_session_key = Some(pr.key.clone());
        let mut app = App::build(
            Box::new(SnapshotVcs {
                info: vcs_info.clone(),
            }),
            vcs_info,
            Theme::dark(),
            None,
            false,
            diff_files,
            session,
            DiffSource::PullRequest(Box::new(pr)),
            InputMode::Normal,
            Vec::new(),
            None,
            None,
        )
        .expect("build app");
        app.diff_view_mode = DiffViewMode::SideBySide;
        app
    }

    fn draw(app: &mut App) -> Buffer {
        let backend = TestBackend::new(160, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render(frame, app))
            .expect("draw frame");
        terminal.backend().buffer().clone()
    }

    fn body_text(buffer: &Buffer) -> String {
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Side-by-side mirror of the unified culling test: the skip/emit wiring
    /// here goes through `ctx.box_visible` and a by-value `line_idx`, so it
    /// needs its own coverage.
    #[test]
    fn should_cull_comment_boxes_outside_the_viewport() {
        use crate::app::AnnotatedLine;
        use crate::model::{Comment, CommentType};

        const NEEDLE: &str = "far-below-the-fold";

        let lines: Vec<DiffLine> = (1..=120)
            .map(|n| DiffLine {
                origin: LineOrigin::Addition,
                content: format!("line {n}"),
                old_lineno: None,
                new_lineno: Some(n),
                highlighted_spans: None,
            })
            .collect();
        let hunks = vec![DiffHunk {
            header: "@@ -0,0 +1,120 @@".to_string(),
            lines,
            old_start: 0,
            old_count: 0,
            new_start: 1,
            new_count: 120,
        }];
        let content_hash = DiffFile::compute_content_hash(&hunks);
        let path = PathBuf::from("src/lib.rs");
        let file = DiffFile {
            old_path: Some(path.clone()),
            new_path: Some(path.clone()),
            status: FileStatus::Modified,
            hunks,
            is_binary: false,
            is_too_large: false,
            is_commit_message: false,
            content_hash,
        };

        let mut app = make_pr_app_with(vec![file]);
        app.session
            .get_file_mut(&path)
            .expect("file registered in session")
            .add_line_comment(
                100,
                Comment::new(NEEDLE.to_string(), CommentType::from_id("note"), None),
            );
        app.rebuild_annotations();

        let body = body_text(&draw(&mut app));
        assert!(
            !body.contains(NEEDLE),
            "off-screen comment should not be visible:\n{body}"
        );

        let comment_row = app
            .line_annotations
            .iter()
            .position(|a| matches!(a, AnnotatedLine::LineComment { .. }))
            .expect("comment annotated in the document");
        app.diff_state.scroll_offset = comment_row;
        app.diff_state.cursor_line = comment_row;

        let body = body_text(&draw(&mut app));
        assert!(
            body.contains(NEEDLE),
            "comment scrolled into view should render at its annotated row:\n{body}"
        );
    }

    #[test]
    fn should_render_remote_comment_inline_in_side_by_side_diff() {
        // given
        let mut app = make_pr_app();
        app.forge_review_threads = vec![thread()];
        app.rebuild_annotations();
        // when
        let buffer = draw(&mut app);
        // then
        let body = body_text(&buffer);
        assert!(
            body.contains("[github @alice]"),
            "expected badge in side-by-side render:\n{body}"
        );
    }

    #[test]
    fn should_hide_remote_comments_under_comments_hide_in_side_by_side() {
        // given
        let mut app = make_pr_app();
        app.forge_review_threads = vec![thread()];
        app.set_remote_comments_visibility(PrCommentsVisibility::Hide);
        // when
        let buffer = draw(&mut app);
        // then
        let body = body_text(&buffer);
        assert!(
            !body.contains("[github @alice"),
            "remote comment leaked under Hide:\n{body}"
        );
    }

    fn diff_file_with_pair(left: &str, right: &str) -> DiffFile {
        let lines = vec![
            DiffLine {
                origin: LineOrigin::Deletion,
                content: left.to_string(),
                old_lineno: Some(1),
                new_lineno: None,
                highlighted_spans: None,
            },
            DiffLine {
                origin: LineOrigin::Addition,
                content: right.to_string(),
                old_lineno: None,
                new_lineno: Some(1),
                highlighted_spans: None,
            },
        ];
        let hunks = vec![DiffHunk {
            header: "@@ -1,1 +1,1 @@".to_string(),
            lines,
            old_start: 1,
            old_count: 1,
            new_start: 1,
            new_count: 1,
        }];
        let content_hash = DiffFile::compute_content_hash(&hunks);
        DiffFile {
            old_path: Some(PathBuf::from("src/lib.rs")),
            new_path: Some(PathBuf::from("src/lib.rs")),
            status: FileStatus::Modified,
            hunks,
            is_binary: false,
            is_too_large: false,
            is_commit_message: false,
            content_hash,
        }
    }

    fn diff_file_with_standalone_deletion(left: &str) -> DiffFile {
        let lines = vec![DiffLine {
            origin: LineOrigin::Deletion,
            content: left.to_string(),
            old_lineno: Some(1),
            new_lineno: None,
            highlighted_spans: None,
        }];
        let hunks = vec![DiffHunk {
            header: "@@ -1,1 +0,0 @@".to_string(),
            lines,
            old_start: 1,
            old_count: 1,
            new_start: 0,
            new_count: 0,
        }];
        let content_hash = DiffFile::compute_content_hash(&hunks);
        DiffFile {
            old_path: Some(PathBuf::from("src/lib.rs")),
            new_path: Some(PathBuf::from("src/lib.rs")),
            status: FileStatus::Modified,
            hunks,
            is_binary: false,
            is_too_large: false,
            is_commit_message: false,
            content_hash,
        }
    }

    fn draw_sbs(app: &mut App, w: u16, h: u16) -> Buffer {
        let backend = TestBackend::new(w, h);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| super::render_side_by_side_diff(frame, app, Rect::new(0, 0, w, h)))
            .expect("draw sbs");
        terminal.backend().buffer().clone()
    }

    fn char_at(buf: &Buffer, x: u16, y: u16) -> String {
        buf[(x, y)].symbol().to_string()
    }

    #[test]
    fn should_wrap_long_line_in_side_by_side_view_when_wrap_enabled() {
        let long_left = "L".repeat(200);
        let mut app = make_pr_app();
        app.diff_files = vec![diff_file_with_pair(&long_left, "short")];
        app.set_diff_wrap(true);
        app.rebuild_annotations();

        let buf = draw_sbs(&mut app, 160, 20);

        let mut rows_with_l = 0u16;
        for y in 0..buf.area.height {
            let row: String = (0..buf.area.width).map(|x| char_at(&buf, x, y)).collect();
            if row.contains("LLLLLLLLLL") {
                rows_with_l += 1;
            }
        }
        assert!(
            rows_with_l >= 2,
            "expected long left content to span >=2 visual rows, got {rows_with_l}"
        );
    }

    #[test]
    fn should_not_wrap_when_wrap_disabled_in_side_by_side() {
        let long_left = "L".repeat(200);
        let mut app = make_pr_app();
        app.diff_files = vec![diff_file_with_pair(&long_left, "short")];
        app.set_diff_wrap(false);
        app.rebuild_annotations();

        let buf = draw_sbs(&mut app, 160, 20);

        let rows_with_l: u16 = (0..buf.area.height)
            .filter(|&y| {
                (0..buf.area.width)
                    .map(|x| char_at(&buf, x, y))
                    .collect::<String>()
                    .contains("LLLLLLLLLL")
            })
            .count() as u16;
        assert_eq!(
            rows_with_l, 1,
            "wrap-off should produce exactly one row of L, got {rows_with_l}"
        );
    }

    #[test]
    fn should_align_divider_on_wrapped_rows_in_side_by_side() {
        let long_left = "L".repeat(200);
        let mut app = make_pr_app();
        app.diff_files = vec![diff_file_with_pair(&long_left, "short")];
        app.set_diff_wrap(true);
        app.rebuild_annotations();

        let lw = 1usize;
        let inner_w = 158usize;
        let content_width = (inner_w - crate::app::sbs_overhead(lw) as usize) / 2;
        let divider_x_inner = crate::app::sbs_left_gutter(lw) as usize + content_width;
        let divider_glyph_x = 1 + divider_x_inner + 1;

        let buf = draw_sbs(&mut app, 160, 20);

        let mut rows_with_l: Vec<u16> = Vec::new();
        for y in 0..buf.area.height {
            let row: String = (0..buf.area.width).map(|x| char_at(&buf, x, y)).collect();
            if row.contains("LLLLLLLLLL") {
                rows_with_l.push(y);
            }
        }
        assert!(
            rows_with_l.len() >= 2,
            "expected ≥2 wrapped rows, got {}",
            rows_with_l.len()
        );
        for y in &rows_with_l {
            let glyph = char_at(&buf, divider_glyph_x as u16, *y);
            assert_eq!(
                glyph, "│",
                "expected │ at col {divider_glyph_x} on row {y}, got {glyph:?}"
            );
        }
    }

    #[test]
    fn should_pad_shorter_column_on_wrapped_rows_in_side_by_side() {
        let long_left = "L".repeat(200);
        let mut app = make_pr_app();
        app.diff_files = vec![diff_file_with_standalone_deletion(&long_left)];
        app.set_diff_wrap(true);
        app.rebuild_annotations();

        let buf = draw_sbs(&mut app, 160, 20);

        let lw = 1usize;
        let inner_w = 158usize;
        let content_width = (inner_w - crate::app::sbs_overhead(lw) as usize) / 2;
        let divider_glyph_x = 1 + crate::app::sbs_left_gutter(lw) as usize + content_width + 1;
        let right_content_start = divider_glyph_x + 2 + lw + 1 + 1;
        let right_content_end = right_content_start + content_width;

        let mut checked = 0;
        for y in 0..buf.area.height {
            let row: String = (0..buf.area.width).map(|x| char_at(&buf, x, y)).collect();
            if !row.contains("LLLLLLLLLL") {
                continue;
            }
            checked += 1;
            let right: String = (right_content_start..right_content_end)
                .map(|x| char_at(&buf, x as u16, y))
                .collect();
            assert!(
                right.trim().is_empty(),
                "right column should be blank on wrapped L row {y}, got {right:?}"
            );
        }
        assert!(
            checked >= 2,
            "expected ≥2 wrapped rows to check, got {checked}"
        );
    }

    fn commit_message_file(message: &str) -> DiffFile {
        let lines: Vec<DiffLine> = message
            .lines()
            .enumerate()
            .map(|(i, line)| DiffLine {
                origin: LineOrigin::Context,
                content: line.to_string(),
                old_lineno: None,
                new_lineno: Some(i as u32 + 1),
                highlighted_spans: None,
            })
            .collect();
        let new_count = lines.len() as u32;
        let hunks = vec![DiffHunk {
            header: String::new(),
            lines,
            old_start: 0,
            old_count: 0,
            new_start: 1,
            new_count,
        }];
        let content_hash = DiffFile::compute_content_hash(&hunks);
        DiffFile {
            old_path: None,
            new_path: Some(PathBuf::from("Commit Message (abc1234)")),
            status: FileStatus::Added,
            hunks,
            is_binary: false,
            is_too_large: false,
            is_commit_message: true,
            content_hash,
        }
    }

    #[test]
    fn should_render_commit_message_full_width_in_side_by_side() {
        let mut app = make_pr_app();
        app.diff_files = vec![commit_message_file("COMMITMSG summary line")];
        app.rebuild_annotations();

        let buf = draw_sbs(&mut app, 160, 20);

        let mut checked = 0;
        for y in 0..buf.area.height {
            let row: String = (0..buf.area.width).map(|x| char_at(&buf, x, y)).collect();
            let Some(col) = row.find("COMMITMSG") else {
                continue;
            };
            checked += 1;
            // Full-width prose: rendered near the left edge (small indent), not
            // pushed into the right diff column, and with no column divider.
            assert!(
                col < 8,
                "commit message should start near the left edge, got col {col} on row {y}: {row:?}"
            );
            assert!(
                !row.contains(" │ "),
                "commit message row should not have a column divider on row {y}: {row:?}"
            );
        }
        assert_eq!(
            checked, 1,
            "expected the commit message body to render exactly once, got {checked}"
        );
    }
}
