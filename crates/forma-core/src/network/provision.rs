//! Forma's section of connman's provisioning file (`/userdata/system/network-connman.config`).
//!
//! The file may hold the user's own sections, so Forma reads and rewrites only
//! `[service_forma_ethernet]` and leaves everything else as it found it.

use std::path::Path;

use super::EthernetConfig;
use crate::error::{Error, Result};

const SECTION: &str = "[service_forma_ethernet]";

/// The static address Forma saved, or DHCP when there is none.
pub fn read(path: &Path) -> EthernetConfig {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let (_, ours) = split(&text);
    let value = |key: &str| {
        ours.iter()
            .filter_map(|l| l.split_once('='))
            .find(|(k, _)| k.trim() == key)
            .map(|(_, v)| v.trim().to_owned())
    };
    let Some(ipv4) = value("IPv4") else {
        return EthernetConfig::Dhcp;
    };
    let mut parts = ipv4.split('/').map(str::to_owned);
    match (parts.next(), parts.next(), parts.next()) {
        (Some(address), Some(netmask), gateway) => EthernetConfig::Manual {
            address,
            netmask,
            gateway,
            nameservers: value("Nameservers")
                .map(|n| {
                    n.split(',')
                        .map(|s| s.trim().to_owned())
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default(),
        },
        _ => EthernetConfig::Dhcp,
    }
}

/// Saves `config` for the interface with this MAC address. DHCP removes Forma's section, and
/// the file too if nothing else is left in it.
pub fn write(path: &Path, mac: &str, config: &EthernetConfig) -> Result<()> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let (others, _) = split(&text);
    let mut out = others.join("\n").trim_end().to_owned();
    if let EthernetConfig::Manual {
        address,
        netmask,
        gateway,
        nameservers,
    } = config
    {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        let gateway = gateway
            .as_deref()
            .map(|g| format!("/{g}"))
            .unwrap_or_default();
        out.push_str(&format!(
            "{SECTION}\n# Written by Forma's network settings.\nType=ethernet\nMAC={}\n\
             IPv4={address}/{netmask}{gateway}\n",
            mac.to_ascii_lowercase()
        ));
        if !nameservers.is_empty() {
            out.push_str(&format!("Nameservers={}\n", nameservers.join(",")));
        }
    }
    if out.trim().is_empty() {
        return match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(Error::io(path, e)),
            _ => Ok(()),
        };
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    // Replaced whole, so connman never reads half a file. The link in /var/lib/connman points at
    // the path, not the old file, so it follows.
    let tmp = path.with_extension("config.forma-tmp");
    std::fs::write(&tmp, out).map_err(|e| Error::io(&tmp, e))?;
    std::fs::rename(&tmp, path).map_err(|e| Error::io(path, e))
}

/// The file's lines outside Forma's section, and the lines inside it.
fn split(text: &str) -> (Vec<&str>, Vec<&str>) {
    let mut others = Vec::new();
    let mut ours = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if line.trim_start().starts_with('[') {
            inside = line.trim() == SECTION;
        }
        if inside {
            ours.push(line);
        } else {
            others.push(line);
        }
    }
    (others, ours)
}
