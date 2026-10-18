use std::ffi::OsString;
use std::path::{Path, PathBuf};

use witdo_io::profile::{ProfileConfig, ProfileName, Profiles, Theme};
use witdo_io::workspace::{self, DirectoryState, Workspace};

use crate::{AppError, Clock, Session};

/// The profile witdo runs in: the workspaces it knows and its window preferences.
pub struct ProfileSession {
    profiles: Profiles,
    name: ProfileName,
    config: ProfileConfig,
}

impl ProfileSession {
    /// Loads a profile; one never used before starts empty.
    ///
    /// # Errors
    ///
    /// When the profile's file exists but cannot be read.
    pub fn load(profiles: Profiles, name: ProfileName) -> Result<Self, AppError> {
        let config = profiles.load(&name)?;
        Ok(Self {
            profiles,
            name,
            config,
        })
    }

    #[must_use]
    pub fn name(&self) -> &ProfileName {
        &self.name
    }

    #[must_use]
    pub fn config(&self) -> &ProfileConfig {
        &self.config
    }

    /// Workspaces opened in this profile, most recent first.
    #[must_use]
    pub fn recent_workspaces(&self) -> &[PathBuf] {
        &self.config.recent_workspaces
    }

    /// What a directory is, to decide between opening it, making it a workspace, and asking
    /// first (when it has files in it).
    ///
    /// # Errors
    ///
    /// When the directory exists but cannot be read.
    pub fn inspect(path: &Path) -> Result<DirectoryState, AppError> {
        Ok(workspace::inspect(path)?)
    }

    /// Opens a workspace and remembers it in this profile.
    ///
    /// # Errors
    ///
    /// When the workspace cannot be opened, or the profile cannot be saved.
    pub fn open_workspace(&mut self, path: &Path, clock: Clock) -> Result<Session, AppError> {
        let session = Session::open(path, clock)?;
        self.config.remember(session.workspace().root());
        self.save()?;
        Ok(session)
    }

    /// Makes a directory a workspace, then opens it. Files already there are left alone; ask
    /// the person first when [`ProfileSession::inspect`] says the directory is not empty.
    ///
    /// # Errors
    ///
    /// When the directory is already a workspace or cannot be set up, or the profile cannot be
    /// saved.
    pub fn create_workspace(
        &mut self,
        path: &Path,
        name: Option<String>,
        clock: Clock,
    ) -> Result<Session, AppError> {
        Workspace::init(path, name)?;
        self.open_workspace(path, clock)
    }

    /// Drops a workspace from this profile's list; its files are not touched.
    ///
    /// # Errors
    ///
    /// When the profile cannot be saved.
    pub fn forget_workspace(&mut self, path: &Path) -> Result<(), AppError> {
        self.config.forget(path);
        self.save()
    }

    /// Remembers the window's size for next time.
    ///
    /// # Errors
    ///
    /// When the profile cannot be saved.
    pub fn set_window_size(&mut self, width: u32, height: u32) -> Result<(), AppError> {
        self.config.window.width = Some(width);
        self.config.window.height = Some(height);
        self.save()
    }

    /// Remembers the desktop app's zoom, in percent.
    ///
    /// # Errors
    ///
    /// When the profile cannot be saved.
    pub fn set_zoom(&mut self, percent: u32) -> Result<(), AppError> {
        self.config.window.zoom = Some(percent);
        self.save()
    }

    /// Remembers the desktop app's theme; `None` follows the system.
    ///
    /// # Errors
    ///
    /// When the profile cannot be saved.
    pub fn set_theme(&mut self, theme: Option<Theme>) -> Result<(), AppError> {
        self.config.window.theme = theme;
        self.save()
    }

    fn save(&self) -> Result<(), AppError> {
        Ok(self.profiles.save(&self.name, &self.config)?)
    }
}

/// What to do at start-up, from the command line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Run(Launch),
    Help,
    Version,
}

/// The profile to run in, and a workspace to open straight away.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Launch {
    pub profile: ProfileName,
    pub workspace: Option<PathBuf>,
}

