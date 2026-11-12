use std::collections::VecDeque;

use bollard::models::{ContainerInspectResponse, ContainerStatsResponse, PortMap};

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Delta {
    pub removed: usize,
    pub added: String,
}

pub struct LogBuffer {
    lines: VecDeque<String>,
    partial: String,
    cap: usize,
}

impl LogBuffer {
    pub fn new(cap: usize) -> Self {
        Self { lines: VecDeque::new(), partial: String::new(), cap }
    }

    pub fn push(&mut self, chunk: &str, query: &str) -> Delta {
        self.partial.push_str(chunk);
        let Some(end) = self.partial.rfind('\n') else { return Delta::default() };
        let rest = self.partial.split_off(end + 1);
        let complete = std::mem::replace(&mut self.partial, rest);
        let query = query.trim().to_lowercase();
        let new: Vec<&str> = complete.lines().collect();
        let overflow = (self.lines.len() + new.len()).saturating_sub(self.cap);
        let evict = overflow.min(self.lines.len());
        let new = &new[overflow - evict..];
        let removed = self.lines.drain(..evict).filter(|l| matches(l, &query)).count();
        let added = new.iter().filter(|l| matches(l, &query)).map(|l| format!("{l}\n")).collect();
        self.lines.extend(new.iter().map(|l| l.to_string()));
        Delta { removed, added }
    }

    pub fn text(&self, query: &str) -> String {
        let query = query.trim().to_lowercase();
        self.lines.iter().filter(|l| matches(l, &query)).map(|l| format!("{l}\n")).collect()
    }
}

pub struct Section {
    pub title: &'static str,
    pub rows: Vec<(String, String)>,
}

pub fn info_sections(c: &ContainerInspectResponse) -> Vec<Section> {
    let live = c.network_settings.as_ref().and_then(|n| n.ports.as_ref()).filter(|p| !p.is_empty());
    let bound = c.host_config.as_ref().and_then(|h| h.port_bindings.as_ref());
    let env = c.config.as_ref().and_then(|c| c.env.as_ref()).into_iter().flatten().map(|e| match e.split_once('=') {
        Some((k, v)) => (k.to_string(), v.to_string()),
        None => (e.clone(), String::new()),
    });
    let mounts = c.mounts.iter().flatten().map(|m| {
        let source = match (m.typ.as_deref(), &m.name) {
            (Some("volume"), Some(name)) => format!("volume {name}"),
            _ => m.source.clone().unwrap_or_default(),
        };
        let ro = if m.rw == Some(false) { " (read-only)" } else { "" };
        (m.destination.clone().unwrap_or_default(), format!("{source}{ro}"))
    });
    let mut networks: Vec<_> = c
        .network_settings
        .as_ref()
        .and_then(|n| n.networks.as_ref())
        .into_iter()
        .flatten()
        .map(|(name, e)| {
            let ip = e.ip_address.clone().filter(|ip| !ip.is_empty()).unwrap_or_else(|| "no IP".into());
            (name.clone(), ip)
        })
        .collect();
    networks.sort();
    vec![
        Section { title: "Ports", rows: live.or(bound).map(port_rows).unwrap_or_default() },
        Section { title: "Environment", rows: env.collect() },
        Section { title: "Mounts", rows: mounts.collect() },
        Section { title: "Networks", rows: networks },
    ]
}

