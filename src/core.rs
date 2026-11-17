use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use bollard::models::{
    ContainerCpuStats, ContainerStatsResponse, ContainerSummary, ContainerSummaryStateEnum as State, PortSummary, Volume,
};
use serde::{Deserialize, Serialize};

pub mod details;
pub mod images;
pub mod networks;

const PROJECT: &str = "com.docker.compose.project";
const CONFIG_FILES: &str = "com.docker.compose.project.config_files";
const WORKING_DIR: &str = "com.docker.compose.project.working_dir";
const DEFAULT_HOST: &str = "unix:///var/run/docker.sock";
const ANONYMOUS: &str = "com.docker.volume.anonymous";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectRecord {
    pub name: String,
    pub config_files: Vec<PathBuf>,
    pub working_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub record: ProjectRecord,
    pub live: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Memory {
    pub used: u64,
    pub limit: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub project: Option<String>,
    pub containers: Vec<ContainerSummary>,
}

pub fn matches(query: &str, fields: &[&str]) -> bool {
    let query = query.trim().to_lowercase();
    fields.iter().any(|f| f.to_lowercase().contains(&query))
}

pub struct Section {
    pub title: &'static str,
    pub rows: Vec<(String, String)>,
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

fn label<'a>(c: &'a ContainerSummary, key: &str) -> Option<&'a str> {
    c.labels
        .as_ref()?
        .get(key)
        .map(String::as_str)
        .filter(|v| !v.is_empty())
}

pub fn group_containers(containers: &[ContainerSummary]) -> Vec<Group> {
    let mut projects: BTreeMap<&str, Vec<ContainerSummary>> = BTreeMap::new();
    let mut standalone = Vec::new();
    for c in containers {
        match label(c, PROJECT) {
            Some(p) => projects.entry(p).or_default().push(c.clone()),
            None => standalone.push(c.clone()),
        }
    }
    let mut groups: Vec<Group> = projects
        .into_iter()
        .map(|(p, containers)| Group {
            project: Some(p.to_string()),
            containers,
        })
        .collect();
    if !standalone.is_empty() {
        groups.push(Group {
            project: None,
            containers: standalone,
        });
    }
    groups
}

pub fn filter_groups(groups: Vec<Group>, query: &str) -> Vec<Group> {
    groups
        .into_iter()
        .filter_map(|mut g| {
            if !g.project.as_deref().is_some_and(|p| matches(query, &[p])) {
                g.containers
                    .retain(|c| matches(query, &[&display_name(c), c.image.as_deref().unwrap_or_default()]));
            }
            (!g.containers.is_empty()).then_some(g)
        })
        .collect()
}

pub fn is_up(c: &ContainerSummary) -> bool {
    matches!(c.state, Some(State::RUNNING | State::PAUSED | State::RESTARTING))
}

pub fn short_id(id: &str) -> String {
    id.trim_start_matches("sha256:").chars().take(12).collect()
}

pub fn display_name(c: &ContainerSummary) -> String {
    match c.names.as_ref().and_then(|n| n.first()) {
        Some(name) => name.trim_start_matches('/').to_string(),
        None => short_id(c.id.as_deref().unwrap_or_default()),
    }
}

pub fn container_rows<'a>(containers: impl IntoIterator<Item = &'a ContainerSummary>) -> Vec<(String, String)> {
    let mut rows: Vec<_> =
        containers.into_iter().map(|c| (display_name(c), c.status.clone().unwrap_or_default())).collect();
    rows.sort();
    rows
}

