use std::collections::BTreeMap;

use bollard::models::{ContainerSummary, CreateImageInfo, ImageSummary};

use super::{Section, container_rows, format_bytes, matches, short_id};

#[derive(Clone, Debug, PartialEq)]
pub struct ImageRow {
    pub repo: String,
    pub tag: String,
    pub id: String,
    pub size: String,
    pub created: String,
    pub target: String,
}

impl ImageRow {
    pub fn matches(&self, query: &str) -> bool {
        matches(query, &[&format!("{}:{}", self.repo, self.tag), &self.id])
    }
}

const UNTAGGED: &str = "<none>";

pub fn rows(images: &[ImageSummary]) -> Vec<ImageRow> {
    let mut images: Vec<_> = images.iter().collect();
    images.sort_by_key(|i| std::cmp::Reverse(i.created));
    images
        .into_iter()
        .flat_map(|i| {
            let tags = tags(i);
            let row = |repo: &str, tag: &str, target: &str| ImageRow {
                repo: repo.into(),
                tag: tag.into(),
                id: short_id(&i.id),
                size: size(i.size),
                created: date(i.created),
                target: target.into(),
            };
            if tags.is_empty() {
                return vec![row(UNTAGGED, UNTAGGED, &i.id)];
            }
            tags.into_iter()
                .map(|t| {
                    let (repo, tag) = split_tag(t);
                    row(repo, tag.unwrap_or(UNTAGGED), t)
                })
                .collect()
        })
        .collect()
}

fn tags(i: &ImageSummary) -> Vec<&str> {
    i.repo_tags.iter().filter(|t| *t != "<none>:<none>").map(String::as_str).collect()
}

pub fn image_sections(i: &ImageSummary, containers: &[ContainerSummary]) -> Vec<Section> {
    let tags = tags(i);
    let tags = if tags.is_empty() { UNTAGGED.into() } else { tags.join(", ") };
    let users = containers.iter().filter(|c| c.image_id.as_ref() == Some(&i.id));
    vec![
        Section {
            title: "Image",
            rows: vec![
                ("ID".into(), i.id.clone()),
                ("Tags".into(), tags),
                ("Size".into(), size(i.size)),
                ("Created".into(), date(i.created)),
            ],
        },
        Section { title: "Used by", rows: container_rows(users) },
    ]
}

pub fn unused(images: &[ImageSummary], used_ids: &[String]) -> Vec<String> {
    images
        .iter()
        .filter(|i| !used_ids.contains(&i.id))
        .map(|i| {
            let tags = tags(i);
            let name = if tags.is_empty() { UNTAGGED.into() } else { tags.join(", ") };
            format!("{name} ({}, {})", short_id(&i.id), size(i.size))
        })
        .collect()
}

pub fn pull_args(input: &str) -> Option<(String, String)> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    let (name, tag) = match input.split_once('@') {
        Some((name, digest)) => (name, digest),
        None => {
            let (name, tag) = split_tag(input);
            (name, tag.unwrap_or("latest"))
        }
    };
    Some((name.into(), tag.into()))
}

fn size(bytes: i64) -> String {
    format_bytes(u64::try_from(bytes).unwrap_or(0))
}

fn split_tag(reference: &str) -> (&str, Option<&str>) {
    match reference.rsplit_once(':') {
        Some((name, tag)) if !tag.contains('/') => (name, Some(tag)),
        _ => (reference, None),
    }
}

