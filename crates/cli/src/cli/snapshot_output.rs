//! Renders endpoint snapshots and operation outcomes for CLI consumers.

use std::io::{self, Write};

use quickshare_control::response::{OperationRecord, Response};
use quickshare_sharing::{Direction, EndpointSnapshot, Phase};

/// Writes machine-readable endpoint observations as key=value lines.
#[expect(
    clippy::pattern_type_mismatch,
    reason = "Borrowed snapshot fields are rendered without taking ownership"
)]
pub(super) fn write_status<Output>(
    output: &mut Output,
    response: &Response,
) -> io::Result<()>
where
    Output: Write,
{
    let Response::Snapshot {
        snapshot,
        preferences,
        operations,
    } = response
    else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "endpoint did not return a snapshot",
        ));
    };
    write_snapshot(output, snapshot, preferences.applied.as_ref())?;
    writeln!(output, "preferences_pending={}", preferences.pending)?;
    if let Some(error) = &preferences.error {
        writeln!(output, "preferences_error={error}")?;
    }
    for operation in operations {
        write_operation(output, operation)?;
    }
    Ok(())
}

pub(super) fn write_operation<Output>(
    output: &mut Output,
    operation: &OperationRecord,
) -> io::Result<()>
where
    Output: Write,
{
    writeln!(output, "operation_id={}", operation.id_string)?;
    if let Some(request_id) = &operation.request_id {
        writeln!(output, "request_id={request_id}")?;
    }
    writeln!(output, "direction={:?}", operation.direction)?;
    writeln!(output, "phase={:?}", operation.phase)?;
    writeln!(output, "total_bytes={}", operation.total_bytes)?;
    writeln!(output, "transferred_bytes={}", operation.transferred_bytes)?;
    writeln!(output, "dismissed={}", operation.dismissed)?;
    if let Some(reason) = &operation.terminal_reason {
        writeln!(output, "terminal_reason={reason}")?;
    }
    if let Some(guidance) = &operation.recovery_guidance {
        writeln!(output, "recovery_guidance={guidance}")?;
    }
    Ok(())
}

/// Renders public snapshot fields used by humans and scripts.
fn write_snapshot<Output>(
    output: &mut Output,
    snapshot: &EndpointSnapshot,
    applied: Option<&quickshare_control::response::PreferenceValues>,
) -> io::Result<()>
where
    Output: Write,
{
    if let Some(applied) = applied {
        let directory = applied.receive_directory.display();
        writeln!(output, "receive_directory={directory}")?;
    }
    writeln!(output, "discovery={:?}", snapshot.discovery())?;
    super::visibility::write_status(output, snapshot)?;
    if let Some(peer) = snapshot.peers().iter().find(|peer| peer.is_pinned()) {
        writeln!(output, "pinned_peer={}", peer.id())?;
    } else if let Some(peer_id) =
        applied.and_then(|value| value.pinned_peer_id.as_deref())
    {
        writeln!(output, "pinned_peer={peer_id}")?;
    } else {
        writeln!(output, "pinned_peer=")?;
    }
    let Some(active) = snapshot.active_share() else {
        return Ok(());
    };
    writeln!(output, "share_id={}", active.id().get())?;
    writeln!(
        output,
        "direction={}",
        match active.direction() {
            Direction::Inbound => "inbound",
            Direction::Outbound => "outbound",
            _ => "unknown",
        }
    )?;
    writeln!(output, "phase={:?}", active.phase())?;
    writeln!(output, "transferred_bytes={}", active.transferred_bytes())?;
    writeln!(output, "total_bytes={}", active.total_bytes())?;
    if let Some(code) = active.verification_code() {
        writeln!(output, "verification_code={code}")?;
    }
    let fallback = terminal_guidance(active.phase());
    if let Some(reason) = active
        .terminal_reason()
        .or_else(|| fallback.map(|(reason, _)| reason))
    {
        writeln!(output, "terminal_reason={reason}")?;
    }
    if let Some(recovery) = active
        .recovery_guidance()
        .or_else(|| fallback.map(|(_, recovery)| recovery))
    {
        writeln!(output, "recovery_guidance={recovery}")?;
    }
    Ok(())
}

/// Returns a user-facing reason and recovery hint for a terminal phase.
const fn terminal_guidance(
    phase: Phase,
) -> Option<(&'static str, &'static str)> {
    match phase {
        Phase::Cancelled => Some((
            "share cancelled",
            "Submit the content again if you still want to send it.",
        )),
        Phase::Failed => Some((
            "transfer failed",
            "Retry the share. Confirm the peer is nearby and the receive \
             directory is writable.",
        )),
        Phase::Rejected => Some((
            "share rejected",
            "Choose another peer or ask the receiver to accept.",
        )),
        _ => None,
    }
}
