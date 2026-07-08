use super::*;

impl App {
    pub fn set_message(&mut self, msg: impl Into<String>) {
        self.set_message_inner(msg, MessageType::Info, Some(MESSAGE_TTL_INFO));
    }

    pub fn set_warning(&mut self, msg: impl Into<String>) {
        self.set_message_inner(msg, MessageType::Warning, Some(MESSAGE_TTL_WARNING));
    }

    pub fn set_error(&mut self, msg: impl Into<String>) {
        self.set_message_inner(msg, MessageType::Error, None);
    }

    /// Warning that stays until something else overwrites it. Used for state-tied
    /// messages like the dirty-quit prompt where the visual must outlive any TTL.
    pub fn set_sticky_warning(&mut self, msg: impl Into<String>) {
        self.set_message_inner(msg, MessageType::Warning, None);
    }

    fn set_message_inner(
        &mut self,
        msg: impl Into<String>,
        message_type: MessageType,
        ttl: Option<Duration>,
    ) {
        self.message = Some(Message {
            content: msg.into(),
            message_type,
            expires_at: ttl.map(|d| Instant::now() + d),
        });
    }

    /// Returns `true` if a message was cleared so the main loop can
    /// schedule a redraw.
    pub fn clear_expired_message(&mut self) -> bool {
        let expired = self
            .message
            .as_ref()
            .and_then(|m| m.expires_at)
            .is_some_and(|t| Instant::now() >= t);
        if expired {
            self.message = None;
        }
        expired
    }

    pub fn enter_command_mode(&mut self) {
        self.input_mode = InputMode::Command;
        self.command_buffer.clear();
        self.command_completion = None;
    }

    pub fn exit_command_mode(&mut self) {
        self.input_mode = InputMode::Normal;
        self.command_buffer.clear();
        self.command_completion = None;
    }

    pub fn enter_search_mode(&mut self) {
        self.input_mode = InputMode::Search;
        self.search_buffer.clear();
    }

    pub fn exit_search_mode(&mut self) {
        self.input_mode = InputMode::Normal;
        self.search_buffer.clear();
    }

    pub fn toggle_help(&mut self) {
        if self.input_mode == InputMode::Help {
            self.input_mode = InputMode::Normal;
        } else {
            self.input_mode = InputMode::Help;
            self.help_state.scroll_offset = 0;
        }
    }

    pub fn open_pr_details(&mut self) -> bool {
        if !matches!(self.diff_source, DiffSource::PullRequest(_)) {
            self.set_error("PR details are only available while reviewing a pull request");
            return false;
        }
        self.input_mode = InputMode::Details;
        self.details_state.scroll_offset = 0;
        true
    }

    pub fn close_pr_details(&mut self) {
        self.input_mode = InputMode::Normal;
    }

    pub fn help_scroll_down(&mut self, lines: usize) {
        scroll_state_down(&mut self.help_state, lines);
    }

    pub fn help_scroll_up(&mut self, lines: usize) {
        scroll_state_up(&mut self.help_state, lines);
    }

    pub fn help_scroll_to_top(&mut self) {
        self.help_state.scroll_offset = 0;
    }

    pub fn help_scroll_to_bottom(&mut self) {
        scroll_state_to_bottom(&mut self.help_state);
    }

    pub fn details_scroll_down(&mut self, lines: usize) {
        scroll_state_down(&mut self.details_state, lines);
    }

    pub fn details_scroll_up(&mut self, lines: usize) {
        scroll_state_up(&mut self.details_state, lines);
    }

    pub fn details_scroll_to_top(&mut self) {
        self.details_state.scroll_offset = 0;
    }

    pub fn details_scroll_to_bottom(&mut self) {
        scroll_state_to_bottom(&mut self.details_state);
    }

    pub fn enter_confirm_mode(&mut self, action: ConfirmAction) {
        self.input_mode = InputMode::Confirm;
        self.pending_confirm = Some(action);
    }

    pub fn exit_confirm_mode(&mut self) {
        self.input_mode = InputMode::Normal;
        self.pending_confirm = None;
    }
}
