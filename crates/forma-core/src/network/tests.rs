use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::Mutex;

use super::*;

/// Answers commands from `fixtures/connman`, or from answers queued by the test, and records
/// every command it was given.
struct FakeShell {
    answers: Mutex<HashMap<String, VecDeque<String>>>,
    ran: Mutex<Vec<String>>,
}

impl FakeShell {
    fn new() -> Self {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/connman");
        let shell = Self {
            answers: Mutex::new(HashMap::new()),
            ran: Mutex::new(Vec::new()),
        };
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let stem = path.file_stem().unwrap().to_str().unwrap().to_owned();
            let text = std::fs::read_to_string(&path).unwrap();
            let command = match stem.as_str() {
                "technologies" | "services" => format!("connmanctl {stem}"),
                id => format!("connmanctl services {id}"),
            };
            shell.answer(&command, &text);
        }
        shell.answer("connmanctl scan wifi", "Scan completed for wifi\n");
        shell
    }

    /// Queues an answer; the last one queued keeps being given.
    fn answer(&self, command: &str, output: &str) {
        self.answers
            .lock()
            .unwrap()
            .entry(command.to_owned())
            .or_default()
            .push_back(output.to_owned());
    }

    /// Replaces every answer to `command`.
    fn set(&self, command: &str, output: &str) {
        self.answers.lock().unwrap().remove(command);
        self.answer(command, output);
    }

    fn ran(&self) -> Vec<String> {
        self.ran.lock().unwrap().clone()
    }
}

impl Shell for &'static FakeShell {
    fn run(&self, program: &str, args: &[&str]) -> Result<String> {
        let command = std::iter::once(program)
            .chain(args.iter().copied())
            .collect::<Vec<_>>()
            .join(" ");
        self.ran.lock().unwrap().push(command.clone());
        let mut answers = self.answers.lock().unwrap();
        match answers.get_mut(&command) {
            Some(queue) if queue.len() > 1 => Ok(queue.pop_front().unwrap()),
            Some(queue) => Ok(queue.front().cloned().unwrap_or_default()),
            // Anything that changes the box answers with nothing, as the real tools do.
            None if !program.starts_with("connmanctl") => Ok(String::new()),
            None => Ok(format!("Error {command}: no such service\n")),
        }
    }
}

struct Setup {
    shell: &'static FakeShell,
    network: Network,
    dir: tempfile::TempDir,
}

/// A network over the fixtures, with `batocera.conf` and the provisioning file in a temp dir.
fn setup(conf: &str) -> Setup {
    let shell: &'static FakeShell = Box::leak(Box::new(FakeShell::new()));
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths {
        userdata: dir.path().to_path_buf(),
        systems_cfg: dir.path().join("es_systems.cfg"),
    };
    std::fs::create_dir_all(dir.path().join("system")).unwrap();
    std::fs::write(paths.batocera_conf(), conf).unwrap();
    let mut network = Network::new(Box::new(shell), &paths);
    network.join_timeout = Duration::from_millis(50);
    network.poll_every = Duration::ZERO;
    Setup {
        shell,
        network,
        dir,
    }
}

const SAVED_HOME: &str = "wifi.enabled=1\nwifi.ssid=Home\nwifi.key=secret-key\nwifi.country=PT\n";
const HOME: &str = "wifi_020000000002_486f6d65_managed_psk";
const NO_CABLE: &str = "    Wired                ethernet_020000000001_cable\n    \
                        Home                 wifi_020000000002_486f6d65_managed_psk\n";

impl Setup {
    fn provisioning(&self) -> PathBuf {
        self.dir.path().join("system/network-connman.config")
    }
}

#[test]
fn status_reads_both_interfaces() {
    let s = setup(SAVED_HOME);
    let status = s.network.status().unwrap();
    let ethernet = status.ethernet.unwrap();
    assert!(ethernet.connected);
    assert_eq!(
        ethernet.ipv4,
        Some(Ipv4 {
            address: "192.168.1.113".into(),
            netmask: "255.255.255.0".into(),
            gateway: Some("192.168.1.254".into()),
        })
    );
    assert_eq!(ethernet.nameservers, ["192.168.1.254"]);
    assert_eq!(ethernet.config, EthernetConfig::Dhcp);
    let wifi = status.wifi.unwrap();
    assert!(wifi.enabled);
    assert_eq!(wifi.connected, None);
    assert_eq!(wifi.saved.as_deref(), Some("Home"));
}

#[test]
fn status_without_wifi_hardware_or_settings() {
    let s = setup("");
    s.shell.set(
        "connmanctl technologies",
        "/net/connman/technology/ethernet\n  Type = ethernet\n  Powered = True\n",
    );
    let status = s.network.status().unwrap();
    assert!(status.wifi.is_none());
    assert!(status.ethernet.is_some());
}

