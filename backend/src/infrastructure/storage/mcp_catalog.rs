use super::{
    OptionalExtension, PortError, SqliteStore, SystemTime, TransactionBehavior, db_error, millis,
    params,
};
use crate::domain::mcp_content::McpContentConfig;
use uuid::Uuid;
impl SqliteStore {
    /// Startup publication of reviewed immutable content. Same revision cannot mean new content.
    pub async fn bind_mcp_catalog(
        &self,
        workspace: Uuid,
        profile: Uuid,
        actor: Uuid,
        config: McpContentConfig,
    ) -> Result<(), PortError> {
        if !config.is_valid() {
            return Err(PortError::InvalidInput {
                code: "invalid_mcp_catalog",
            });
        }
        self.run_critical(move|c| {
            let record=super::capacity::encode_bounded(&config)?;
            let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db_error)?;
            let w=workspace.to_string();let p=profile.to_string();
            let existing:Option<(u64,String)>=tx.query_row("SELECT revision,record FROM mcp_catalog_releases WHERE workspace=?1 AND profile=?2 ORDER BY revision DESC LIMIT 1",params![w,p],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(db_error)?;
            if let Some((revision,old))=existing {
                if revision>config.revision || (revision==config.revision&&record!=old) {return Err(PortError::Conflict)}
                if revision==config.revision {return Ok(())}
            }
            tx.execute("INSERT INTO mcp_catalog_releases VALUES(?1,?2,?3,?4,?5,?6)",params![w,p,config.revision,record,actor.to_string(),millis(SystemTime::now())?]).map_err(db_error)?;
            tx.commit().map_err(|_|PortError::OutcomeUnknown)
        }).await
    }
}
