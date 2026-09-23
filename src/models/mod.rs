pub mod change;
pub mod project;
pub mod token;
pub mod phase;
pub mod task;
pub mod artifact;
pub mod run_log;
pub mod password;
pub mod setup_code;

/// Tell "absent" from "null" in a JSON patch.
///
/// A plain `Option<Option<T>>` collapses the two — serde reads `null` as the
/// outer `None` — so a patch could never clear a field, only leave it alone.
/// With this on a field: absent is `None`, `null` is `Some(None)`, a value is
/// `Some(Some(v))`. Pair with `#[serde(default)]`.
pub fn present<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    serde::Deserialize::deserialize(d).map(Some)
}
