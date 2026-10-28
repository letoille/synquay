//! Internal recent-message read port. Callers must authorize the exact conversation first.
use super::{PortError, PortFuture};
use crate::domain::recent_messages::{ConversationKey, RecentPage};

pub trait RecentMessages: Send + Sync {
    fn read_recent(
        &self,
        conversation: ConversationKey,
        before: Option<u64>,
        limit: usize,
    ) -> PortFuture<'_, RecentPage>;
}

pub fn validate_limit(limit: usize) -> Result<(), PortError> {
    if (1..=50).contains(&limit) {
        Ok(())
    } else {
        Err(PortError::InvalidInput {
            code: "invalid_recent_limit",
        })
    }
}
