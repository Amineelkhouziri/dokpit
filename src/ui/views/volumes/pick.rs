use bollard::models::Volume;

const ANONYMOUS: &str = "com.docker.volume.anonymous";

pub fn prune_candidates(dangling: Vec<Volume>) -> Vec<String> {
    let mut names: Vec<String> = dangling
        .into_iter()
        .filter(|v| v.driver == "local" && v.labels.contains_key(ANONYMOUS))
        .map(|v| v.name)
        .collect();
    names.sort();
    names
}

pub fn matches(v: &Volume, query: &str) -> bool {
    let query = query.to_lowercase();
    [&v.name, &v.driver, &v.mountpoint].iter().any(|f| f.to_lowercase().contains(&query))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vol(name: &str, driver: &str, anonymous: bool) -> Volume {
        let mut v = Volume { name: name.into(), driver: driver.into(), ..Default::default() };
        if anonymous {
            v.labels.insert("com.docker.volume.anonymous".into(), String::new());
        }
        v
    }

    #[test]
    fn prune_keeps_only_anonymous_local_volumes_sorted() {
        let dangling = vec![
            vol("b-anon", "local", true),
            vol("pgdata", "local", false),
            vol("plugin-anon", "rexray", true),
            vol("a-anon", "local", true),
        ];
        assert_eq!(prune_candidates(dangling), ["a-anon", "b-anon"]);
    }

    #[test]
    fn filter_matches_name_driver_or_mountpoint_ignoring_case() {
        let v = Volume {
            name: "Shop_pgdata".into(),
            driver: "local".into(),
            mountpoint: "/var/lib/docker/volumes/Shop_pgdata/_data".into(),
            ..Default::default()
        };
        assert!(matches(&v, ""));
        assert!(matches(&v, "PGDATA"));
        assert!(matches(&v, "loc"));
        assert!(matches(&v, "/var/lib"));
        assert!(!matches(&v, "redis"));
    }
}
