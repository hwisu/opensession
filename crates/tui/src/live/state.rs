#[derive(Debug, Clone)]
pub struct FollowTailState {
    pub is_following: bool,
    pub detached_by_user: bool,
    pub auto_follow_threshold_rows: usize,
    pub was_near_tail_before_update: bool,
}

impl Default for FollowTailState {
    fn default() -> Self {
        Self {
            is_following: true,
            detached_by_user: false,
            auto_follow_threshold_rows: 2,
            was_near_tail_before_update: true,
        }
    }
}

impl FollowTailState {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn detach(&mut self) {
        self.is_following = false;
        self.detached_by_user = true;
    }

    pub fn reattach(&mut self) {
        self.is_following = true;
        self.detached_by_user = false;
    }

    pub fn mark_before_update(&mut self, near_tail: bool) {
        self.was_near_tail_before_update = near_tail;
    }

    pub fn should_follow_after_update(&self) -> bool {
        self.is_following && !self.detached_by_user && self.was_near_tail_before_update
    }
}

#[cfg(test)]
mod tests {
    use super::FollowTailState;

    #[test]
    fn follow_state_detach_and_reattach_roundtrip() {
        let mut state = FollowTailState::default();
        assert!(state.should_follow_after_update());

        state.detach();
        state.mark_before_update(true);
        assert!(!state.should_follow_after_update());

        state.reattach();
        state.mark_before_update(true);
        assert!(state.should_follow_after_update());
    }
}