pub fn format_ports(ports: &[PortSummary]) -> String {
    let unique: BTreeSet<_> = ports
        .iter()
        .map(|p| {
            (
                p.public_port.is_none(),
                p.public_port,
                p.private_port,
                p.typ,
            )
        })
        .collect();
    unique
        .into_iter()
        .map(|(_, public, private, typ)| {
            let typ = typ.map(|t| t.to_string()).unwrap_or_default();
            match public {
                Some(public) => format!("{public}→{private}/{typ}"),
                None => format!("{private}/{typ}"),
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn newest(containers: &[ContainerSummary]) -> Option<&ContainerSummary> {
    containers.iter().reduce(|a, b| if b.created > a.created { b } else { a })
}

pub fn group_working_dir(group: &Group) -> Option<PathBuf> {
    let dir = label(newest(&group.containers)?, WORKING_DIR)?;
    (!dir.is_empty()).then(|| PathBuf::from(dir))
}

pub fn state_class(counts: Option<(usize, usize)>) -> &'static str {
    match counts {
        Some((up, total)) if up == total => "success",
        Some((up, _)) if up > 0 => "warning",
        _ => "dim-label",
    }
}

pub fn live_projects(containers: &[ContainerSummary]) -> Vec<ProjectRecord> {
    group_containers(containers)
        .into_iter()
        .filter_map(|g| {
            let name = g.project?;
            let newest = newest(&g.containers)?;
            let working_dir = PathBuf::from(label(newest, WORKING_DIR).unwrap_or_default());
            let config_files = label(newest, CONFIG_FILES)
                .unwrap_or_default()
                .split(',')
                .filter(|f| !f.is_empty())
                .map(|f| working_dir.join(Path::new(f)))
                .collect();
            Some(ProjectRecord {
                name,
                config_files,
                working_dir,
            })
        })
        .collect()
}

pub fn merge_projects(live: Vec<ProjectRecord>, remembered: &[ProjectRecord]) -> Vec<Project> {
    let mut merged: BTreeMap<String, Project> = remembered
        .iter()
        .map(|r| {
            let project = Project {
                record: r.clone(),
                live: false,
            };
            (r.name.clone(), project)
        })
        .collect();
    for record in live {
        merged.insert(record.name.clone(), Project { record, live: true });
    }
    merged.into_values().collect()
}

pub fn prune_candidates(dangling: Vec<Volume>) -> Vec<String> {
    let mut names: Vec<String> = dangling
        .into_iter()
        .filter(|v| v.driver == "local" && v.labels.contains_key(ANONYMOUS))
        .map(|v| v.name)
        .collect();
    names.sort();
    names
}

pub fn volume_sections(v: &Volume, containers: &[ContainerSummary]) -> Vec<Section> {
    let mut labels: Vec<_> = v.labels.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    labels.sort();
    let users = containers.iter().filter(|c| {
        c.mounts.iter().flatten().any(|m| m.typ.as_deref() == Some("volume") && m.name.as_ref() == Some(&v.name))
    });
    vec![
        Section {
            title: "Volume",
            rows: vec![
                ("Name".into(), v.name.clone()),
                ("Driver".into(), v.driver.clone()),
                ("Mountpoint".into(), v.mountpoint.clone()),
            ],
        },
        Section { title: "Labels", rows: labels },
        Section { title: "Used by", rows: container_rows(users) },
    ]
}

pub fn volume_matches(v: &Volume, query: &str) -> bool {
    matches(query, &[&v.name, &v.driver, &v.mountpoint])
}

pub fn project_members<'a>(
    containers: &'a [ContainerSummary],
    project: &'a str,
) -> impl Iterator<Item = &'a ContainerSummary> {
    containers.iter().filter(move |c| label(c, PROJECT) == Some(project))
}

pub fn up_count(containers: &[ContainerSummary], project: &str) -> Option<(usize, usize)> {
    let (up, total) =
        project_members(containers, project).fold((0, 0), |(up, total), c| (up + usize::from(is_up(c)), total + 1));
    (total > 0).then_some((up, total))
}

pub fn project_state(counts: Option<(usize, usize)>) -> String {
    counts.map_or_else(|| "stopped".into(), |(up, total)| format!("{up}/{total} running"))
}

pub fn project_sections(p: &Project, containers: &[ContainerSummary]) -> Vec<Section> {
    let r = &p.record;
    vec![
        Section {
            title: "Project",
            rows: vec![
                ("State".into(), project_state(up_count(containers, &r.name))),
                ("Working directory".into(), r.working_dir.display().to_string()),
            ],
        },
        Section {
            title: "Config files",
            rows: r.config_files.iter().map(|f| (f.display().to_string(), String::new())).collect(),
        },
        Section { title: "Containers", rows: container_rows(project_members(containers, &r.name)) },
    ]
}

pub fn forget(remembered: &mut Vec<ProjectRecord>, name: &str) {
    remembered.retain(|r| r.name != name);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposeCmd {
    Up,
    Down,
    Pull,
}

impl ComposeCmd {
    pub fn name(self) -> &'static str {
        match self {
            ComposeCmd::Up => "up",
            ComposeCmd::Down => "down",
            ComposeCmd::Pull => "pull",
        }
    }
}

pub fn docker_host(env: Option<&str>) -> String {
    env.filter(|h| h.starts_with("unix://")).unwrap_or(DEFAULT_HOST).to_string()
}

pub fn shell_argv(host: &str, id: &str) -> Option<Vec<String>> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(["docker", "-H", host, "exec", "-it", id, "sh"].map(String::from).to_vec())
}

