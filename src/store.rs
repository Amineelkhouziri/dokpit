use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::ProjectRecord;

#[derive(Serialize, Deserialize)]
struct File {
    #[serde(default)]
    project: Vec<ProjectRecord>,
}

pub fn default_path() -> PathBuf {
    relm4::gtk::glib::user_config_dir().join("dockpit/projects.toml")
}

pub fn load(path: &Path) -> io::Result<Vec<ProjectRecord>> {
    let text = match std::fs::read_to_string(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        r => r?,
    };
    let file: File = toml::from_str(&text).map_err(io::Error::other)?;
    Ok(file.project)
}

pub fn save(path: &Path, records: &[ProjectRecord]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = toml::to_string(&File { project: records.to_vec() }).map_err(io::Error::other)?;
    std::fs::write(path, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dockpit-store-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn record(name: &str, files: &[&str], dir: &str) -> ProjectRecord {
        ProjectRecord { name: name.into(), config_files: files.iter().map(PathBuf::from).collect(), working_dir: dir.into() }
    }

    #[test]
    fn saved_projects_load_back_in_order() {
        let dir = temp("roundtrip");
        let path = dir.join("nested/projects.toml");
        let records = [
            record("icon-it", &["/w/icon-it/docker-compose.yml", "/w/icon-it/docker-compose.override.yml"], "/w/icon-it"),
            record("ctoc", &["/w/ctoc/docker-compose.yml"], "/w/ctoc"),
        ];
        save(&path, &records).unwrap();
        assert_eq!(load(&path).unwrap(), records);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_file_loads_as_no_projects() {
        assert_eq!(load(&temp("missing").join("projects.toml")).unwrap(), []);
    }
}
