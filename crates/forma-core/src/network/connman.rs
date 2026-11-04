//! Reading `connmanctl`'s output: the service list and the `Key = value` property blocks.

use std::collections::HashMap;

/// One line of `connmanctl services`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceLine {
    pub id: String,
    /// Empty for a hidden Wi-Fi network.
    pub name: String,
    pub favorite: bool,
    /// Connected: `ready` (an address) or `online` (and the internet answers).
    pub connected: bool,
    /// `online`: connman's own check found the internet.
    pub online: bool,
}

impl ServiceLine {
    pub fn is_ethernet(&self) -> bool {
        self.id.starts_with("ethernet_")
    }

    pub fn is_wifi(&self) -> bool {
        self.id.starts_with("wifi_")
    }

    /// From the id's last part: `wifi_<mac>_<ssid>_managed_<security>`.
    pub fn wifi_security(&self) -> &str {
        self.id.rsplit('_').next().unwrap_or("")
    }
}

/// `*AO Wired                ethernet_…_cable`: three flag columns, then the name (which may
/// hold spaces) and the id (which never does).
pub fn services(output: &str) -> Vec<ServiceLine> {
    output
        .lines()
        .filter(|l| l.len() > 4)
        .filter_map(|line| {
            let (flags, rest) = line.split_at(4);
            let rest = rest.trim_end();
            let id = rest.rsplit(char::is_whitespace).next()?;
            if !id.contains('_') {
                return None;
            }
            Some(ServiceLine {
                id: id.to_owned(),
                name: rest[..rest.len() - id.len()].trim().to_owned(),
                favorite: flags.contains('*'),
                connected: flags.contains('O') || flags.contains('R'),
                online: flags.contains('O'),
            })
        })
        .collect()
}

/// The `  Key = value` lines under one object path. For `connmanctl technologies`, which lists
/// several, see [`technologies`].
pub fn properties(output: &str) -> HashMap<String, String> {
    output
        .lines()
        .filter(|l| l.starts_with(char::is_whitespace))
        .filter_map(|l| l.split_once(" = "))
        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
        .collect()
}

/// Each technology's type (`ethernet`, `wifi`…) and whether it is powered.
pub fn technologies(output: &str) -> HashMap<String, bool> {
    let mut out = HashMap::new();
    for block in output.split("/net/connman/technology/").skip(1) {
        let props = properties(block);
        if let Some(kind) = props.get("Type") {
            out.insert(
                kind.clone(),
                props.get("Powered").is_some_and(|p| p == "True"),
            );
        }
    }
    out
}

/// `[ Method=dhcp, Address=192.168.1.2, … ]` as pairs.
pub fn dict(value: &str) -> HashMap<String, String> {
    list(value)
        .into_iter()
        .filter_map(|item| {
            item.split_once('=')
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
        })
        .collect()
}

/// `[ 192.168.1.254, 1.1.1.1 ]` as items; `[  ]` is empty.
pub fn list(value: &str) -> Vec<String> {
    value
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

/// `connmanctl` exits 0 even when it fails, and says so on standard output.
pub fn error(output: &str) -> Option<&str> {
    output
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("Error "))
}
