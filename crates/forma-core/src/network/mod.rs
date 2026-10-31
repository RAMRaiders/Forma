//! This box's own connections, Ethernet and Wi-Fi, through connman as Batocera sets it up.
//! (Online services, RetroAchievements and the netplay lobby, are [`crate::net`].)
//!
//! Wi-Fi goes through Batocera's `batocera-wifi`, which keeps the network in `batocera.conf`,
//! so EmulationStation and Forma share it. A static Ethernet address goes in
//! `/userdata/system/network-connman.config`, the provisioning file Batocera links into
//! `/var/lib/connman`: `/var` lives in memory, so anything set with `connmanctl config` alone
//! is gone at the next boot. connman reads provisioning files when it starts, so saving one
//! restarts it, as `batocera-wifi` does.
//!
//! Batocera runs connman with `SingleConnectedTechnology` and Ethernet preferred: while a cable
//! is plugged in, a Wi-Fi network is saved for later rather than used.

mod connman;
mod provision;
#[cfg(test)]
mod tests;

use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::net::batocera_conf;
use crate::paths::Paths;

use connman::ServiceLine;

/// Runs the system's tools; a fake one in tests.
pub trait Shell: Send + Sync {
    /// Runs `program` to completion and returns its standard output.
    fn run(&self, program: &str, args: &[&str]) -> Result<String>;
}

/// Runs real programs, with the host's environment rather than the AppImage's (see
/// [`crate::host`]): `connmand`, restarted from here, must not load Forma's libraries. Errors
/// name the program, never its arguments: those can hold a Wi-Fi password.
pub struct SystemShell;

