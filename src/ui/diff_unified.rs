use ratatui::{Frame, layout::Rect};

use crate::app::App;

pub(super) fn render_unified_diff(frame: &mut Frame, app: &mut App, area: Rect) {
    crate::ui::diff_viewport::render_unified_diff(frame, app, area);
}

#[cfg(test)]
mod remote_comments_snapshot_tests {
    //! Render-snapshot tests for inline remote review threads in the
    //! unified diff. We drive `ui::render` against `TestBackend` and check
    //! for the provider badge text on the expected row.
    use crate::app::{App, DiffSource, InputMode, PullRequestDiffSource};
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
        // Two-line file with one context line and one addition so we have
        // a stable `line=2` anchor for the test thread.
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

    fn header_only_diff_file_at(path: &str) -> DiffFile {
        let hunks = Vec::new();
        let content_hash = DiffFile::compute_content_hash(&hunks);
        DiffFile {
            old_path: Some(PathBuf::from(path)),
            new_path: Some(PathBuf::from(path)),
            status: FileStatus::Modified,
            hunks,
            is_binary: false,
            is_too_large: false,
            is_commit_message: false,
            content_hash,
        }
    }

    fn thread(
        id: &str,
        author: &str,
        body: &str,
        line: u32,
        resolved: bool,
        outdated: bool,
    ) -> RemoteReviewThread {
        RemoteReviewThread {
            id: id.to_string(),
            path: "src/lib.rs".to_string(),
            line: Some(line),
            side: RemoteCommentSide::Right,
            is_resolved: resolved,
            is_outdated: outdated,
            comments: vec![RemoteReviewComment {
                id: format!("{id}-root"),
                author: Some(author.to_string()),
                body: body.to_string(),
                created_at: None,
                in_reply_to: None,
                url: "https://example.com/x".to_string(),
            }],
        }
    }

    fn make_pr_app() -> App {
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
        App::build(
            Box::new(SnapshotVcs {
                info: vcs_info.clone(),
            }),
            vcs_info,
            Theme::dark(),
            None,
            false,
            vec![sample_diff_file()],
            session,
            DiffSource::PullRequest(Box::new(pr)),
            InputMode::Normal,
            Vec::new(),
            None,
            None,
        )
        .expect("build app")
    }

    fn make_revision_app(diff_files: Vec<DiffFile>) -> App {
        let vcs_info = VcsInfo {
            root_path: PathBuf::from("/tmp/tuicr"),
            head_commit: "headsha".to_string(),
            branch_name: None,
            vcs_type: VcsType::Git,
        };
        let session = ReviewSession::new(
            vcs_info.root_path.clone(),
            "headsha".to_string(),
            None,
            SessionDiffSource::CommitRange,
        );
        App::build(
            Box::new(SnapshotVcs {
                info: vcs_info.clone(),
            }),
            vcs_info,
            Theme::dark(),
            None,
            false,
            diff_files,
            session,
            DiffSource::CommitRange(vec!["HEAD".to_string()]),
            InputMode::Normal,
            Vec::new(),
            None,
            None,
        )
        .expect("build app")
    }

    fn draw(app: &mut App) -> Buffer {
        let backend = TestBackend::new(140, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render(frame, app))
            .expect("draw frame");
        terminal.backend().buffer().clone()
    }

