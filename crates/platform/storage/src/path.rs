use crate::Error;
use std::path::{Component, Path};

/// Linux `NAME_MAX` for a single directory entry.
const NAME_MAX: usize = 255;

/// Returns `name` when it is a single safe destination basename.
#[expect(
    clippy::pub_with_shorthand,
    clippy::redundant_pub_crate,
    clippy::single_call_fn,
    reason = "isolates destination basename policy from staging mechanics"
)]
pub(crate) fn safe_file_name(name: &str) -> Result<&str, Error> {
    if name.is_empty() || name.len() > NAME_MAX {
        return Err(Error::InvalidName);
    }
    if name == "." || name == ".." || name.starts_with(".quickshare-") {
        return Err(Error::InvalidName);
    }
    if name
        .bytes()
        .any(|byte| byte == b'/' || byte == b'\\' || byte.is_ascii_control())
    {
        return Err(Error::InvalidName);
    }
    let path = Path::new(name);
    match (path.components().next(), path.components().nth(1)) {
        (Some(Component::Normal(component)), None) if component == name => {
            Ok(name)
        }
        _ => Err(Error::InvalidName),
    }
}
