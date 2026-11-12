#![cfg_attr(not(test), allow(dead_code))]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bollard::models::{ContainerCpuStats, ContainerStatsResponse, ContainerSummary};

const PROJECT: &str = "com.docker.compose.project";
const CONFIG_FILES: &str = "com.docker.compose.project.config_files";
const WORKING_DIR: &str = "com.docker.compose.project.working_dir";

#[derive(Debug, Clone, PartialEq, Eq)]
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

pub fn live_projects(containers: &[ContainerSummary]) -> Vec<ProjectRecord> {
    group_containers(containers)
        .into_iter()
        .filter_map(|g| {
            let name = g.project?;
            let newest = g
                .containers
                .iter()
                .reduce(|a, b| if b.created > a.created { b } else { a })?;
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
    use bollard::models::{ContainerCpuUsage, ContainerMemoryStats};

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
        let merged = merge_projects(vec![], &[record("kept", &[], "/k")]);
        assert_eq!(
            merged,
            [Project {
                record: record("kept", &[], "/k"),
                live: false
            }]
        );
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
}