/// The command-line help, for a binary called `binary`.
#[must_use]
pub fn usage(binary: &str) -> String {
    format!(
        "Usage: {binary} [--profile <name>] [--workspace <path>]\n\n\
         Options:\n  \
         --profile <name>    Run in this profile (default: $WITDO_PROFILE, or \"default\")\n  \
         --workspace <path>  Open this workspace and remember it in the profile\n  \
         -h, --help          Show this help\n  \
         -V, --version       Show the version\n"
    )
}

/// Reads the command line (without the program name) and `WITDO_PROFILE`. `--profile` wins
/// over the variable.
///
/// # Errors
///
/// When an option is unknown, lacks its value, or the profile name is not valid.
pub fn parse_args(
    args: impl IntoIterator<Item = OsString>,
    profile_from_env: Option<OsString>,
) -> Result<Command, AppError> {
    let mut profile = profile_from_env.filter(|name| !name.is_empty());
    let mut workspace = None;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let arg = arg.into_string().map_err(|arg| {
            AppError::Usage(format!("“{}” is not valid text.", arg.to_string_lossy()))
        })?;
        let (option, inline) = match arg.split_once('=') {
            Some((option, value)) if option.starts_with("--") => {
                (option.to_owned(), Some(value.to_owned()))
            }
            _ => (arg, None),
        };
        let mut value = |option: &str| {
            inline
                .clone()
                .map(OsString::from)
                .or_else(|| args.next())
                .ok_or_else(|| AppError::Usage(format!("{option} needs a value.")))
        };
        match option.as_str() {
            "-h" | "--help" => return Ok(Command::Help),
            "-V" | "--version" => return Ok(Command::Version),
            "--profile" => profile = Some(value("--profile")?),
            "--workspace" => workspace = Some(PathBuf::from(value("--workspace")?)),
            other => return Err(AppError::Usage(format!("Unknown option “{other}”."))),
        }
    }
    let profile = match profile {
        None => ProfileName::default(),
        Some(name) => name
            .to_str()
            .ok_or_else(|| AppError::Usage("The profile name is not valid text.".into()))?
            .parse()?,
    };
    Ok(Command::Run(Launch { profile, workspace }))
}

/// Where start-up landed.
pub enum Opening {
    /// A workspace is open.
    Opened(Box<Session>),
    /// No workspace yet: let the person pick a recent one or choose a directory.
    Choose,
    /// `--workspace` named a directory that is not a workspace: ask whether to make it one
    /// (and say so when it already has files in it).
    Confirm {
        path: PathBuf,
        state: DirectoryState,
    },
    /// The workspace could not be opened; say why, then let the person choose.
    Failed(AppError),
}

/// Opens what the command line asked for, or else the profile's last workspace.
///
/// # Errors
///
/// When the profile cannot be loaded. Problems opening a workspace are in the [`Opening`].
pub fn start(
    launch: &Launch,
    profiles: Profiles,
    clock: &Clock,
) -> Result<(ProfileSession, Opening), AppError> {
    let mut profile = ProfileSession::load(profiles, launch.profile.clone())?;
    let opening = if let Some(path) = &launch.workspace {
        match ProfileSession::inspect(path) {
            Ok(DirectoryState::Workspace) => opened(profile.open_workspace(path, clock.clone())),
            Ok(state) => Opening::Confirm {
                path: path.clone(),
                state,
            },
            Err(error) => Opening::Failed(error),
        }
    } else if let Some(path) = profile.config.last_workspace.clone() {
        opened(profile.open_workspace(&path, clock.clone()))
    } else {
        Opening::Choose
    };
    Ok((profile, opening))
}