    fn draw_unified_diff(app: &mut App) -> Buffer {
        let backend = TestBackend::new(100, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| super::render_unified_diff(frame, app, Rect::new(0, 0, 100, 12)))
            .expect("draw unified diff");
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
    fn should_render_commit_message_without_line_numbers_in_unified() {
        let mut app = make_revision_app(vec![commit_message_file(
            "COMMITMSG summary\n\nsecond body line",
        )]);
        let buf = draw_unified_diff(&mut app);

        let mut checked = 0;
        for y in 0..buf.area.height {
            let cells: Vec<String> = (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect();
            let row: String = cells.concat();
            let Some(byte_col) = row
                .find("COMMITMSG")
                .or_else(|| row.find("second body line"))
            else {
                continue;
            };
            checked += 1;
            // No line-number gutter: nothing but whitespace precedes the text.
            let col = row[..byte_col].chars().count();
            let gutter: String = cells[..col].concat();
            assert!(
                gutter.chars().all(|c| c.is_whitespace() || c == '│'),
                "commit message row {y} should have no line number, got gutter {gutter:?}"
            );
        }
        assert_eq!(
            checked, 2,
            "expected both message body lines to render, got {checked}"
        );
    }

    #[test]
    fn should_render_unresolved_remote_comment_inline_in_unified_diff() {
        // given a PR app with one unresolved remote thread anchored on
        // the addition line
        let mut app = make_pr_app();
        app.forge_review_threads = vec![thread("t1", "alice", "looks good?", 2, false, false)];
        app.rebuild_annotations();
        // when
        let buffer = draw(&mut app);
        // then — the badge appears somewhere in the rendered frame
        let body = body_text(&buffer);
        assert!(
            body.contains("[github @alice]"),
            "expected [github @alice] badge in:\n{body}"
        );
        assert!(
            body.contains("looks good?"),
            "expected remote comment body in:\n{body}"
        );
    }

    // Revision diffs with `wrap = true` render the file-header rule without a
    // cursor gutter. The right-edge fill overlay must measure that exact row:
    // treating it like guttered diff content truncated `README.md [M]` to
    // `README` in `tuicr -r HEAD`.
    #[test]
    fn should_render_full_file_header_for_revision_diff() {
        let mut app = make_revision_app(vec![header_only_diff_file_at("README.md")]);
        app.diff_state.wrap_lines = true;

        let body = body_text(&draw_unified_diff(&mut app));

        assert!(
            body.contains("═══ README.md [M] "),
            "expected full README.md file header in:\n{body}"
        );
    }

    #[test]
    fn should_render_resolved_remote_comment_only_under_comments_all() {
        // given a PR app with one resolved remote thread
        let mut app = make_pr_app();
        app.forge_review_threads = vec![thread(
            "t1", "alice", "old note", 2, /* resolved */ true, false,
        )];
        // default Unresolved visibility — should not render
        app.rebuild_annotations();
        let before = body_text(&draw(&mut app));
        assert!(
            !before.contains("[github @alice"),
            "resolved thread leaked under Unresolved:\n{before}"
        );

        // when — flip to All
        assert!(app.set_remote_comments_visibility(PrCommentsVisibility::All));
        // then — the resolved badge appears with the "resolved" marker
        let after = body_text(&draw(&mut app));
        assert!(
            after.contains("[github @alice resolved]"),
            "expected resolved badge in:\n{after}"
        );
    }

    #[test]
    fn should_hide_all_remote_comments_when_comments_hide() {
        // given
        let mut app = make_pr_app();
        app.forge_review_threads = vec![thread("t1", "alice", "blocker", 2, false, false)];
        app.rebuild_annotations();
        // sanity: visible by default
        let before = body_text(&draw(&mut app));
        assert!(before.contains("[github @alice]"));

        // when
        assert!(app.set_remote_comments_visibility(PrCommentsVisibility::Hide));
        // then
        let after = body_text(&draw(&mut app));
        assert!(
            !after.contains("[github @alice"),
            "comment leaked under Hide:\n{after}"
        );
    }

    #[test]
    fn should_render_outdated_marker_for_outdated_thread_under_all() {
        // given
        let mut app = make_pr_app();
        app.forge_review_threads = vec![thread(
            "t1",
            "bob",
            "stale anchor",
            2,
            false,
            /* outdated */ true,
        )];
        // when — switch to all so the outdated thread is visible
        app.set_remote_comments_visibility(PrCommentsVisibility::All);
        let body = body_text(&draw(&mut app));
        // then
        assert!(
            body.contains("[github @bob outdated]"),
            "expected outdated badge in:\n{body}"
        );
    }

    #[test]
    fn should_render_review_level_remote_thread_in_review_comments_section() {
        // given — a review-level thread (line: None, path: "") as produced by
        // GitLab individual_note: true discussions
        let mut app = make_pr_app();
        app.forge_review_threads = vec![RemoteReviewThread {
            id: "rv1".to_string(),
            path: String::new(),
            line: None,
            side: RemoteCommentSide::Right,
            is_resolved: false,
            is_outdated: false,
            comments: vec![RemoteReviewComment {
                id: "rv1-root".to_string(),
                author: Some("carol".to_string()),
                body: "overall this looks fine".to_string(),
                created_at: None,
                in_reply_to: None,
                url: String::new(),
            }],
        }];
        app.rebuild_annotations();
        // when
        let buffer = draw(&mut app);
        let body = body_text(&buffer);
        // then — the badge and body appear in the rendered frame
        assert!(
            body.contains("carol"),
            "expected author in review comments:\n{body}"
        );
        assert!(
            body.contains("overall this looks fine"),
            "expected body in review comments:\n{body}"
        );
    }

    #[test]
    fn should_not_render_review_level_thread_when_comments_hidden() {
        let mut app = make_pr_app();
        app.forge_review_threads = vec![RemoteReviewThread {
            id: "rv1".to_string(),
            path: String::new(),
            line: None,
            side: RemoteCommentSide::Right,
            is_resolved: false,
            is_outdated: false,
            comments: vec![RemoteReviewComment {
                id: "rv1-root".to_string(),
                author: Some("carol".to_string()),
                body: "should be hidden".to_string(),
                created_at: None,
                in_reply_to: None,
                url: String::new(),
            }],
        }];
        app.set_remote_comments_visibility(PrCommentsVisibility::Hide);
        let buffer = draw(&mut app);
        let body = body_text(&buffer);
        assert!(
            !body.contains("should be hidden"),
            "review-level thread leaked under Hide:\n{body}"
        );
    }

    #[test]
    fn should_wrap_long_line_in_unified_view_when_wrap_enabled() {
        let long: String = "x".repeat(200);
        let hunk = DiffHunk {
            header: "@@ -0,0 +1,1 @@".to_string(),
            lines: vec![DiffLine {
                origin: LineOrigin::Addition,
                content: long.clone(),
                old_lineno: None,
                new_lineno: Some(1),
                highlighted_spans: None,
            }],
            old_start: 0,
            old_count: 0,
            new_start: 1,
            new_count: 1,
        };
        let hunks = vec![hunk];
        let content_hash = DiffFile::compute_content_hash(&hunks);
        let file = DiffFile {
            old_path: Some(PathBuf::from("src/lib.rs")),
            new_path: Some(PathBuf::from("src/lib.rs")),
            status: FileStatus::Modified,
            hunks,
            is_binary: false,
            is_too_large: false,
            is_commit_message: false,
            content_hash,
        };
        let mut app = make_revision_app(vec![file]);
        app.set_diff_wrap(true);
        app.rebuild_annotations();

        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| super::render_unified_diff(frame, &mut app, Rect::new(0, 0, 80, 20)))
            .expect("draw");
        let body = body_text(terminal.backend().buffer());

        let tail: String = long.chars().rev().take(20).collect::<String>();
        let tail: String = tail.chars().rev().collect();
        assert!(
            body.contains(&tail),
            "tail of wrapped long line should appear in body:\n{body}"
        );

        assert!(
            app.diff_state.visible_line_count > 0 && app.diff_state.visible_line_count < 20,
            "expected logical visible_line_count 1..20, got {}",
            app.diff_state.visible_line_count
        );
    }

    /// Comment boxes outside the viewport are replaced with blank placeholder
    /// rows instead of being formatted. The rows still have to be there, and in
    /// the right number, or every row below the comment would shift — so this
    /// also scrolls to the row `line_annotations` assigned the comment and
    /// expects the box to be there.
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

        let mut app = make_revision_app(vec![file]);
        app.session
            .get_file_mut(&path)
            .expect("file registered in session")
            .add_line_comment(
                100,
                Comment::new(NEEDLE.to_string(), CommentType::from_id("note"), None),
            );
        app.rebuild_annotations();

        // Top of the file: the comment is ~100 rows below a 12-row viewport.
        let buffer = draw_unified_diff(&mut app);
        let body = body_text(&buffer);
        assert!(
            !body.contains(NEEDLE),
            "off-screen comment should not be visible:\n{body}"
        );
        assert!(
            body.contains("line 1"),
            "diff content should still render:\n{body}"
        );

        // Scroll to the row the annotation builder says the comment occupies.
        // If the culled box had emitted the wrong number of placeholder rows,
        // this index would point somewhere else and the body would not appear.
        let comment_row = app
            .line_annotations
            .iter()
            .position(|a| matches!(a, AnnotatedLine::LineComment { .. }))
            .expect("comment annotated in the document");
        app.diff_state.scroll_offset = comment_row;
        app.diff_state.cursor_line = comment_row;

        let buffer = draw_unified_diff(&mut app);
        let body = body_text(&buffer);
        assert!(
            body.contains(NEEDLE),
            "comment scrolled into view should render at its annotated row:\n{body}"
        );
    }