pub fn compose_args(host: &str, record: &ProjectRecord, cmd: ComposeCmd) -> Vec<String> {
    let mut args = vec!["-H".to_string(), host.into(), "compose".into(), "-p".into(), record.name.clone()];
    for file in &record.config_files {
        args.push("-f".into());
        args.push(file.display().to_string());
    }
    args.push(cmd.name().into());
    if cmd == ComposeCmd::Up {
        args.push("-d".into());
    }
    args
}

fn cpu_times(c: &ContainerCpuStats) -> Option<(u64, u64)> {
    Some((c.cpu_usage.as_ref()?.total_usage?, c.system_cpu_usage?))
}

pub fn cpu_percent(stats: &ContainerStatsResponse) -> f64 {
    let (Some(now), Some(pre)) = (stats.cpu_stats.as_ref(), stats.precpu_stats.as_ref()) else {
        return 0.0;
    };
    let (Some((total, system)), Some((pre_total, pre_system))) = (cpu_times(now), cpu_times(pre))
    else {
        return 0.0;
    };
    let cpu_delta = total.saturating_sub(pre_total);
    let system_delta = system.saturating_sub(pre_system);
    if cpu_delta == 0 || system_delta == 0 {
        return 0.0;
    }
    let cpus = now.online_cpus.map(u64::from).unwrap_or_else(|| {
        now.cpu_usage
            .as_ref()
            .and_then(|u| u.percpu_usage.as_ref())
            .map_or(0, |p| p.len() as u64)
    });
    cpu_delta as f64 / system_delta as f64 * cpus.max(1) as f64 * 100.0
}

