#[derive(Debug, Clone, Default)]
pub struct VisibilitySession {
    pub has_session: bool,
    pub hidden_by_user: bool,
    pub visible: bool,
    pub missing_since: Option<u64>,
    pub hide_at: Option<u64>,
}
impl VisibilitySession {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn observe(&mut self, connected: bool, now: u64) -> bool {
        if connected {
            self.missing_since = None;
            self.hide_at = None;
            if !self.has_session {
                self.has_session = true;
                self.hidden_by_user = false;
                self.visible = true;
                return true;
            }
            return false;
        }
        if !self.has_session {
            return false;
        }
        let missing = self.missing_since.get_or_insert(now);
        if now.saturating_sub(*missing) < 3_000 {
            return false;
        }
        self.has_session = false;
        self.hide_at = if self.visible {
            Some(now.saturating_add(3_000))
        } else {
            None
        };
        false
    }
    pub fn tick(&mut self, now: u64) -> bool {
        if self.hide_at.is_some_and(|deadline| now >= deadline) {
            self.hide_at = None;
            self.visible = false;
            true
        } else {
            false
        }
    }
    pub fn hide_manually(&mut self) {
        self.visible = false;
        self.hidden_by_user = true;
        self.hide_at = None;
    }
    pub fn show_manually(&mut self) {
        self.visible = true;
        self.hidden_by_user = false;
        self.hide_at = None;
    }
    pub fn reset_for_device(&mut self) {
        self.has_session = false;
        self.missing_since = None;
        self.hide_at = None;
    }
}