    #[test]
    fn should_reach_last_line_scrolling_down_through_wrapped_content() {
        // Many long lines that wrap to several visual rows each, so far fewer
        // logical lines fit per screen than the viewport height. This is
        // what makes `visible_line_count` (wrap-aware) diverge sharply from
        // `viewport_height`. A short, uniquely-named last line lets us detect
        // whether repeated `j` ever scrolls it into view.
        let long: String = "x".repeat(200);
        let mut lines: Vec<DiffLine> = (0..30)
            .map(|i| DiffLine {
                origin: LineOrigin::Addition,
                content: long.clone(),
                old_lineno: None,
                new_lineno: Some(i + 1),
                highlighted_spans: None,
            })
            .collect();
        lines.push(DiffLine {
            origin: LineOrigin::Addition,
            content: "LASTLINEMARKER".to_string(),
            old_lineno: None,
            new_lineno: Some(31),
            highlighted_spans: None,
        });
        let hunk = DiffHunk {
            header: "@@ -0,0 +1,31 @@".to_string(),
            lines,
            old_start: 0,
            old_count: 0,
            new_start: 1,
            new_count: 31,
        };
        let hunks = vec![hunk];
        let content_hash = DiffFile::compute_content_hash(&hunks);
        let file = DiffFile {
            old_path: Some(PathBuf::from("src/lib.rs")),
            new_path: Some(PathBuf::from("src/lib.rs")),
            status: FileStatus::Modified,
            hunks,
            is_binary: false,
            is_too_large: false,
            is_commit_message: false,
            content_hash,
        };
        let mut app = make_revision_app(vec![file]);
        app.set_diff_wrap(true);
        app.rebuild_annotations();

        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        // Drive `j` one keypress at a time, re-rendering between presses so
        // `visible_line_count` is refreshed the way it would be in the real
        // render loop. Far more presses than there are logical lines, so a
        // working implementation has ample opportunity to reach the end.
        let max_presses = app.total_lines() * 3;
        for _ in 0..max_presses {
            terminal
                .draw(|frame| super::render_unified_diff(frame, &mut app, Rect::new(0, 0, 80, 20)))
                .expect("draw");
            app.cursor_down(1);
        }
        terminal
            .draw(|frame| super::render_unified_diff(frame, &mut app, Rect::new(0, 0, 80, 20)))
            .expect("draw");
        let body = body_text(terminal.backend().buffer());

        assert_eq!(
            app.diff_state.cursor_line,
            app.max_cursor_line(),
            "cursor should saturate at the last navigable line"
        );
        assert!(
            body.contains("LASTLINEMARKER"),
            "scrolling down should eventually reveal the last line; view got stuck:\n{body}"
        );
    }

