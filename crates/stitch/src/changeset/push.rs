use crate::changeset;
use crate::model::ChangesetState;

pub fn execute() -> Result<(), String> {
    let mut cs = match changeset::load_current()? {
        Some(cs) => cs,
        None => return Err("No active changeset.".to_string()),
    };

    if cs.state != ChangesetState::Committed {
        return Err(format!("Changeset state is '{}', not 'committed'. Run `stitch changeset commit` first.", cs.state));
    }

    let ws_name = cs.workspace.clone();
    let cs_id = cs.id.clone();
    // TODO: implement push
    // For now, acknowledge the request

    println!("Push requested for changeset '{}' in workspace '{}'.", cs_id, ws_name);
    println!("Push not yet implemented in this first pass.");
    println!("");
    println!("Future behavior:");
    println!("  1. For each committed repo, run: git push");
    println!("  2. If all push: changeset -> pushed");
    println!("  3. If some fail: changeset -> pushed-partial");

    cs.state = ChangesetState::Pushed;
    changeset::save(&cs)?;

    Ok(())
}
