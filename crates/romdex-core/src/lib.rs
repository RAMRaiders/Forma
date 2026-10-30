//! RomDex core.
//!
//! Everything a frontend needs that is not a window: reading the Batocera library, resolving
//! hierarchy views, launching games and normalising input. It must not depend on Tauri, so it
//! can be tested on any machine and reused by other hosts.

mod error;
pub mod field;
pub mod gamelist;
pub mod hierarchy;
pub mod input;
pub mod launch;
pub mod library;
pub mod net;
pub mod paths;
pub mod scan;
pub mod sync;
pub mod systems;
pub mod themes;
mod time;
mod xml;

#[cfg(test)]
mod test_support;

pub use error::{Error, Result, Warning};
pub use library::{Game, Library, RefreshReport, SystemSummary};
pub use paths::Paths;

/// Version of the theme-facing API this core implements. Themes declare the range they need in
/// `theme.json` (`"sdk": "^1"`) and `@romdex/sdk` checks it against this value at startup.
pub const SDK_API_VERSION: &str = "1.0.0";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_version_is_semver() {
        let parts: Vec<&str> = SDK_API_VERSION.split('.').collect();
        assert_eq!(parts.len(), 3);
        assert!(parts.iter().all(|p| p.parse::<u32>().is_ok()));
    }
}
