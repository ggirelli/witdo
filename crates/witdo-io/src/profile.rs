//! Profiles: separate sets of witdo's own state (known workspaces, window preferences), stored
//! in the platform's configuration directory. They hold paths and preferences, never task
//! content, and share nothing with each other.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

const CONFIG: &str = "config.toml";
/// How many workspaces a profile remembers.
const MAX_RECENT: usize = 20;

/// A profile's name: letters, digits, `-` and `_`, as it names a directory.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProfileName(String);

impl ProfileName {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for ProfileName {
    fn default() -> Self {
        Self("default".to_owned())
    }
}

impl fmt::Display for ProfileName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for ProfileName {
    type Err = ProfileError;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        let valid = (1..=64).contains(&name.len())
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if valid {
            Ok(Self(name.to_owned()))
        } else {
            Err(ProfileError::BadName(name.to_owned()))
        }
    }
}

/// What a profile remembers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileConfig {
    /// Most recently opened first.
    #[serde(default)]
    pub recent_workspaces: Vec<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_workspace: Option<PathBuf>,
    #[serde(default)]
    pub window: WindowPreferences,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowPreferences {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    /// The desktop app's zoom, in percent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoom: Option<u32>,
}

impl ProfileConfig {
    /// Records that a workspace was opened: it becomes the last one and moves to the top of
    /// the recent list.
    pub fn remember(&mut self, workspace: &Path) {
        self.recent_workspaces.retain(|path| path != workspace);
        self.recent_workspaces.insert(0, workspace.to_owned());
        self.recent_workspaces.truncate(MAX_RECENT);
        self.last_workspace = Some(workspace.to_owned());
    }

    /// Drops a workspace from the profile; its files are not touched.
    pub fn forget(&mut self, workspace: &Path) {
        self.recent_workspaces.retain(|path| path != workspace);
        if self.last_workspace.as_deref() == Some(workspace) {
            self.last_workspace = None;
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    #[error("“{0}” can’t be a profile name: use up to 64 letters, digits, dashes and underscores.")]
    BadName(String),
    #[error("There is no configuration directory for witdo on this system.")]
    NoConfigDir,
    #[error("{} could not be {action}: {source}", path.display())]
    Io {
        path: PathBuf,
        action: &'static str,
        source: io::Error,
    },
    #[error("{} could not be read: {message}", path.display())]
    BadConfig { path: PathBuf, message: String },
}

/// Where profiles are stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profiles {
    dir: PathBuf,
}

impl Profiles {
    /// The platform's location, e.g. `$XDG_CONFIG_HOME/witdo/profiles` on Linux.
    ///
    /// # Errors
    ///
    /// When the system has no home or configuration directory.
    pub fn from_system() -> Result<Self, ProfileError> {
        let dirs =
            directories::ProjectDirs::from("", "", "witdo").ok_or(ProfileError::NoConfigDir)?;
        Ok(Self::at(dirs.config_dir().join("profiles")))
    }

    /// Profiles stored under `dir`.
    #[must_use]
    pub fn at(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// The file a profile is stored in.
    #[must_use]
    pub fn config_path(&self, name: &ProfileName) -> PathBuf {
        self.dir.join(name.as_str()).join(CONFIG)
    }

    /// A profile's configuration; empty for a profile never saved.
    ///
    /// # Errors
    ///
    /// When the file exists but cannot be read or parsed.
    pub fn load(&self, name: &ProfileName) -> Result<ProfileConfig, ProfileError> {
        let path = self.config_path(name);
        match fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).map_err(|error| ProfileError::BadConfig {
                path,
                message: error.message().to_owned(),
            }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(ProfileConfig::default()),
            Err(source) => Err(ProfileError::Io {
                path,
                action: "read",
                source,
            }),
        }
    }

