use std::io;

pub(super) fn io_rejected(
    operation: &'static str,
    boundary: &'static str,
    error: &io::Error,
) {
    let reason = if error.kind() == io::ErrorKind::TimedOut {
        "deadline"
    } else {
        boundary
    };
    tracing::debug!(
        target: "omarchy_quickshare::protocol",
        stage = "framing",
        operation,
        outcome = "rejected",
        reason,
        io_error_kind = io_error_kind(error.kind()),
        "connection_io"
    );
}

pub(super) fn io_error_kind(kind: io::ErrorKind) -> &'static str {
    if kind == io::ErrorKind::NotFound {
        return "not_found";
    }
    if kind == io::ErrorKind::PermissionDenied {
        return "permission_denied";
    }
    if kind == io::ErrorKind::ConnectionRefused {
        return "connection_refused";
    }
    if kind == io::ErrorKind::ConnectionReset {
        return "connection_reset";
    }
    if kind == io::ErrorKind::ConnectionAborted {
        return "connection_aborted";
    }
    if kind == io::ErrorKind::NotConnected {
        return "not_connected";
    }
    if kind == io::ErrorKind::AddrInUse {
        return "address_in_use";
    }
    if kind == io::ErrorKind::AddrNotAvailable {
        return "address_not_available";
    }
    if kind == io::ErrorKind::BrokenPipe {
        return "broken_pipe";
    }
    other_io_error_kind(kind)
}

fn other_io_error_kind(kind: io::ErrorKind) -> &'static str {
    if kind == io::ErrorKind::AlreadyExists {
        return "already_exists";
    }
    if kind == io::ErrorKind::WouldBlock {
        return "would_block";
    }
    if kind == io::ErrorKind::InvalidInput {
        return "invalid_input";
    }
    if kind == io::ErrorKind::InvalidData {
        return "invalid_data";
    }
    if kind == io::ErrorKind::TimedOut {
        return "timed_out";
    }
    if kind == io::ErrorKind::WriteZero {
        return "write_zero";
    }
    if kind == io::ErrorKind::Interrupted {
        return "interrupted";
    }
    if kind == io::ErrorKind::UnexpectedEof {
        return "unexpected_eof";
    }
    if kind == io::ErrorKind::OutOfMemory {
        return "out_of_memory";
    }
    "other"
}