impl Shell for SystemShell {
    fn run(&self, program: &str, args: &[&str]) -> Result<String> {
        let out = crate::host::command(program)
            .args(args)
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| Error::Network(format!("could not run `{program}`: {e}")))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let why = stderr.lines().next().unwrap_or("no message");
            return Err(Error::Network(format!("`{program}` failed: {why}")));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkStatus {
    /// `None` when the box has no wired interface.
    pub ethernet: Option<Ethernet>,
    /// `None` when the box has no Wi-Fi hardware.
    pub wifi: Option<Wifi>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Ethernet {
    /// A cable is in and connman has an address on it.
    pub connected: bool,
    pub ipv4: Option<Ipv4>,
    pub nameservers: Vec<String>,
    /// What is saved: DHCP unless Forma set a static address.
    pub config: EthernetConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Wifi {
    /// Batocera's `wifi.enabled`.
    pub enabled: bool,
    /// The network in use, if any.
    pub connected: Option<String>,
    pub ipv4: Option<Ipv4>,
    /// The network saved in `batocera.conf`, which Batocera joins whenever it can.
    pub saved: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Ipv4 {
    pub address: String,
    pub netmask: String,
    pub gateway: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "camelCase")]
pub enum EthernetConfig {
    Dhcp,
    #[serde(rename_all = "camelCase")]
    Manual {
        address: String,
        netmask: String,
        gateway: Option<String>,
        #[serde(default)]
        nameservers: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WifiNetwork {
    pub ssid: String,
    /// 0–100, as connman reports it.
    pub strength: Option<u8>,
    pub security: WifiSecurity,
    pub connected: bool,
    /// The network saved in `batocera.conf`.
    pub saved: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WifiSecurity {
    Open,
    Wep,
    Psk,
    /// 802.1X (eduroam and the like): Batocera's Wi-Fi settings cannot join these.
    Enterprise,
}

impl WifiSecurity {
    fn from_connman(security: &str) -> Self {
        match security {
            "none" => Self::Open,
            "wep" => Self::Wep,
            "ieee8021x" => Self::Enterprise,
            _ => Self::Psk,
        }
    }
}

/// How joining a Wi-Fi network ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Joined {
    Connected,
    /// Saved, but a cable is plugged in, so Ethernet stays in use.
    Saved,
}

pub struct Network {
    shell: Box<dyn Shell>,
    batocera_conf: PathBuf,
    provisioning: PathBuf,
    /// How long a Wi-Fi network has to accept the box, and how often to look.
    pub join_timeout: Duration,
    pub poll_every: Duration,
    /// Held while a change runs: each restarts connman, and two at once would interleave.
    changing: Mutex<()>,
}

const CONNMAN_INIT: &str = "/etc/init.d/S08connman";

impl Network {
    pub fn new(shell: Box<dyn Shell>, paths: &Paths) -> Self {
        Self {
            shell,
            batocera_conf: paths.batocera_conf(),
            provisioning: paths.network_config(),
            join_timeout: Duration::from_secs(30),
            poll_every: Duration::from_secs(1),
            changing: Mutex::new(()),
        }
    }

    pub fn status(&self) -> Result<NetworkStatus> {
        let technologies = connman::technologies(&self.connmanctl(&["technologies"])?);
        let services = self.services()?;

        let ethernet = match technologies.contains_key("ethernet") {
            false => None,
            true => {
                let config = provision::read(&self.provisioning);
                Some(match services.iter().find(|s| s.is_ethernet()) {
                    Some(service) => {
                        let props = self.details(&service.id)?;
                        Ethernet {
                            connected: service.connected,
                            ipv4: ipv4(&props),
                            nameservers: props
                                .get("Nameservers")
                                .map(|n| connman::list(n))
                                .unwrap_or_default(),
                            config,
                        }
                    }
                    None => Ethernet {
                        connected: false,
                        ipv4: None,
                        nameservers: Vec::new(),
                        config,
                    },
                })
            }
        };

        let wifi = match technologies.contains_key("wifi") {
            false => None,
            true => {
                let conf = batocera_conf(&self.batocera_conf);
                let active = services.iter().find(|s| s.is_wifi() && s.connected);
                let ipv4 = match active {
                    Some(service) => ipv4(&self.details(&service.id)?),
                    None => None,
                };
                Some(Wifi {
                    enabled: conf.get("wifi.enabled").is_some_and(|v| v == "1"),
                    connected: active.map(|s| s.name.clone()),
                    ipv4,
                    saved: conf.get("wifi.ssid").filter(|s| !s.is_empty()).cloned(),
                })
            }
        };
        Ok(NetworkStatus { ethernet, wifi })
    }

    /// Networks in range, the one in use first, then the saved one, then by strength. Empty
    /// while Wi-Fi is off.
    pub fn wifi_scan(&self) -> Result<Vec<WifiNetwork>> {
        match connman::technologies(&self.connmanctl(&["technologies"])?).get("wifi") {
            None => return Err(Error::Unsupported("this box has no Wi-Fi".into())),
            Some(false) => return Ok(Vec::new()),
            Some(true) => {}
        }
        self.connmanctl(&["scan", "wifi"])?;
        let saved = batocera_conf(&self.batocera_conf)
            .remove("wifi.ssid")
            .unwrap_or_default();

        let mut found: Vec<WifiNetwork> = Vec::new();
        // Hidden networks have no name to show; they can still be joined by name.
        for service in self
            .services()?
            .iter()
            .filter(|s| s.is_wifi() && !s.name.is_empty())
        {
            // A network can drop out of range between the list and this.
            let Ok(props) = self.details(&service.id) else {
                continue;
            };
            let strength = props.get("Strength").and_then(|s| s.parse().ok());
            // One network can show up once per radio: keep one, as strong as it gets.
            if let Some(known) = found.iter_mut().find(|n| n.ssid == service.name) {
                known.connected |= service.connected;
                known.strength = known.strength.max(strength);
                continue;
            }
            found.push(WifiNetwork {
                ssid: service.name.clone(),
                strength,
                security: WifiSecurity::from_connman(service.wifi_security()),
                connected: service.connected,
                saved: service.name == saved,
            });
        }
        found.sort_by_key(|n| {
            (
                !n.connected,
                !n.saved,
                std::cmp::Reverse(n.strength.unwrap_or(0)),
            )
        });
        Ok(found)
    }

    /// Saves `ssid` as Batocera's Wi-Fi network and waits for it to connect. Turns Wi-Fi on.
    pub fn wifi_join(&self, ssid: &str, passphrase: Option<&str>) -> Result<Joined> {
        let ssid = ssid.trim();
        if ssid.is_empty() || ssid.len() > 32 || ssid.contains(char::is_control) {
            return Err(Error::Invalid(
                "a Wi-Fi network name is 1–32 characters".into(),
            ));
        }
        let services = self.services()?;
        let security = services
            .iter()
            .find(|s| s.is_wifi() && s.name == ssid)
            .map(|s| WifiSecurity::from_connman(s.wifi_security()));
        let key = match security {
            Some(WifiSecurity::Open) => "",
            Some(WifiSecurity::Enterprise) => {
                return Err(Error::Unsupported(format!(
                    "{ssid} needs a username and certificate (802.1X), which Batocera's Wi-Fi \
                     settings cannot set up"
                )));
            }
            _ => passphrase.unwrap_or(""),
        };
        check_passphrase(key, security)?;

        let _changing = self.changing.lock().unwrap_or_else(PoisonError::into_inner);
        let country = batocera_conf(&self.batocera_conf)
            .remove("wifi.country")
            .unwrap_or_default();
        let cable = services.iter().any(|s| s.is_ethernet() && s.connected);
        self.shell
            .run("batocera-wifi", &["enable", ssid, key, &country])?;
        if cable {
            return Ok(Joined::Saved);
        }

        let deadline = Instant::now() + self.join_timeout;
        loop {
            std::thread::sleep(self.poll_every);
            // connman has just restarted: an error here means "not yet", not "failed".
            let services = self.services().unwrap_or_default();
            if let Some(service) = services.iter().find(|s| s.is_wifi() && s.name == ssid) {
                if service.connected {
                    return Ok(Joined::Connected);
                }
                let state = self
                    .details(&service.id)
                    .ok()
                    .and_then(|p| p.get("State").cloned());
                if state.as_deref() == Some("failure") {
                    return Err(Error::Network(format!(
                        "{ssid} refused the connection: check the password"
                    )));
                }
            }
            if Instant::now() >= deadline {
                return Err(Error::Network(format!(
                    "{ssid} did not answer in {} seconds. It stays saved, and the box joins it \
                     when it is in range",
                    self.join_timeout.as_secs()
                )));
            }
        }
    }

    /// Batocera's `wifi.enabled`. Turning it on joins the saved network, if there is one.
    pub fn wifi_set_enabled(&self, enabled: bool) -> Result<()> {
        let _changing = self.changing.lock().unwrap_or_else(PoisonError::into_inner);
        if enabled {
            self.shell.run("batocera-wifi", &["enable"])?;
        } else {
            // `batocera-wifi disable` does the same, then waits for every address to go: with a
            // cable plugged in, that is ten seconds for nothing.
            self.shell
                .run("batocera-settings-set", &["wifi.enabled", "0"])?;
            self.shell.run(CONNMAN_INIT, &["reload"])?;
        }
        Ok(())
    }

    /// Saves how the wired interface gets its address, and restarts connman to apply it.
    pub fn ethernet_configure(&self, config: &EthernetConfig) -> Result<()> {
        let config = check_ethernet(config)?;
        let service = self
            .services()?
            .into_iter()
            .find(ServiceLine::is_ethernet)
            .ok_or_else(|| Error::Unsupported("this box has no wired network interface".into()))?;
        let mac = connman::dict(self.details(&service.id)?.get("Ethernet").map_or("", |s| s))
            .remove("Address")
            .ok_or_else(|| {
                Error::Network("connman did not say the wired interface's address".into())
            })?;
        let _changing = self.changing.lock().unwrap_or_else(PoisonError::into_inner);
        provision::write(&self.provisioning, &mac, &config)?;
        self.shell.run(CONNMAN_INIT, &["restart"])?;
        Ok(())
    }

    fn connmanctl(&self, args: &[&str]) -> Result<String> {
        let out = self.shell.run("connmanctl", args)?;
        match connman::error(&out) {
            Some(e) => Err(Error::Network(format!("connman: {e}"))),
            None => Ok(out),
        }
    }

    fn services(&self) -> Result<Vec<ServiceLine>> {
        Ok(connman::services(&self.connmanctl(&["services"])?))
    }

    fn details(&self, id: &str) -> Result<std::collections::HashMap<String, String>> {
        Ok(connman::properties(&self.connmanctl(&["services", id])?))
    }
}

fn ipv4(props: &std::collections::HashMap<String, String>) -> Option<Ipv4> {
    let mut ip = connman::dict(props.get("IPv4")?);
    Some(Ipv4 {
        address: ip.remove("Address")?,
        netmask: ip.remove("Netmask").unwrap_or_default(),
        gateway: ip.remove("Gateway"),
    })
}

/// WPA wants 8–63 characters or 64 hex digits; WEP 5 or 13 characters, or 10 or 26 hex digits.
/// An unknown network (hidden, or out of range) may be either.
fn check_passphrase(key: &str, security: Option<WifiSecurity>) -> Result<()> {
    if key.contains(char::is_control) {
        return Err(Error::Invalid(
            "the password has a character Wi-Fi does not allow".into(),
        ));
    }
    let hex = |n: usize| key.len() == n && key.chars().all(|c| c.is_ascii_hexdigit());
    let wpa = (8..=63).contains(&key.chars().count()) || hex(64);
    let wep = matches!(key.len(), 5 | 13) || hex(10) || hex(26);
    let ok = match security {
        Some(WifiSecurity::Open) => true,
        Some(WifiSecurity::Psk) => wpa,
        Some(WifiSecurity::Wep) => wep,
        Some(WifiSecurity::Enterprise) => false,
        None => key.is_empty() || wpa || wep,
    };
    if ok {
        Ok(())
    } else if key.is_empty() {
        Err(Error::Invalid("this network needs a password".into()))
    } else {
        Err(Error::Invalid(
            "a Wi-Fi password is 8 to 63 characters".into(),
        ))
    }
}

/// A static configuration with clean, valid addresses, or why not.
fn check_ethernet(config: &EthernetConfig) -> Result<EthernetConfig> {
    let EthernetConfig::Manual {
        address,
        netmask,
        gateway,
        nameservers,
    } = config
    else {
        return Ok(EthernetConfig::Dhcp);
    };
    let parse = |what: &str, s: &str| {
        s.trim().parse::<Ipv4Addr>().map_err(|_| {
            Error::Invalid(format!("the {what} {:?} is not an IPv4 address", s.trim()))
        })
    };
    let ip = parse("address", address)?;
    let mask = parse("netmask", netmask)?;
    let bits = u32::from(mask);
    if bits == 0 || bits.leading_ones() + bits.trailing_zeros() != 32 {
        return Err(Error::Invalid(format!("{mask} is not a netmask")));
    }
    let gateway = match gateway.as_deref().map(str::trim).filter(|g| !g.is_empty()) {
        None => None,
        Some(g) => {
            let gw = parse("gateway", g)?;
            if u32::from(gw) & bits != u32::from(ip) & bits {
                return Err(Error::Invalid(format!(
                    "the gateway {gw} is not on the same network as {ip}/{mask}"
                )));
            }
            Some(gw.to_string())
        }
    };
    let nameservers = nameservers
        .iter()
        .map(|n| n.trim())
        .filter(|n| !n.is_empty())
        .map(|n| parse("DNS server", n).map(|a| a.to_string()))
        .collect::<Result<Vec<_>>>()?;
    if nameservers.len() > 3 {
        return Err(Error::Invalid("at most three DNS servers".into()));
    }
    Ok(EthernetConfig::Manual {
        address: ip.to_string(),
        netmask: mask.to_string(),
        gateway,
        nameservers,
    })
}
