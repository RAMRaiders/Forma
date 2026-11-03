//! Bluetooth controllers, through Batocera's own `batocera-bluetooth`, as EmulationStation's
//! Controllers menu uses it: pairing finds and trusts every pad put in pairing mode within a
//! minute; paired pads can be connected, disconnected and forgotten; Bluetooth itself can be
//! turned off (`controllers.bluetooth.enabled`).

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::Stdio;

use serde::Serialize;

use crate::error::{Error, Result};
use crate::host::{Shell, command};
use crate::net::batocera_conf;
use crate::paths::Paths;

const TOOL: &str = "batocera-bluetooth";
const ENABLED_KEY: &str = "controllers.bluetooth.enabled";

/// A device Batocera has paired and trusts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BluetoothDevice {
    /// `AA:BB:CC:DD:EE:FF`.
    pub address: String,
    pub name: String,
    /// A gamepad or joystick, by its Bluetooth class; else something else (a headset…).
    pub controller: bool,
    pub connected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BluetoothStatus {
    /// Batocera's `controllers.bluetooth.enabled`.
    pub enabled: bool,
    pub devices: Vec<BluetoothDevice>,
}

pub struct Bluetooth {
    shell: Box<dyn Shell>,
    batocera_conf: PathBuf,
}

impl Bluetooth {
    pub fn new(shell: Box<dyn Shell>, paths: &Paths) -> Self {
        Self {
            shell,
            batocera_conf: paths.batocera_conf(),
        }
    }

    pub fn status(&self) -> Result<BluetoothStatus> {
        let enabled = batocera_conf(&self.batocera_conf)
            .get(ENABLED_KEY)
            .is_none_or(|v| v != "0");
        let devices = if enabled {
            parse_list(&self.shell.run(TOOL, &["list"])?)
        } else {
            Vec::new()
        };
        Ok(BluetoothStatus { enabled, devices })
    }

    /// Turns Bluetooth on or off: Batocera restarts its Bluetooth services.
    pub fn set_enabled(&self, enabled: bool) -> Result<()> {
        self.shell
            .run(TOOL, &[if enabled { "enable" } else { "disable" }])
            .map(drop)
    }

    pub fn connect(&self, address: &str) -> Result<()> {
        self.shell
            .run(TOOL, &["connect", checked(address)?])
            .map(drop)
    }

    pub fn disconnect(&self, address: &str) -> Result<()> {
        self.shell
            .run(TOOL, &["disconnect", checked(address)?])
            .map(drop)
    }

    /// Unpairs a device and forgets it.
    pub fn forget(&self, address: &str) -> Result<()> {
        self.shell
            .run(TOOL, &["remove", checked(address)?])
            .map(drop)
    }

    /// Looks for pads in pairing mode for a minute, pairing and trusting each it finds, and says
    /// what it is doing as it goes (`on_status`). Returns the devices it paired.
    pub fn pair(&self, mut on_status: impl FnMut(&str)) -> Result<Vec<BluetoothDevice>> {
        let before = self.status()?.devices;
        let mut child = command(TOOL)
            .args(["trust", "input"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| Error::Network(format!("could not run `{TOOL}`: {e}")))?;
        if let Some(out) = child.stdout.take() {
            for line in BufReader::new(out).lines().map_while(std::io::Result::ok) {
                let line = line.trim();
                if !line.is_empty() {
                    on_status(line);
                }
            }
        }
        let _ = child.wait();
        let after = self.status()?.devices;
        Ok(after
            .into_iter()
            .filter(|d| !before.iter().any(|b| b.address == d.address))
            .collect())
    }
}

/// An address as Batocera's script takes it, so nothing else reaches its command line.
fn checked(address: &str) -> Result<&str> {
    let ok = address.len() == 17
        && address.split(':').count() == 6
        && address
            .split(':')
            .all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_hexdigit()));
    if ok {
        Ok(address)
    } else {
        Err(Error::Invalid(format!(
            "`{address}` is not a Bluetooth address"
        )))
    }
}

