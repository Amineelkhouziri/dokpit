use bollard::models::{ContainerSummary, EndpointSettings, Network};

#[derive(Clone, Debug, PartialEq)]
pub struct NetRow {
    pub id: String,
    pub name: String,
    pub driver: String,
    pub scope: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Member {
    pub id: String,
    pub name: String,
    pub ip: String,
}

pub fn rows(nets: Vec<Network>) -> Vec<NetRow> {
    let mut rows: Vec<NetRow> = nets
        .into_iter()
        .map(|n| NetRow {
            id: n.id.unwrap_or_default(),
            name: n.name.unwrap_or_default(),
            driver: n.driver.unwrap_or_default(),
            scope: n.scope.unwrap_or_default(),
        })
        .collect();
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    rows
}

pub fn matches(row: &NetRow, query: &str) -> bool {
    let q = query.to_lowercase();
    [&row.name, &row.driver, &row.scope].iter().any(|f| f.to_lowercase().contains(&q))
}

fn endpoint<'a>(c: &'a ContainerSummary, network: &str) -> Option<&'a EndpointSettings> {
    c.network_settings.as_ref()?.networks.as_ref()?.get(network)
}

fn member(c: &ContainerSummary, ip: Option<&String>) -> Member {
    let id = c.id.clone().unwrap_or_default();
    let name = c
        .names
        .as_ref()
        .and_then(|n| n.first())
        .map(|n| n.trim_start_matches('/').to_string())
        .unwrap_or_else(|| id.chars().take(12).collect());
    Member { id, name, ip: ip.cloned().unwrap_or_default() }
}

fn sorted(mut members: Vec<Member>) -> Vec<Member> {
    members.sort_by(|a, b| a.name.cmp(&b.name));
    members
}

pub fn attached(containers: &[ContainerSummary], network: &str) -> Vec<Member> {
    sorted(
        containers
            .iter()
            .filter_map(|c| endpoint(c, network).map(|e| member(c, e.ip_address.as_ref())))
            .collect(),
    )
}

pub fn connectable(containers: &[ContainerSummary], network: &str) -> Vec<Member> {
    sorted(
        containers
            .iter()
            .filter(|c| endpoint(c, network).is_none())
            .map(|c| member(c, None))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use bollard::models::ContainerSummaryNetworkSettings;

    use super::*;

    fn container(name: &str, nets: &[(&str, &str)]) -> ContainerSummary {
        let networks: HashMap<_, _> = nets
            .iter()
            .map(|(n, ip)| (n.to_string(), EndpointSettings { ip_address: Some(ip.to_string()), ..Default::default() }))
            .collect();
        ContainerSummary {
            id: Some(format!("id-{name}")),
            names: Some(vec![format!("/{name}")]),
            network_settings: Some(ContainerSummaryNetworkSettings { networks: Some(networks) }),
            ..Default::default()
        }
    }

    fn member(name: &str, ip: &str) -> Member {
        Member { id: format!("id-{name}"), name: name.into(), ip: ip.into() }
    }

    fn net(name: &str, driver: &str) -> Network {
        Network {
            id: Some(format!("id-{name}")),
            name: Some(name.into()),
            driver: Some(driver.into()),
            scope: Some("local".into()),
            ..Default::default()
        }
    }

    #[test]
    fn rows_are_sorted_by_name() {
        let got = rows(vec![net("web", "bridge"), net("bridge", "bridge"), net("host", "host")]);
        let names: Vec<_> = got.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["bridge", "host", "web"]);
        assert_eq!(
            got[1],
            NetRow { id: "id-host".into(), name: "host".into(), driver: "host".into(), scope: "local".into() }
        );
    }

    #[test]
    fn filter_matches_name_driver_or_scope_ignoring_case() {
        let rows = rows(vec![net("ctoc_default", "bridge"), net("host", "host")]);
        let hits = |q: &str| rows.iter().filter(|r| matches(r, q)).map(|r| r.name.clone()).collect::<Vec<_>>();
        assert_eq!(hits(""), ["ctoc_default", "host"]);
        assert_eq!(hits("CTOC"), ["ctoc_default"]);
        assert_eq!(hits("bri"), ["ctoc_default"]);
        assert_eq!(hits("LOCAL"), ["ctoc_default", "host"]);
        assert!(hits("zzz").is_empty());
    }

    #[test]
    fn attached_lists_containers_on_the_network_sorted_by_name() {
        let cs = [
            container("web", &[("app_net", "172.18.0.3"), ("bridge", "172.17.0.2")]),
            container("db", &[("app_net", "")]),
            container("other", &[("bridge", "172.17.0.4")]),
            ContainerSummary::default(),
        ];
        assert_eq!(attached(&cs, "app_net"), [member("db", ""), member("web", "172.18.0.3")]);
    }

    #[test]
    fn connectable_lists_the_other_containers_by_name() {
        let cs = [
            container("web", &[("app_net", "172.18.0.3")]),
            container("zeta", &[("bridge", "172.17.0.4")]),
            container("alpha", &[]),
        ];
        assert_eq!(connectable(&cs, "app_net"), [member("alpha", ""), member("zeta", "")]);
    }
}