    #[test]
    fn should_extend_comment_bar_over_wrapped_rows_when_wrap_enabled() {
        use crate::model::{Comment, CommentType};

        let long: String = "x".repeat(200);
        let hunk = DiffHunk {
            header: "@@ -0,0 +1,1 @@".to_string(),
            lines: vec![DiffLine {
                origin: LineOrigin::Addition,
                content: long.clone(),
                old_lineno: None,
                new_lineno: Some(1),
                highlighted_spans: None,
            }],
            old_start: 0,
            old_count: 0,
            new_start: 1,
            new_count: 1,
        };
        let hunks = vec![hunk];
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
        let mut app = make_revision_app(vec![file]);
        app.set_diff_wrap(true);

        let file_review = app
            .session
            .get_file_mut(&path)
            .expect("file registered in session");
        file_review.add_line_comment(
            1,
            Comment::new(
                "needs a rename".to_string(),
                CommentType::from_id("note"),
                None,
            ),
        );
        app.rebuild_annotations();

        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| super::render_unified_diff(frame, &mut app, Rect::new(0, 0, 80, 20)))
            .expect("draw");
        let buffer = terminal.backend().buffer();

        let mut cap: Option<(u16, u16)> = None;
        let mut cap_count = 0;
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                if buffer[(x, y)].symbol() == "╭" {
                    cap = Some((x, y));
                    cap_count += 1;
                }
            }
        }
        assert_eq!(
            cap_count, 1,
            "expected exactly one ╭ cap in the gutter, got {cap_count}"
        );
        let (bar_x, cap_y) = cap.unwrap();

        let mut box_top_y: Option<u16> = None;
        for y in (cap_y + 1)..buffer.area.height {
            if buffer[(bar_x, y)].symbol() == "├" {
                box_top_y = Some(y);
                break;
            }
        }
        let box_top_y = box_top_y.expect("expected ├ box top below the ╭ cap");
        assert!(
            box_top_y > cap_y + 1,
            "test needs at least one row between cap and box top; cap_y={cap_y} box_top_y={box_top_y}"
        );

        for y in (cap_y + 1)..box_top_y {
            let glyph = buffer[(bar_x, y)].symbol();
            assert_eq!(
                glyph, "│",
                "expected │ at ({bar_x},{y}) between cap ({cap_y}) and box top ({box_top_y}), got {glyph:?}"
            );
        }
    }
}