pub fn memory(stats: &ContainerStatsResponse) -> Option<Memory> {
    let m = stats.memory_stats.as_ref()?;
    let usage = m.usage?;
    let cache = m
        .stats
        .as_ref()
        .and_then(|s| {
            s.get("inactive_file")
                .or_else(|| s.get("total_inactive_file"))
        })
        .copied()
        .filter(|&c| c < usage)
        .unwrap_or(0);
    Some(Memory {
        used: usage - cache,
        limit: m.limit?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bollard::models::{ContainerCpuUsage, ContainerMemoryStats, PortSummaryTypeEnum};

    fn container(id: &str, created: i64, labels: &[(&str, &str)]) -> ContainerSummary {
        ContainerSummary {
            id: Some(id.into()),
            created: Some(created),
            labels: Some(
                labels
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
            ),
            ..Default::default()
        }
    }

    fn compose(id: &str, project: &str) -> ContainerSummary {
        container(id, 0, &[("com.docker.compose.project", project)])
    }

    fn ids(group: &Group) -> Vec<&str> {
        group
            .containers
            .iter()
            .map(|c| c.id.as_deref().unwrap())
            .collect()
    }

    #[test]
    fn group_working_dir_comes_from_newest_container_label() {
        let mut old = container("a", 1, &[("com.docker.compose.project", "p"), ("com.docker.compose.project.working_dir", "/old")]);
        old.created = Some(1);
        let mut new = container("b", 2, &[("com.docker.compose.project", "p"), ("com.docker.compose.project.working_dir", "/new")]);
        new.created = Some(2);
        let group = Group { project: Some("p".into()), containers: vec![old, new] };
        assert_eq!(group_working_dir(&group), Some(PathBuf::from("/new")));
        assert_eq!(group_working_dir(&Group { project: None, containers: vec![compose("c", "p")] }), None);
        assert_eq!(state_class(Some((2, 2))), "success");
        assert_eq!(state_class(Some((1, 2))), "warning");
        assert_eq!(state_class(Some((0, 2))), "dim-label");
    }

    #[test]
    fn matches_trims_and_ignores_case_across_fields() {
        assert!(matches(" PGDATA ", &["shop_pgdata", "local"]));
        assert!(matches("LOC", &["shop_pgdata", "local"]));
        assert!(matches("  ", &["anything"]));
        assert!(!matches("redis", &["shop_pgdata", "local"]));
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
        assert!(volume_matches(&v, ""));
        assert!(volume_matches(&v, " PGDATA "));
        assert!(volume_matches(&v, "loc"));
        assert!(volume_matches(&v, "/var/lib"));
        assert!(!volume_matches(&v, "redis"));
    }

    fn rows_of(section: &Section) -> Vec<(&str, &str)> {
        section.rows.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect()
    }

    fn with_status(c: ContainerSummary, name: &str, status: &str) -> ContainerSummary {
        ContainerSummary { names: Some(vec![format!("/{name}")]), status: Some(status.into()), ..c }
    }

    #[test]
    fn volume_sections_list_fields_sorted_labels_and_containers_mounting_it() {
        use bollard::models::MountPoint;
        let mount = |typ: &str, name: &str| MountPoint { typ: Some(typ.into()), name: Some(name.into()), ..Default::default() };
        let using = |id: &str, name: &str, mounts: Vec<MountPoint>| ContainerSummary {
            mounts: Some(mounts),
            ..with_status(container(id, 0, &[]), name, "Up 2 hours")
        };
        let v = Volume {
            name: "pgdata".into(),
            driver: "local".into(),
            mountpoint: "/var/lib/docker/volumes/pgdata/_data".into(),
            labels: [("z".to_string(), "1".to_string()), ("a".to_string(), "2".to_string())].into(),
            ..Default::default()
        };
        let sections = volume_sections(
            &v,
            &[
                using("1", "web", vec![mount("volume", "other")]),
                using("2", "db", vec![mount("volume", "pgdata")]),
                using("3", "bind", vec![mount("bind", "pgdata")]),
                using("4", "backup", vec![mount("volume", "pgdata")]),
            ],
        );
        let titles: Vec<_> = sections.iter().map(|s| s.title).collect();
        assert_eq!(titles, ["Volume", "Labels", "Used by"]);
        assert_eq!(
            rows_of(&sections[0]),
            [("Name", "pgdata"), ("Driver", "local"), ("Mountpoint", "/var/lib/docker/volumes/pgdata/_data")]
        );
        assert_eq!(rows_of(&sections[1]), [("a", "2"), ("z", "1")]);
        assert_eq!(rows_of(&sections[2]), [("backup", "Up 2 hours"), ("db", "Up 2 hours")]);
    }

    #[test]
    fn project_sections_show_state_paths_and_member_containers() {
        use bollard::models::ContainerSummaryStateEnum as State;
        let member = |id: &str, project: &str, state| ContainerSummary {
            state: Some(state),
            ..with_status(compose(id, project), id, if state == State::RUNNING { "Up" } else { "Exited (0)" })
        };
        let containers = [
            member("web-1", "shop", State::RUNNING),
            member("db-1", "shop", State::EXITED),
            member("api-1", "blog", State::RUNNING),
        ];
        let live = Project { record: record("shop", &["/s/compose.yml", "/s/override.yml"], "/s"), live: true };
        let sections = project_sections(&live, &containers);
        let titles: Vec<_> = sections.iter().map(|s| s.title).collect();
        assert_eq!(titles, ["Project", "Config files", "Containers"]);
        assert_eq!(rows_of(&sections[0]), [("State", "1/2 running"), ("Working directory", "/s")]);
        assert_eq!(rows_of(&sections[1]), [("/s/compose.yml", ""), ("/s/override.yml", "")]);
        assert_eq!(rows_of(&sections[2]), [("db-1", "Exited (0)"), ("web-1", "Up")]);

        let stopped = Project { record: record("old", &[], "/o"), live: false };
        let sections = project_sections(&stopped, &containers);
        assert_eq!(rows_of(&sections[0]), [("State", "stopped"), ("Working directory", "/o")]);
        assert!(sections[2].rows.is_empty());
    }

    #[test]
    fn project_state_counts_paused_as_up() {
        use bollard::models::ContainerSummaryStateEnum as State;
        let member = |id: &str, state| ContainerSummary { state: Some(state), ..compose(id, "shop") };
        let containers = [member("a", State::RUNNING), member("b", State::PAUSED), member("c", State::EXITED)];
        assert_eq!(up_count(&containers, "shop"), Some((2, 3)));
        assert_eq!(up_count(&containers, "none"), None);
    }

    #[test]
    fn groups_by_project_label_with_standalone_last() {
        let groups = group_containers(&[
            compose("w1", "web"),
            container("solo", 0, &[]),
            compose("a1", "api"),
            compose("w2", "web"),
            compose("blank", ""),
        ]);
        let names: Vec<_> = groups.iter().map(|g| g.project.as_deref()).collect();
        assert_eq!(names, [Some("api"), Some("web"), None]);
        assert_eq!(ids(&groups[0]), ["a1"]);
        assert_eq!(ids(&groups[1]), ["w1", "w2"]);
        assert_eq!(ids(&groups[2]), ["solo", "blank"]);
    }

    fn compose_files(
        id: &str,
        project: &str,
        created: i64,
        files: &str,
        dir: &str,
    ) -> ContainerSummary {
        container(
            id,
            created,
            &[
                ("com.docker.compose.project", project),
                ("com.docker.compose.project.config_files", files),
                ("com.docker.compose.project.working_dir", dir),
            ],
        )
    }

    fn record(name: &str, files: &[&str], dir: &str) -> ProjectRecord {
        ProjectRecord {
            name: name.into(),
            config_files: files.iter().map(PathBuf::from).collect(),
            working_dir: dir.into(),
        }
    }

    #[test]
    fn relative_config_file_resolves_against_working_dir() {
        let projects = live_projects(&[
            compose_files(
                "n",
                "ctoc",
                100,
                "docker-compose.yml",
                "/home/hamza/Webpick/ctoc",
            ),
            compose_files(
                "p",
                "ctoc",
                100,
                "docker-compose.yml",
                "/home/hamza/Webpick/ctoc",
            ),
        ]);
        assert_eq!(
            projects,
            [record(
                "ctoc",
                &["/home/hamza/Webpick/ctoc/docker-compose.yml"],
                "/home/hamza/Webpick/ctoc"
            )]
        );
    }

    #[test]
    fn newest_container_file_set_wins_and_keeps_order() {
        let dir = "/home/hamza/Webpick/icon-it";
        let base = "/home/hamza/Webpick/icon-it/docker-compose.yml";
        let both = "/home/hamza/Webpick/icon-it/docker-compose.yml,/home/hamza/Webpick/icon-it/docker-compose.override.yml";
        let projects = live_projects(&[
            compose_files("pma", "icon-it", 1_762_698_760, base, dir),
            compose_files("nginx", "icon-it", 1_762_702_665, both, dir),
            compose_files("db", "icon-it", 1_762_702_663, both, dir),
            compose_files("memcached", "icon-it", 1_762_698_760, base, dir),
        ]);
        assert_eq!(
            projects,
            [record(
                "icon-it",
                &[
                    "/home/hamza/Webpick/icon-it/docker-compose.yml",
                    "/home/hamza/Webpick/icon-it/docker-compose.override.yml"
                ],
                dir
            )]
        );
    }

    #[test]
    fn standalone_containers_are_not_projects() {
        assert_eq!(live_projects(&[container("solo", 0, &[])]), []);
    }

    #[test]
    fn merge_live_updates_record_and_missing_live_is_stopped() {
        let merged = merge_projects(
            vec![record("web", &["/w/a.yml", "/w/b.yml"], "/w")],
            &[
                record("web", &["/w/a.yml"], "/w"),
                record("old", &["/o/compose.yml"], "/o"),
            ],
        );
        assert_eq!(
            merged,
            [
                Project {
                    record: record("old", &["/o/compose.yml"], "/o"),
                    live: false
                },
                Project {
                    record: record("web", &["/w/a.yml", "/w/b.yml"], "/w"),
                    live: true
                },
            ]
        );
    }

    #[test]
    fn merge_forgets_project_whose_record_was_removed() {
        let mut remembered = vec![record("gone", &["/g/compose.yml"], "/g"), record("kept", &[], "/k")];
        forget(&mut remembered, "gone");
        let merged = merge_projects(vec![], &remembered);
        assert_eq!(
            merged,
            [Project {
                record: record("kept", &[], "/k"),
                live: false
            }]
        );
    }

    #[test]
    fn running_paused_and_restarting_count_as_up() {
        use bollard::models::ContainerSummaryStateEnum as State;
        let with = |state| ContainerSummary { state: Some(state), ..Default::default() };
        for state in [State::RUNNING, State::PAUSED, State::RESTARTING] {
            assert!(is_up(&with(state)), "{state:?}");
        }
        for state in [State::EXITED, State::CREATED, State::DEAD, State::REMOVING] {
            assert!(!is_up(&with(state)), "{state:?}");
        }
        assert!(!is_up(&ContainerSummary::default()));
    }

    #[test]
    fn compose_cmd_names_match_cli_subcommands() {
        assert_eq!([ComposeCmd::Up, ComposeCmd::Down, ComposeCmd::Pull].map(ComposeCmd::name), ["up", "down", "pull"]);
    }

    #[test]
    fn compose_args_target_host_and_pass_project_and_every_file_in_order() {
        let r = record("icon-it", &["/w/docker-compose.yml", "/w/docker-compose.override.yml"], "/w");
        let host = "unix:///var/run/docker.sock";
        let base = [
            "-H", host, "compose", "-p", "icon-it", "-f", "/w/docker-compose.yml", "-f", "/w/docker-compose.override.yml",
        ];
        let with = |tail: &[&'static str]| [&base[..], tail].concat();
        assert_eq!(compose_args(host, &r, ComposeCmd::Up), with(&["up", "-d"]));
        assert_eq!(compose_args(host, &r, ComposeCmd::Down), with(&["down"]));
        assert_eq!(compose_args(host, &r, ComposeCmd::Pull), with(&["pull"]));
    }

    #[test]
    fn docker_host_uses_unix_docker_host_else_default_socket() {
        assert_eq!(docker_host(Some("unix:///run/user/1000/docker.sock")), "unix:///run/user/1000/docker.sock");
        assert_eq!(docker_host(Some("tcp://10.0.0.1:2375")), "unix:///var/run/docker.sock");
        assert_eq!(docker_host(None), "unix:///var/run/docker.sock");
    }

    #[test]
    fn shell_argv_targets_host_and_requires_hex_id() {
        let host = "unix:///var/run/docker.sock";
        assert_eq!(
            shell_argv(host, "0123abcdef"),
            Some(vec!["docker", "-H", host, "exec", "-it", "0123abcdef", "sh"].into_iter().map(String::from).collect())
        );
        assert_eq!(shell_argv(host, ""), None);
        assert_eq!(shell_argv(host, "abc; rm -rf ~"), None);
        assert_eq!(shell_argv(host, "web-1"), None);
    }

    fn cpu(total: u64, system: u64, online: Option<u32>, percpu: usize) -> ContainerCpuStats {
        ContainerCpuStats {
            cpu_usage: Some(ContainerCpuUsage {
                total_usage: Some(total),
                percpu_usage: Some(vec![0; percpu]),
                ..Default::default()
            }),
            system_cpu_usage: Some(system),
            online_cpus: online,
            ..Default::default()
        }
    }

    fn sample(now: ContainerCpuStats, pre: ContainerCpuStats) -> ContainerStatsResponse {
        ContainerStatsResponse {
            cpu_stats: Some(now),
            precpu_stats: Some(pre),
            ..Default::default()
        }
    }

    #[test]
    fn cpu_percent_scales_delta_ratio_by_online_cpus() {
        let s = sample(cpu(400, 2000, Some(4), 0), cpu(200, 1000, Some(4), 0));
        assert_eq!(cpu_percent(&s), 80.0);
    }

    #[test]
    fn cpu_percent_falls_back_to_percpu_count() {
        let s = sample(cpu(150, 1100, None, 2), cpu(100, 1000, None, 2));
        assert_eq!(cpu_percent(&s), 100.0);
    }

    #[test]
    fn cpu_percent_is_zero_without_system_delta_or_previous_sample() {
        let s = sample(cpu(400, 1000, Some(4), 0), cpu(200, 1000, Some(4), 0));
        assert_eq!(cpu_percent(&s), 0.0);
        let first = ContainerStatsResponse {
            cpu_stats: Some(cpu(400, 1000, Some(4), 0)),
            precpu_stats: Some(ContainerCpuStats::default()),
            ..Default::default()
        };
        assert_eq!(cpu_percent(&first), 0.0);
    }

    fn mem(usage: u64, limit: u64, stats: &[(&str, u64)]) -> ContainerStatsResponse {
        ContainerStatsResponse {
            memory_stats: Some(ContainerMemoryStats {
                usage: Some(usage),
                limit: Some(limit),
                stats: Some(stats.iter().map(|(k, v)| (k.to_string(), *v)).collect()),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn memory_excludes_inactive_file_cache() {
        let v2 = mem(500, 8000, &[("inactive_file", 120), ("active_file", 50)]);
        assert_eq!(
            memory(&v2),
            Some(Memory {
                used: 380,
                limit: 8000
            })
        );
        let v1 = mem(500, 8000, &[("total_inactive_file", 100)]);
        assert_eq!(
            memory(&v1),
            Some(Memory {
                used: 400,
                limit: 8000
            })
        );
    }

    #[test]
    fn memory_uses_raw_usage_when_cache_is_absent_or_larger() {
        assert_eq!(
            memory(&mem(500, 8000, &[])),
            Some(Memory {
                used: 500,
                limit: 8000
            })
        );
        let odd = mem(500, 8000, &[("inactive_file", 900)]);
        assert_eq!(
            memory(&odd),
            Some(Memory {
                used: 500,
                limit: 8000
            })
        );
    }

    #[test]
    fn memory_is_none_for_stopped_container_sample() {
        assert_eq!(memory(&ContainerStatsResponse::default()), None);
    }

    #[test]
    fn display_name_strips_leading_slash_and_falls_back_to_short_id() {
        let named = ContainerSummary {
            names: Some(vec!["/web-1".into()]),
            ..container("0123456789abcdef", 0, &[])
        };
        assert_eq!(display_name(&named), "web-1");
        assert_eq!(
            display_name(&container("0123456789abcdef", 0, &[])),
            "0123456789ab"
        );
    }

    fn port(
        ip: Option<&str>,
        public: Option<u16>,
        private: u16,
        typ: PortSummaryTypeEnum,
    ) -> PortSummary {
        PortSummary {
            ip: ip.map(Into::into),
            private_port: private,
            public_port: public,
            typ: Some(typ),
        }
    }

    #[test]
    fn ports_are_deduped_across_ip_families_sorted_and_unpublished_shown_bare() {
        use PortSummaryTypeEnum::{TCP, UDP};
        let ports = [
            port(Some("::"), Some(8080), 80, TCP),
            port(None, None, 9000, TCP),
            port(Some("0.0.0.0"), Some(8080), 80, TCP),
            port(Some("0.0.0.0"), Some(53), 53, UDP),
        ];
        assert_eq!(format_ports(&ports), "53→53/udp, 8080→80/tcp, 9000/tcp");
        assert_eq!(format_ports(&[]), "");
    }

    fn named(id: &str, image: &str, project: Option<&str>) -> ContainerSummary {
        let labels: Vec<_> = project
            .map(|p| ("com.docker.compose.project", p))
            .into_iter()
            .collect();
        ContainerSummary {
            names: Some(vec![format!("/{id}")]),
            image: Some(image.into()),
            ..container(id, 0, &labels)
        }
    }

    #[test]
    fn filter_matches_name_image_or_project_case_insensitively() {
        let groups = group_containers(&[
            named("shop-db-1", "postgres:16", Some("shop")),
            named("shop-web-1", "nginx:1.27", Some("shop")),
            named("blog-web-1", "Nginx:1.25", Some("blog")),
            named("redis", "redis:7", None),
        ]);
        let ids_of = |gs: Vec<Group>| -> Vec<Vec<String>> {
            gs.iter()
                .map(|g| ids(g).into_iter().map(String::from).collect())
                .collect()
        };

        assert_eq!(
            ids_of(filter_groups(groups.clone(), "SHOP")),
            [vec!["shop-db-1", "shop-web-1"]]
        );
        assert_eq!(
            ids_of(filter_groups(groups.clone(), "nginx")),
            [vec!["blog-web-1"], vec!["shop-web-1"]]
        );
        assert_eq!(
            ids_of(filter_groups(groups.clone(), " redis ")),
            [vec!["redis"]]
        );
        assert_eq!(filter_groups(groups.clone(), ""), groups);
        assert!(filter_groups(groups, "nothing").is_empty());
    }
}
