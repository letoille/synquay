use super::Pipeline;
use crate::{
    application::ports::{PortError, recovery::RecoveryStore},
    domain::recovery::{RecoveryCommand, RecoveryReceipt, RecoveryTarget},
};
use serde_json::{Value, json};
use subtle::ConstantTimeEq;
use tokio::sync::OwnedSemaphorePermit;
use uuid::Uuid;

impl Pipeline {
    /// Explicit trusted embedding configuration; never accepts webhook/model arguments.
    pub fn with_management(mut self, principal: Uuid, token: String) -> Result<Self, PortError> {
        if principal.is_nil()
            || principal == self.principal
            || token == self.token
            || token.len() < 32
            || token.len() > 4096
            || !token.bytes().all(|b| b.is_ascii_graphic())
        {
            return Err(PortError::InvalidInput {
                code: "invalid_admin_secret",
            });
        }
        self.admin = Some((principal, token));
        Ok(self)
    }
    pub fn authenticate_operator(&self, header: Option<&str>) -> Option<Uuid> {
        let (principal, token) = self.admin.as_ref()?;
        let bearer = header?.strip_prefix("Bearer ")?;
        bool::from(bearer.as_bytes().ct_eq(token.as_bytes())).then_some(*principal)
    }
    pub fn try_admit_admin(&self) -> Result<OwnedSemaphorePermit, PortError> {
        self.admin_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| PortError::RateLimited {
                retry_after: std::time::Duration::from_secs(1),
            })
    }
    fn authorize_operator(&self, principal: Uuid) -> Result<(), PortError> {
        if self.admin.as_ref().is_some_and(|(p, _)| *p == principal) {
            Ok(())
        } else {
            Err(PortError::PermissionDenied)
        }
    }
    pub async fn recover(
        &self,
        principal: Uuid,
        command: RecoveryCommand,
    ) -> Result<RecoveryReceipt, PortError> {
        self.authorize_operator(principal)?;
        if let RecoveryTarget::Route { source_id, .. } = &command.target
            && *source_id != self.source.connection_id
        {
            return Err(PortError::ScopeMismatch);
        }
        let result = self
            .store
            .recover(self.source.workspace_id, principal, command)
            .await;
        if result.is_ok() {
            self.wake.notify_waiters();
        }
        result
    }
    pub async fn operations(&self, principal: Uuid) -> Result<Value, PortError> {
        self.authorize_operator(principal)?;
        let database = self
            .store
            .operations_snapshot(self.source.workspace_id)
            .await?;
        let mut targets = Vec::new();
        for lane in &self.delivery_lanes {
            let s = lane.state.lock().await;
            targets.push(json!({"target_id":lane.id,"consecutive_failures":s.failures,"circuit_open_remaining_ms":s.open_until.map(|t|t.saturating_duration_since(std::time::Instant::now()).as_millis()),"processed":s.starts}));
        }
        Ok(
            json!({"database":database,"runtime":self.store.metrics.snapshot(),"targets":targets,"scheduler":self.scheduler}),
        )
    }
}

impl Pipeline {
    pub async fn inspect_recovery(
        &self,
        principal: Uuid,
        kind: &str,
        id: Uuid,
    ) -> Result<Value, PortError> {
        self.authorize_operator(principal)?;
        let target = match kind {
            "delivery" => RecoveryTarget::Delivery { delivery_id: id },
            "route" => RecoveryTarget::Route {
                source_id: self.source.connection_id,
                event_id: id,
            },
            _ => return Err(PortError::NotFound),
        };
        self.store
            .inspect_recovery(self.source.workspace_id, target)
            .await
    }
}