fn port_rows(ports: &PortMap) -> Vec<(String, String)> {
    let mut rows: Vec<_> = ports
        .iter()
        .map(|(port, binds)| {
            let mut hosts: Vec<String> = binds
                .iter()
                .flatten()
                .map(|b| {
                    let host_port = b.host_port.as_deref().unwrap_or_default();
                    match b.host_ip.as_deref() {
                        None | Some("" | "0.0.0.0" | "::") => host_port.to_string(),
                        Some(ip) => format!("{ip}:{host_port}"),
                    }
                })
                .collect();
            hosts.sort();
            hosts.dedup();
            let value = if hosts.is_empty() { "not published".into() } else { hosts.join(", ") };
            (port.clone(), value)
        })
        .collect();
    rows.sort_by_key(|(port, _)| (port.split('/').next().and_then(|n| n.parse::<u16>().ok()), port.clone()));
    rows
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Net {
    pub rx: u64,
    pub tx: u64,
}

impl Net {
    pub fn rate(self, prev: Net, secs: f64) -> (f64, f64) {
        if secs <= 0.0 {
            return (0.0, 0.0);
        }
        let per_sec = |now: u64, before: u64| now.saturating_sub(before) as f64 / secs;
        (per_sec(self.rx, prev.rx), per_sec(self.tx, prev.tx))
    }
}

pub fn net_totals(stats: &ContainerStatsResponse) -> Net {
    stats.networks.iter().flatten().fold(Net::default(), |acc, (_, n)| Net {
        rx: acc.rx + n.rx_bytes.unwrap_or(0),
        tx: acc.tx + n.tx_bytes.unwrap_or(0),
    })
}

pub fn format_bytes(bytes: u64) -> String {
    format_size(bytes as f64)
}

pub fn format_rate(bytes_per_sec: f64) -> String {
    format!("{}/s", format_size(bytes_per_sec.max(0.0)))
}

fn format_size(bytes: f64) -> String {
    const UNITS: [&str; 4] = ["KiB", "MiB", "GiB", "TiB"];
    if bytes < 1024.0 {
        return format!("{} B", bytes.round());
    }
    let mut value = bytes / 1024.0;
    let mut unit = 0;
    while (value * 10.0).round() >= 10240.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

fn matches(line: &str, query: &str) -> bool {
    query.is_empty() || line.to_lowercase().contains(query)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bollard::models::{
        ContainerConfig, ContainerInspectResponse, ContainerNetworkStats, EndpointSettings, HostConfig, MountPoint,
        NetworkSettings, PortBinding,
    };

    #[test]
    fn keeps_only_the_newest_lines_up_to_the_cap() {
        let mut logs = LogBuffer::new(3);
        logs.push("a\nb\n", "");
        logs.push("c\nd\ne\n", "");
        assert_eq!(logs.text(""), "c\nd\ne\n");
    }

    #[test]
    fn holds_a_partial_line_until_its_newline_arrives() {
        let mut logs = LogBuffer::new(10);
        logs.push("one\ntw", "");
        assert_eq!(logs.text(""), "one\n");
        logs.push("o\r\nthree\n", "");
        assert_eq!(logs.text(""), "one\ntwo\nthree\n");
    }

    #[test]
    fn search_keeps_matching_lines_case_insensitively() {
        let mut logs = LogBuffer::new(10);
        logs.push("GET /ok 200\nERROR db down\nretrying\nerror: timeout\n", "");
        assert_eq!(logs.text("error"), "ERROR db down\nerror: timeout\n");
        assert_eq!(logs.text(" Timeout "), "error: timeout\n");
        assert_eq!(logs.text("nothing"), "");
    }

    #[test]
    fn push_reports_shown_lines_evicted_and_added() {
        let mut logs = LogBuffer::new(3);
        logs.push("x1\ny1\nx2\n", "x");
        let delta = logs.push("y2\nx3\n", "x");
        assert_eq!(delta, Delta { removed: 1, added: "x3\n".into() });
        assert_eq!(logs.text("x"), "x2\nx3\n");
    }

    #[test]
    fn push_larger_than_cap_drops_old_lines_and_reports_only_survivors() {
        let mut logs = LogBuffer::new(3);
        logs.push("a\nb\n", "");
        let delta = logs.push("c\nd\ne\nf\n", "");
        assert_eq!(delta, Delta { removed: 2, added: "d\ne\nf\n".into() });
        assert_eq!(logs.text(""), "d\ne\nf\n");
    }

    #[test]
    fn bytes_use_binary_units_with_one_decimal() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(1536), "1.5 KiB");
        assert_eq!(format_bytes(12_897_484), "12.3 MiB");
        assert_eq!(format_bytes(8_160_437_862), "7.6 GiB");
    }

    #[test]
    fn byte_rounding_rolls_over_to_next_unit() {
        assert_eq!(format_bytes(1_048_575), "1.0 MiB");
    }

    #[test]
    fn rates_are_per_second_and_never_negative() {
        assert_eq!(format_rate(0.0), "0 B/s");
        assert_eq!(format_rate(1536.0), "1.5 KiB/s");
        assert_eq!(format_rate(-5.0), "0 B/s");
    }

    fn net_sample(ifaces: &[(&str, u64, u64)]) -> ContainerStatsResponse {
        let networks = ifaces
            .iter()
            .map(|&(name, rx, tx)| {
                let stats = ContainerNetworkStats { rx_bytes: Some(rx), tx_bytes: Some(tx), ..Default::default() };
                (name.to_string(), stats)
            })
            .collect();
        ContainerStatsResponse { networks: Some(networks), ..Default::default() }
    }

    #[test]
    fn network_totals_sum_every_interface() {
        let s = net_sample(&[("eth0", 1000, 200), ("eth1", 500, 50)]);
        assert_eq!(net_totals(&s), Net { rx: 1500, tx: 250 });
        assert_eq!(net_totals(&ContainerStatsResponse::default()), Net::default());
    }

    #[test]
    fn network_rate_is_delta_over_elapsed_seconds_and_zero_after_counter_reset() {
        let prev = Net { rx: 1000, tx: 500 };
        assert_eq!(Net { rx: 4000, tx: 1500 }.rate(prev, 2.0), (1500.0, 500.0));
        assert_eq!(Net { rx: 10, tx: 10 }.rate(prev, 1.0), (0.0, 0.0));
        assert_eq!(Net { rx: 4000, tx: 1500 }.rate(prev, 0.0), (0.0, 0.0));
    }

    fn bind(ip: &str, port: &str) -> PortBinding {
        PortBinding { host_ip: Some(ip.into()), host_port: Some(port.into()) }
    }

    fn rows(section: &Section) -> Vec<(&str, &str)> {
        section.rows.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect()
    }

    #[test]
    fn ports_dedupe_wildcard_ips_sort_numerically_and_show_unpublished() {
        let ports = [
            ("8443/tcp".to_string(), Some(vec![bind("127.0.0.1", "8443")])),
            ("80/tcp".to_string(), Some(vec![bind("0.0.0.0", "8080"), bind("::", "8080")])),
            ("9000/tcp".to_string(), None),
        ];
        let inspect = ContainerInspectResponse {
            network_settings: Some(NetworkSettings { ports: Some(ports.into_iter().collect()), ..Default::default() }),
            ..Default::default()
        };
        assert_eq!(
            rows(&info_sections(&inspect)[0]),
            [("80/tcp", "8080"), ("8443/tcp", "127.0.0.1:8443"), ("9000/tcp", "not published")]
        );
    }

    #[test]
    fn stopped_container_ports_come_from_host_config_bindings() {
        let bindings = [("80/tcp".to_string(), Some(vec![bind("", "8080")]))];
        let inspect = ContainerInspectResponse {
            host_config: Some(HostConfig { port_bindings: Some(bindings.into_iter().collect()), ..Default::default() }),
            network_settings: Some(NetworkSettings { ports: Some(Default::default()), ..Default::default() }),
            ..Default::default()
        };
        let ports = &info_sections(&inspect)[0];
        assert_eq!(ports.title, "Ports");
        assert_eq!(rows(ports), [("80/tcp", "8080")]);
    }

    #[test]
    fn env_mounts_and_networks_sections() {
        let mount = |typ: &str, name: Option<&str>, source: &str, dest: &str, rw: bool| MountPoint {
            typ: Some(typ.into()),
            name: name.map(Into::into),
            source: Some(source.into()),
            destination: Some(dest.into()),
            rw: Some(rw),
            ..Default::default()
        };
        let endpoint = |ip: &str| EndpointSettings { ip_address: Some(ip.into()), ..Default::default() };
        let inspect = ContainerInspectResponse {
            config: Some(ContainerConfig {
                env: Some(vec!["PATH=/usr/bin".into(), "URL=a=b".into(), "EMPTY".into()]),
                ..Default::default()
            }),
            mounts: Some(vec![
                mount("volume", Some("pgdata"), "/var/lib/docker/volumes/pgdata/_data", "/var/lib/postgresql", true),
                mount("bind", None, "/home/me/conf", "/etc/app", false),
            ]),
            network_settings: Some(NetworkSettings {
                networks: Some([("web".to_string(), endpoint("")), ("bridge".to_string(), endpoint("172.17.0.2"))].into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let sections = info_sections(&inspect);
        let titles: Vec<_> = sections.iter().map(|s| s.title).collect();
        assert_eq!(titles, ["Ports", "Environment", "Mounts", "Networks"]);
        assert_eq!(rows(&sections[1]), [("PATH", "/usr/bin"), ("URL", "a=b"), ("EMPTY", "")]);
        assert_eq!(
            rows(&sections[2]),
            [("/var/lib/postgresql", "volume pgdata"), ("/etc/app", "/home/me/conf (read-only)")]
        );
        assert_eq!(rows(&sections[3]), [("bridge", "172.17.0.2"), ("web", "no IP")]);
    }
}