    /// Saves a profile's configuration, replacing the file in one step.
    ///
    /// # Errors
    ///
    /// When the file cannot be written, or a path in it is not valid UTF-8.
    pub fn save(&self, name: &ProfileName, config: &ProfileConfig) -> Result<(), ProfileError> {
        let path = self.config_path(name);
        let text = toml::to_string(config).map_err(|error| ProfileError::BadConfig {
            path: path.clone(),
            message: error.to_string(),
        })?;
        let dir = path.parent().unwrap_or(&self.dir);
        let io_error = |action| {
            let path = path.clone();
            move |source| ProfileError::Io {
                path,
                action,
                source,
            }
        };
        fs::create_dir_all(dir).map_err(io_error("created"))?;
        let tmp = dir.join(format!("{CONFIG}.tmp"));
        fs::write(&tmp, text).map_err(io_error("written"))?;
        fs::rename(&tmp, &path).map_err(|error| {
            let _ = fs::remove_file(&tmp);
            io_error("written")(error)
        })
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    fn name(name: &str) -> ProfileName {
        name.parse().unwrap()
    }

    #[test]
    fn profile_names_are_safe_directory_names() {
        assert_eq!(ProfileName::default().as_str(), "default");
        assert!("work_2026-q4".parse::<ProfileName>().is_ok());
        for bad in [
            "",
            "../etc",
            "my profile",
            "a/b",
            ".hidden",
            &"x".repeat(65),
        ] {
            assert!(bad.parse::<ProfileName>().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn an_unsaved_profile_is_empty() {
        let dir = TempDir::new().unwrap();
        let profiles = Profiles::at(dir.path().to_owned());
        assert_eq!(
            profiles.load(&name("work")).unwrap(),
            ProfileConfig::default()
        );
    }

    #[test]
    fn profiles_save_and_load_separately() {
        let dir = TempDir::new().unwrap();
        let profiles = Profiles::at(dir.path().to_owned());
        let mut work = ProfileConfig::default();
        work.remember(Path::new("/home/me/work"));
        work.window.width = Some(1200);
        profiles.save(&name("work"), &work).unwrap();

        assert_eq!(profiles.load(&name("work")).unwrap(), work);
        assert_eq!(
            profiles.load(&name("personal")).unwrap(),
            ProfileConfig::default()
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("work/config.toml")).unwrap(),
            "recent_workspaces = [\"/home/me/work\"]\nlast_workspace = \"/home/me/work\"\n\n[window]\nwidth = 1200\n"
        );
    }

    #[test]
    fn remembering_moves_a_workspace_to_the_top() {
        let mut config = ProfileConfig::default();
        for path in ["/a", "/b", "/c", "/a"] {
            config.remember(Path::new(path));
        }
        assert_eq!(
            config.recent_workspaces,
            [Path::new("/a"), Path::new("/c"), Path::new("/b")]
        );
        assert_eq!(config.last_workspace.as_deref(), Some(Path::new("/a")));

        for n in 0..30 {
            config.remember(Path::new(&format!("/w{n}")));
        }
        assert_eq!(config.recent_workspaces.len(), MAX_RECENT);
    }

    #[test]
    fn forgetting_drops_a_workspace() {
        let mut config = ProfileConfig::default();
        config.remember(Path::new("/a"));
        config.remember(Path::new("/b"));
        config.forget(Path::new("/b"));
        assert_eq!(config.recent_workspaces, [Path::new("/a")]);
        assert_eq!(config.last_workspace, None);
    }

    #[test]
    fn a_broken_config_is_reported() {
        let dir = TempDir::new().unwrap();
        let profiles = Profiles::at(dir.path().to_owned());
        fs::create_dir_all(dir.path().join("work")).unwrap();
        fs::write(
            dir.path().join("work/config.toml"),
            "recent_workspaces = 3\n",
        )
        .unwrap();
        assert!(matches!(
            profiles.load(&name("work")),
            Err(ProfileError::BadConfig { .. })
        ));
    }
}