fn date(unix: i64) -> String {
    let z = unix.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[derive(Default)]
struct Layer {
    done: bool,
    current: i64,
    total: i64,
}

pub struct PullProgress {
    reference: String,
    layers: BTreeMap<String, Layer>,
    status: Option<String>,
}

impl PullProgress {
    pub fn new(reference: impl Into<String>) -> Self {
        Self { reference: reference.into(), layers: BTreeMap::new(), status: None }
    }

    pub fn update(&mut self, info: &CreateImageInfo) -> Result<(), String> {
        if let Some(e) = &info.error_detail {
            return Err(e.message.clone().unwrap_or_else(|| "pull failed".into()));
        }
        let status = info.status.as_deref().unwrap_or_default();
        match &info.id {
            None => {
                if let Some(s) = status.strip_prefix("Status: ") {
                    self.status = Some(s.into());
                }
            }
            Some(_) if status.starts_with("Pulling from ") => {}
            Some(id) => {
                let layer = self.layers.entry(id.clone()).or_default();
                match status {
                    "Downloading" => {
                        if let Some(p) = &info.progress_detail {
                            layer.current = p.current.unwrap_or(layer.current);
                            layer.total = p.total.unwrap_or(layer.total);
                        }
                    }
                    "Verifying Checksum" | "Download complete" | "Extracting" => layer.current = layer.total,
                    "Pull complete" | "Already exists" => {
                        layer.current = layer.total;
                        layer.done = true;
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    pub fn line(&self) -> String {
        if let Some(s) = &self.status {
            return s.clone();
        }
        if self.layers.is_empty() {
            return format!("Pulling {}…", self.reference);
        }
        let done = self.layers.values().filter(|l| l.done).count();
        let mut line = format!("Pulling {}: {done}/{} layers", self.reference, self.layers.len());
        let total: i64 = self.layers.values().map(|l| l.total).sum();
        if total > 0 {
            let current: i64 = self.layers.values().map(|l| l.current).sum();
            line += &format!(", {} / {}", size(current), size(total));
        }
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bollard::models::{ErrorDetail, ProgressDetail};

    fn image(id: &str, tags: &[&str], created: i64) -> ImageSummary {
        ImageSummary {
            id: id.into(),
            repo_tags: tags.iter().map(|t| t.to_string()).collect(),
            created,
            size: 187_400_000,
            ..Default::default()
        }
    }

    #[test]
    fn one_row_per_tag_newest_first() {
        let rows = rows(&[
            image("sha256:aaaaaaaaaaaaaaaa", &["nginx:latest"], 1_600_000_000),
            image("sha256:bbbbbbbbbbbbbbbb", &["app:1.0", "localhost:5000/app:1.0"], 1_700_000_000),
        ]);
        let got: Vec<_> = rows.iter().map(|r| (r.repo.as_str(), r.tag.as_str(), r.target.as_str())).collect();
        assert_eq!(
            got,
            [
                ("app", "1.0", "app:1.0"),
                ("localhost:5000/app", "1.0", "localhost:5000/app:1.0"),
                ("nginx", "latest", "nginx:latest"),
            ]
        );
        assert_eq!(rows[0].id, "bbbbbbbbbbbb");
        assert_eq!(rows[0].size, "178.7 MiB");
        assert_eq!(rows[0].created, "2023-11-14");
    }

    #[test]
    fn filter_matches_repo_tag_or_id_ignoring_case() {
        let row = &rows(&[image("sha256:abc123def456789", &["Docker.io/Nginx:Alpine"], 0)])[0];
        for q in ["", "nginx", "ALPINE", "nginx:alp", "abc123", "  docker.io  "] {
            assert!(row.matches(q), "{q:?} should match");
        }
        for q in ["redis", "789"] {
            assert!(!row.matches(q), "{q:?} should not match");
        }
    }

    #[test]
    fn pull_always_names_one_tag_or_digest() {
        let args = |s| pull_args(s).map(|(n, t)| format!("{n} {t}"));
        assert_eq!(args(" nginx "), Some("nginx latest".into()));
        assert_eq!(args("nginx:1.25"), Some("nginx 1.25".into()));
        assert_eq!(args("localhost:5000/foo"), Some("localhost:5000/foo latest".into()));
        assert_eq!(args("localhost:5000/foo:dev"), Some("localhost:5000/foo dev".into()));
        assert_eq!(args("foo@sha256:abc"), Some("foo sha256:abc".into()));
        assert_eq!(args("  "), None);
    }

    fn info(id: Option<&str>, status: &str, progress: Option<(i64, i64)>) -> CreateImageInfo {
        CreateImageInfo {
            id: id.map(Into::into),
            status: Some(status.into()),
            progress_detail: progress.map(|(current, total)| ProgressDetail {
                current: Some(current),
                total: Some(total),
            }),
            ..Default::default()
        }
    }

    fn pull(events: &[CreateImageInfo]) -> PullProgress {
        let mut p = PullProgress::new("nginx:latest");
        for e in events {
            p.update(e).unwrap();
        }
        p
    }

    #[test]
    fn progress_starts_as_plain_pulling_line() {
        let p = pull(&[info(Some("latest"), "Pulling from library/nginx", None)]);
        assert_eq!(p.line(), "Pulling nginx:latest…");
    }

    #[test]
    fn progress_counts_finished_layers_and_downloaded_bytes() {
        let p = pull(&[
            info(Some("latest"), "Pulling from library/nginx", None),
            info(Some("a"), "Pulling fs layer", None),
            info(Some("b"), "Pulling fs layer", None),
            info(Some("c"), "Already exists", None),
            info(Some("a"), "Downloading", Some((1_000_000, 4_000_000))),
            info(Some("b"), "Downloading", Some((500_000, 2_000_000))),
            info(Some("a"), "Downloading", Some((3_000_000, 4_000_000))),
            info(Some("b"), "Download complete", None),
            info(Some("b"), "Extracting", Some((100, 2_000_000))),
            info(Some("b"), "Pull complete", None),
        ]);
        assert_eq!(p.line(), "Pulling nginx:latest: 2/3 layers, 4.8 MiB / 5.7 MiB");
    }

    #[test]
    fn final_status_replaces_progress() {
        let p = pull(&[
            info(Some("a"), "Pull complete", None),
            info(None, "Digest: sha256:abc", None),
            info(None, "Status: Downloaded newer image for nginx:latest", None),
        ]);
        assert_eq!(p.line(), "Downloaded newer image for nginx:latest");
    }

    #[test]
    fn error_detail_fails_the_pull() {
        let mut p = PullProgress::new("nope:latest");
        let err = CreateImageInfo {
            error_detail: Some(ErrorDetail { code: None, message: Some("manifest unknown".into()) }),
            ..Default::default()
        };
        assert_eq!(p.update(&err), Err("manifest unknown".into()));
    }

    #[test]
    fn unused_lists_images_no_container_references() {
        let images = [
            image("sha256:aaaaaaaaaaaaaaaa", &["nginx:latest"], 2),
            image("sha256:bbbbbbbbbbbbbbbb", &["app:1.0", "app:latest"], 1),
            image("sha256:cccccccccccccccc", &[], 0),
        ];
        assert_eq!(
            unused(&images, &["sha256:aaaaaaaaaaaaaaaa".into()]),
            ["app:1.0, app:latest (bbbbbbbbbbbb, 178.7 MiB)", "<none> (cccccccccccc, 178.7 MiB)"]
        );
    }

    #[test]
    fn image_sections_show_fields_and_containers_using_it() {
        use bollard::models::ContainerSummary;
        let c = |name: &str, image_id: &str| ContainerSummary {
            names: Some(vec![format!("/{name}")]),
            image_id: Some(image_id.into()),
            status: Some("Up 3 minutes".into()),
            ..Default::default()
        };
        let img = image("sha256:bbbbbbbbbbbbbbbb", &["app:1.0", "app:latest"], 1_700_000_000);
        let sections = image_sections(
            &img,
            &[c("web", "sha256:bbbbbbbbbbbbbbbb"), c("other", "sha256:aaaa"), c("api", "sha256:bbbbbbbbbbbbbbbb")],
        );
        let rows = |i: usize| sections[i].rows.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect::<Vec<_>>();
        assert_eq!(sections.iter().map(|s| s.title).collect::<Vec<_>>(), ["Image", "Used by"]);
        assert_eq!(
            rows(0),
            [
                ("ID", "sha256:bbbbbbbbbbbbbbbb"),
                ("Tags", "app:1.0, app:latest"),
                ("Size", "178.7 MiB"),
                ("Created", "2023-11-14")
            ]
        );
        assert_eq!(rows(1), [("api", "Up 3 minutes"), ("web", "Up 3 minutes")]);
        let untagged = image_sections(&image("sha256:cc", &["<none>:<none>"], 0), &[]);
        assert_eq!(untagged[0].rows[1], ("Tags".to_string(), "<none>".to_string()));
        assert!(untagged[1].rows.is_empty());
    }

    #[test]
    fn untagged_image_is_one_none_row_removed_by_id() {
        for tags in [&[][..], &["<none>:<none>"][..]] {
            let rows = rows(&[image("sha256:cccccccccccccccc", tags, 0)]);
            assert_eq!(rows.len(), 1);
            assert_eq!((rows[0].repo.as_str(), rows[0].tag.as_str()), ("<none>", "<none>"));
            assert_eq!(rows[0].target, "sha256:cccccccccccccccc");
            assert_eq!(rows[0].created, "1970-01-01");
        }
    }
}