/// `batocera-bluetooth list`: one `<device id="…" name="…" type="joystick" connected="yes" />`
/// per trusted device.
fn parse_list(output: &str) -> Vec<BluetoothDevice> {
    let attr = |line: &str, name: &str| -> Option<String> {
        let start = line.find(&format!(" {name}=\""))? + name.len() + 3;
        let end = line[start..]
            .find("\" ")
            .or_else(|| line[start..].rfind('"'))?;
        Some(line[start..start + end].to_owned())
    };
    output
        .lines()
        .filter(|l| l.trim_start().starts_with("<device"))
        .filter_map(|line| {
            let address = attr(line, "id")?;
            checked(&address).ok()?;
            Some(BluetoothDevice {
                name: attr(line, "name")
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| address.clone()),
                controller: attr(line, "type").as_deref() == Some("joystick"),
                connected: attr(line, "connected").as_deref() == Some("yes"),
                address,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct Fake {
        list: String,
        ran: Mutex<Vec<String>>,
    }

    impl Shell for &'static Fake {
        fn run(&self, program: &str, args: &[&str]) -> Result<String> {
            self.ran
                .lock()
                .unwrap()
                .push(format!("{program} {}", args.join(" ")));
            Ok(if args == ["list"] {
                self.list.clone()
            } else {
                String::new()
            })
        }
    }

    const LIST: &str = r#"<device id="00:06:F5:13:C3:50" name="PLAYSTATION(R)3 Controller" type="joystick" connected="no" />
<device id="9C:DE:B3:05:06:07" name="Wireless Controller" type="joystick" connected="yes" />
<device id="11:22:33:44:55:66" name="" type="" connected="no" />
"#;

    fn setup(conf: &str) -> (tempfile::TempDir, &'static Fake, Bluetooth) {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths {
            userdata: dir.path().into(),
            systems_cfg: dir.path().join("es_systems.cfg"),
        };
        std::fs::create_dir_all(paths.batocera_conf().parent().unwrap()).unwrap();
        std::fs::write(paths.batocera_conf(), conf).unwrap();
        let fake: &'static Fake = Box::leak(Box::new(Fake {
            list: LIST.into(),
            ran: Mutex::new(Vec::new()),
        }));
        let bt = Bluetooth::new(Box::new(fake), &paths);
        (dir, fake, bt)
    }

    #[test]
    fn lists_paired_devices_as_batocera_does() {
        let (_dir, _fake, bt) = setup("controllers.bluetooth.enabled=1\n");
        let status = bt.status().unwrap();
        assert!(status.enabled);
        assert_eq!(
            status.devices,
            [
                BluetoothDevice {
                    address: "00:06:F5:13:C3:50".into(),
                    name: "PLAYSTATION(R)3 Controller".into(),
                    controller: true,
                    connected: false,
                },
                BluetoothDevice {
                    address: "9C:DE:B3:05:06:07".into(),
                    name: "Wireless Controller".into(),
                    controller: true,
                    connected: true,
                },
                BluetoothDevice {
                    address: "11:22:33:44:55:66".into(),
                    name: "11:22:33:44:55:66".into(),
                    controller: false,
                    connected: false,
                },
            ]
        );
    }

    #[test]
    fn off_lists_nothing() {
        let (_dir, fake, bt) = setup("controllers.bluetooth.enabled=0\n");
        let status = bt.status().unwrap();
        assert!(!status.enabled && status.devices.is_empty());
        assert!(fake.ran.lock().unwrap().is_empty());
    }

    #[test]
    fn acts_on_a_device_only_by_a_real_address() {
        let (_dir, fake, bt) = setup("");
        bt.forget("9C:DE:B3:05:06:07").unwrap();
        bt.connect("9C:DE:B3:05:06:07").unwrap();
        bt.set_enabled(false).unwrap();
        assert_eq!(
            *fake.ran.lock().unwrap(),
            [
                "batocera-bluetooth remove 9C:DE:B3:05:06:07",
                "batocera-bluetooth connect 9C:DE:B3:05:06:07",
                "batocera-bluetooth disable",
            ]
        );
        for bad in [
            "input",
            "9C:DE:B3:05:06",
            "9C:DE:B3:05:06:0G",
            "x; rm -rf /",
        ] {
            assert!(bt.forget(bad).is_err(), "{bad}");
        }
    }
}