#[test]
fn status_names_the_wifi_network_in_use() {
    let s = setup(SAVED_HOME);
    s.shell.set(
        "connmanctl services",
        "*AR Home                 wifi_020000000002_486f6d65_managed_psk\n",
    );
    s.shell.set(
        &format!("connmanctl services {HOME}"),
        "  State = ready\n  IPv4 = [ Method=dhcp, Address=10.0.0.7, Netmask=255.0.0.0 ]\n",
    );
    let wifi = s.network.status().unwrap().wifi.unwrap();
    assert_eq!(wifi.connected.as_deref(), Some("Home"));
    assert_eq!(wifi.ipv4.unwrap().address, "10.0.0.7");
    let ethernet = s.network.status().unwrap().ethernet.unwrap();
    assert!(!ethernet.connected, "no Ethernet service: no cable");
}

#[test]
fn scan_lists_each_network_once_saved_first_then_by_strength() {
    let s = setup(SAVED_HOME);
    let found = s.network.wifi_scan().unwrap();
    let names: Vec<_> = found.iter().map(|n| n.ssid.as_str()).collect();
    assert_eq!(names, ["Home", "Cafe Guest", "Office 5G", "Campus"]);
    assert_eq!(found[0].strength, Some(94), "the stronger radio of the two");
    assert!(found[0].saved);
    assert_eq!(found[0].security, WifiSecurity::Psk);
    assert_eq!(found[1].security, WifiSecurity::Open);
    assert_eq!(found[3].security, WifiSecurity::Enterprise);
    assert!(s.shell.ran().contains(&"connmanctl scan wifi".to_owned()));
}

#[test]
fn scan_is_empty_with_wifi_off_and_refused_without_hardware() {
    let s = setup("");
    let off = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/connman/technologies.txt"),
    )
    .unwrap()
    .replace(
        "Type = wifi\n  Powered = True",
        "Type = wifi\n  Powered = False",
    );
    s.shell.set("connmanctl technologies", &off);
    assert!(s.network.wifi_scan().unwrap().is_empty());
    assert!(!s.shell.ran().contains(&"connmanctl scan wifi".to_owned()));

    s.shell.set("connmanctl technologies", "");
    assert!(matches!(
        s.network.wifi_scan().unwrap_err(),
        Error::Unsupported(_)
    ));
}

#[test]
fn joining_with_a_cable_in_saves_through_batocera_and_keeps_ethernet() {
    let s = setup(SAVED_HOME);
    assert_eq!(
        s.network.wifi_join("Office 5G", Some("hunter22")).unwrap(),
        Joined::Saved
    );
    assert!(
        s.shell
            .ran()
            .contains(&"batocera-wifi enable Office 5G hunter22 PT".to_owned()),
        "the country is kept: {:?}",
        s.shell.ran()
    );
}

#[test]
fn joining_without_a_cable_waits_for_the_network() {
    let s = setup(SAVED_HOME);
    s.shell.set("connmanctl services", NO_CABLE);
    s.shell.answer(
        "connmanctl services",
        "*AR Home                 wifi_020000000002_486f6d65_managed_psk\n",
    );
    assert_eq!(
        s.network.wifi_join("Home", Some("correct horse")).unwrap(),
        Joined::Connected
    );
}

#[test]
fn a_refused_password_is_reported_and_a_silent_network_times_out() {
    let s = setup(SAVED_HOME);
    s.shell.set("connmanctl services", NO_CABLE);
    s.shell.set(
        &format!("connmanctl services {HOME}"),
        "  State = failure\n",
    );
    let err = s.network.wifi_join("Home", Some("wrong pass")).unwrap_err();
    assert!(err.to_string().contains("check the password"), "{err}");
    assert!(
        !err.to_string().contains("wrong pass"),
        "never echoes the password"
    );

    s.shell.set(
        &format!("connmanctl services {HOME}"),
        "  State = association\n",
    );
    let err = s
        .network
        .wifi_join("Home", Some("correct horse"))
        .unwrap_err();
    assert!(err.to_string().contains("stays saved"), "{err}");
}

#[test]
fn passwords_and_names_are_checked_before_anything_changes() {
    let s = setup(SAVED_HOME);
    for (ssid, key) in [
        ("Home", None),
        ("Home", Some("short")),
        ("Home", Some("line\nbreak")),
        ("", Some("whatever-long")),
        ("x".repeat(33).as_str(), Some("whatever-long")),
    ] {
        let err = s.network.wifi_join(ssid, key).unwrap_err();
        assert!(matches!(err, Error::Invalid(_)), "{ssid:?} {key:?}: {err}");
    }
    assert!(matches!(
        s.network
            .wifi_join("Campus", Some("whatever-long"))
            .unwrap_err(),
        Error::Unsupported(_)
    ));
    assert!(
        !s.shell.ran().iter().any(|c| c.starts_with("batocera-wifi")),
        "nothing was saved"
    );
    // An open network needs no password; one Forma cannot see may be anything sensible.
    s.network.wifi_join("Cafe Guest", None).unwrap();
    assert!(
        s.shell
            .ran()
            .contains(&"batocera-wifi enable Cafe Guest  PT".to_owned())
    );
    s.network.wifi_join("Hidden Lab", Some("abcde")).unwrap();
}

