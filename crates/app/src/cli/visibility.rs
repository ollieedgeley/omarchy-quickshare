use std::io::{self, Write};

use quickshare_sharing::EndpointSnapshot;

/// Renders daemon-owned inbound permission and listener readiness.
pub(super) fn write_status<Output>(
    output: &mut Output,
    snapshot: &EndpointSnapshot,
) -> io::Result<()>
where
    Output: Write,
{
    let status = snapshot.visibility_status();
    writeln!(output, "visibility={:?}", snapshot.visibility())?;
    writeln!(output, "discoverable={}", status.discoverable)?;
    writeln!(output, "visibility_requested={}", status.requested)?;
    writeln!(output, "visibility_temporary={}", status.temporary)?;
    write!(output, "visibility_remaining_secs=")?;
    if let Some(remaining) = status.remaining_secs {
        write!(output, "{remaining}")?;
    }
    writeln!(output)?;
    write!(output, "visibility_available_media=")?;
    for (index, medium) in status.available_media.iter().enumerate() {
        if index > 0 {
            write!(output, ",")?;
        }
        write!(output, "{medium}")?;
    }
    writeln!(output)?;
    if let Some(error) = &status.error {
        writeln!(output, "visibility_error={error}")?;
    }
    Ok(())
}