fn opened(result: Result<Session, AppError>) -> Opening {
    match result {
        Ok(session) => Opening::Opened(Box::new(session)),
        Err(error) => Opening::Failed(error),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    fn args(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    fn run(args_: &[&str], env: Option<&str>) -> Launch {
        match parse_args(args(args_), env.map(OsString::from)).unwrap() {
            Command::Run(launch) => launch,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_the_command_line() {
        assert_eq!(run(&[], None), Launch::default());
        let launch = run(&["--profile", "work", "--workspace", "/w"], None);
        assert_eq!(launch.profile.as_str(), "work");
        assert_eq!(launch.workspace.as_deref(), Some(Path::new("/w")));
        assert_eq!(run(&["--profile=work"], None).profile.as_str(), "work");
        assert_eq!(parse_args(args(&["-h"]), None).unwrap(), Command::Help);
        assert_eq!(
            parse_args(args(&["--version"]), None).unwrap(),
            Command::Version
        );
    }

    #[test]
    fn the_profile_comes_from_the_environment_unless_given() {
        assert_eq!(run(&[], Some("personal")).profile.as_str(), "personal");
        assert_eq!(
            run(&["--profile", "work"], Some("personal"))
                .profile
                .as_str(),
            "work"
        );
        assert_eq!(run(&[], Some("")).profile.as_str(), "default");
    }

    #[test]
    fn explains_bad_command_lines() {
        let error = |a: &[&str]| parse_args(args(a), None).unwrap_err().to_string();
        assert_eq!(error(&["--profile"]), "--profile needs a value.");
        assert_eq!(error(&["--colour"]), "Unknown option “--colour”.");
        assert!(error(&["--profile", "my work"]).contains("can’t be a profile name"));
        assert!(usage("witdo").starts_with("Usage: witdo [--profile <name>]"));
    }

    fn profiles(dir: &TempDir) -> Profiles {
        Profiles::at(dir.path().join("config"))
    }

    #[test]
    fn opening_a_workspace_remembers_it() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("work");
        let mut profile = ProfileSession::load(profiles(&dir), ProfileName::default()).unwrap();
        profile
            .create_workspace(&root, None, Clock::system())
            .unwrap();
        let root = root.canonicalize().unwrap();
        assert_eq!(profile.recent_workspaces(), std::slice::from_ref(&root));

        let again = ProfileSession::load(profiles(&dir), ProfileName::default()).unwrap();
        assert_eq!(
            again.config().last_workspace.as_deref(),
            Some(root.as_path())
        );
    }

    #[test]
    fn profiles_do_not_see_each_other() {
        let dir = TempDir::new().unwrap();
        let work: ProfileName = "work".parse().unwrap();
        let mut profile = ProfileSession::load(profiles(&dir), work).unwrap();
        profile
            .create_workspace(&dir.path().join("w"), None, Clock::system())
            .unwrap();
        let personal = ProfileSession::load(profiles(&dir), "personal".parse().unwrap()).unwrap();
        assert_eq!(personal.recent_workspaces(), Vec::<PathBuf>::new());
    }

    #[test]
    fn start_opens_the_last_workspace() {
        let dir = TempDir::new().unwrap();
        let clock = Clock::system();
        let (_, first) = start(&Launch::default(), profiles(&dir), &clock).unwrap();
        assert!(matches!(first, Opening::Choose));

        let launch = Launch {
            workspace: Some(dir.path().join("w")),
            ..Launch::default()
        };
        let (mut profile, opening) = start(&launch, profiles(&dir), &clock).unwrap();
        let Opening::Confirm { path, state } = opening else {
            panic!("should ask first")
        };
        assert_eq!(state, DirectoryState::Missing);
        profile
            .create_workspace(&path, None, clock.clone())
            .unwrap();

        let (_, again) = start(&Launch::default(), profiles(&dir), &clock).unwrap();
        assert!(matches!(again, Opening::Opened(_)));
    }

    #[test]
    fn start_says_when_the_last_workspace_is_gone() {
        let dir = TempDir::new().unwrap();
        let clock = Clock::system();
        let mut profile = ProfileSession::load(profiles(&dir), ProfileName::default()).unwrap();
        profile
            .create_workspace(&dir.path().join("w"), None, clock.clone())
            .unwrap();
        fs::remove_dir_all(dir.path().join("w")).unwrap();
        let (_, opening) = start(&Launch::default(), profiles(&dir), &clock).unwrap();
        assert!(matches!(opening, Opening::Failed(AppError::Workspace(_))));
    }

    #[test]
    fn forgetting_a_workspace_leaves_its_files() {
        let dir = TempDir::new().unwrap();
        let mut profile = ProfileSession::load(profiles(&dir), ProfileName::default()).unwrap();
        let session = profile
            .create_workspace(&dir.path().join("w"), None, Clock::system())
            .unwrap();
        let root = session.workspace().root().to_owned();
        profile.forget_workspace(&root).unwrap();
        assert_eq!(profile.recent_workspaces(), Vec::<PathBuf>::new());
        assert!(root.join(".witdo/settings.toml").exists());
    }
}
