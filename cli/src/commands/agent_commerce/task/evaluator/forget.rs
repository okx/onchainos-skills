use anyhow::Result;

use super::commit_store;
use crate::commands::agent_commerce::task::signing;

/// Delete the locally-cached commit record for a settled dispute.
/// Invoked by the `dispute_resolved` / `round_failed` flow step — dispute round is terminal, the `{disputeId, side}`
/// record is no longer needed. Idempotent: reports 0 removed if nothing matched.
pub async fn handle_forget(dispute_id: &str) -> Result<()> {
    let (_, address) = signing::resolve_wallet(None, None)?;
    let removed = commit_store::remove(dispute_id, &address)?;
    if removed > 0 {
        println!("forgot {removed} commit record(s) for disputeId={dispute_id} voter={address}");
    } else {
        println!("no matching commit record for disputeId={dispute_id} voter={address} (already clean)");
    }
    Ok(())
}