#[test]
fn turning_wifi_off_skips_batocera_wifis_wait() {
    let s = setup(SAVED_HOME);
    s.network.wifi_set_enabled(false).unwrap();
    s.network.wifi_set_enabled(true).unwrap();
    assert_eq!(
        s.shell.ran(),
        [
            "batocera-settings-set wifi.enabled 0",
            "/etc/init.d/S08connman reload",
            "batocera-wifi enable",
        ]
    );
}

#[test]
fn a_static_address_is_provisioned_for_the_cable_and_dhcp_removes_it() {
    let s = setup(SAVED_HOME);
    let file = s.provisioning();
    std::fs::write(&file, "[service_mine]\nType=wifi\nName=Mine\n").unwrap();

    let manual = EthernetConfig::Manual {
        address: " 192.168.1.50 ".into(),
        netmask: "255.255.255.0".into(),
        gateway: Some("192.168.1.254".into()),
        nameservers: vec!["1.1.1.1".into(), "".into()],
    };
    s.network.ethernet_configure(&manual).unwrap();
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(
        text.starts_with("[service_mine]\nType=wifi\nName=Mine\n\n"),
        "{text}"
    );
    assert!(
        text.contains("MAC=02:00:00:00:00:01\nIPv4=192.168.1.50/255.255.255.0/192.168.1.254\n")
    );
    assert!(text.contains("Nameservers=1.1.1.1\n"));
    assert_eq!(
        s.shell.ran().last().unwrap(),
        "/etc/init.d/S08connman restart"
    );

    let status = s.network.status().unwrap().ethernet.unwrap();
    assert_eq!(
        status.config,
        EthernetConfig::Manual {
            address: "192.168.1.50".into(),
            netmask: "255.255.255.0".into(),
            gateway: Some("192.168.1.254".into()),
            nameservers: vec!["1.1.1.1".into()],
        }
    );

    // Saving again replaces Forma's section rather than adding a second one.
    s.network.ethernet_configure(&manual).unwrap();
    let text = std::fs::read_to_string(&file).unwrap();
    assert_eq!(text.matches("[service_forma_ethernet]").count(), 1);

    s.network.ethernet_configure(&EthernetConfig::Dhcp).unwrap();
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "[service_mine]\nType=wifi\nName=Mine\n",
        "the user's own section is untouched"
    );
}

#[test]
fn dhcp_with_nothing_else_in_the_file_removes_it() {
    let s = setup("");
    let manual = EthernetConfig::Manual {
        address: "10.0.0.2".into(),
        netmask: "255.255.255.0".into(),
        gateway: None,
        nameservers: vec![],
    };
    s.network.ethernet_configure(&manual).unwrap();
    assert!(
        std::fs::read_to_string(s.provisioning())
            .unwrap()
            .contains("IPv4=10.0.0.2/255.255.255.0\n")
    );
    s.network.ethernet_configure(&EthernetConfig::Dhcp).unwrap();
    assert!(!s.provisioning().exists());
}

#[test]
fn static_addresses_are_checked_before_anything_changes() {
    let s = setup("");
    let config = |address: &str, netmask: &str, gateway: &str| EthernetConfig::Manual {
        address: address.into(),
        netmask: netmask.into(),
        gateway: Some(gateway.into()),
        nameservers: vec![],
    };
    for bad in [
        config("192.168.1.300", "255.255.255.0", ""),
        config("192.168.1.5", "255.0.255.0", ""),
        config("192.168.1.5", "0.0.0.0", ""),
        config("192.168.1.5", "255.255.255.0", "192.168.2.1"),
        EthernetConfig::Manual {
            address: "10.0.0.2".into(),
            netmask: "255.0.0.0".into(),
            gateway: None,
            nameservers: vec!["dns.example".into()],
        },
    ] {
        let err = s.network.ethernet_configure(&bad).unwrap_err();
        assert!(matches!(err, Error::Invalid(_)), "{bad:?}: {err}");
    }
    assert!(!s.provisioning().exists());
    assert!(!s.shell.ran().iter().any(|c| c.contains("S08connman")));
}

#[test]
fn connman_errors_are_errors_even_with_exit_code_zero() {
    let s = setup("");
    s.shell.set(
        "connmanctl technologies",
        "Error /net/connman: not running\n",
    );
    assert!(matches!(s.network.status().unwrap_err(), Error::Network(_)));
}

#[test]
fn the_ethernet_config_is_json_themes_can_send() {
    let manual: EthernetConfig = serde_json::from_str(
        r#"{"method":"manual","address":"10.0.0.2","netmask":"255.0.0.0","gateway":null}"#,
    )
    .unwrap();
    assert!(
        matches!(manual, EthernetConfig::Manual { ref nameservers, .. } if nameservers.is_empty())
    );
    let dhcp: EthernetConfig = serde_json::from_str(r#"{"method":"dhcp"}"#).unwrap();
    assert_eq!(dhcp, EthernetConfig::Dhcp);
}
