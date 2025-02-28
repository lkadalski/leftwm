//! Starts programs in autostart, runs global 'up' script, and boots theme. Provides function to
//! boot other desktop files also.
use crate::errors::Result;
use crate::models::Manager;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::iter::{Extend, FromIterator};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{atomic::AtomicBool, Arc};
use xdg::BaseDirectories;

#[derive(Default)]
pub struct Nanny {}

impl Nanny {
    #[must_use]
    pub fn autostart() -> Children {
        dirs_next::home_dir()
            .map(|mut path| {
                path.push(".config");
                path.push("autostart");
                path
            })
            .and_then(|path| list_desktop_files(&path).ok())
            .map(|files| {
                files
                    .iter()
                    .filter_map(|file| boot_desktop_file(file).ok())
                    .collect::<Children>()
            })
            .unwrap_or_default()
    }

    /// Retrieve the path to the config directory. Tries to create it if it does not exist.
    ///
    /// # Errors
    ///
    /// Will error if unable to open or create the config directory.
    /// Could be caused by inadequate permissions.
    fn get_config_dir() -> Result<PathBuf> {
        BaseDirectories::with_prefix("leftwm")?
            .create_config_directory("")
            .map_err(|e| e.into())
    }

    /// Runs a script if it exits
    fn run_script(path: &Path) -> Result<Option<Child>> {
        if path.is_file() {
            Command::new(&path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .spawn()
                .map(Some)
                .map_err(|e| e.into())
        } else {
            Ok(None)
        }
    }

    /// Runs the 'up' script in the config directory, if there is one.
    ///
    /// # Errors
    ///
    /// Will error if unable to open current config directory.
    /// Could be caused by inadequate permissions.
    pub fn run_global_up_script() -> Result<Option<Child>> {
        let mut path = Self::get_config_dir()?;
        path.push("up");
        Self::run_script(&path)
    }

    /// Runs the 'up' script of the current theme, if there is one.
    ///
    /// # Errors
    ///
    /// Will error if unable to open current theme directory.
    /// Could be caused by inadequate permissions.
    pub fn boot_current_theme() -> Result<Option<Child>> {
        let mut path = Self::get_config_dir()?;
        path.push("themes");
        path.push("current");
        path.push("up");
        Self::run_script(&path)
    }
}

fn boot_desktop_file(path: &Path) -> std::io::Result<Child> {
    let args = format!( "`if [ \"$(grep '^X-GNOME-Autostart-enabled' {:?} | tail -1 | sed 's/^X-GNOME-Autostart-enabled=//' | tr '[A-Z]' '[a-z]')\" != 'false' ]; then grep '^Exec' {:?} | tail -1 | sed 's/^Exec=//' | sed 's/%.//' | sed 's/^\"//g' | sed 's/\" *$//g'; else echo 'exit'; fi`", path , path);
    Command::new("sh").arg("-c").arg(args).spawn()
}

// get all the .desktop files in a folder
fn list_desktop_files(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut list = vec![];
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension() {
                    if ext == "desktop" {
                        list.push(path);
                    }
                }
            }
        }
    }
    Ok(list)
}

/// A struct managing children processes.
///
/// The `reap` method could be called at any place the user wants to.
/// `register_child_hook` provides a hook that sets a flag. User may use the
/// flag to do a epoch-based reaping.
#[derive(Debug, Default)]
pub struct Children {
    inner: HashMap<u32, Child>,
}

impl Children {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.len()
    }
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.len() == 0
    }
    /// Insert a `Child` in the `Children`.
    /// If this `Children` did not have this value present, true is returned.
    /// If this `Children` did have this value present, false is returned.
    pub fn insert(&mut self, child: Child) -> bool {
        // Not possible to have duplication!
        self.inner.insert(child.id(), child).is_none()
    }
    /// Merge another `Children` into this `Children`.
    pub fn merge(&mut self, reaper: Self) {
        self.inner.extend(reaper.inner.into_iter());
    }
    /// Try reaping all the children processes managed by this struct.
    pub fn reap(&mut self) {
        // The `try_wait` needs `child` to be `mut`, but only `HashMap::retain`
        // allows modifying the value. Here `id` is not needed.
        self.inner
            .retain(|_, child| child.try_wait().map_or(true, |ret| ret.is_none()));
    }
}

impl FromIterator<Child> for Children {
    fn from_iter<T: IntoIterator<Item = Child>>(iter: T) -> Self {
        Self {
            inner: iter
                .into_iter()
                .map(|child| (child.id(), child))
                .collect::<HashMap<_, _>>(),
        }
    }
}

impl Extend<Child> for Children {
    fn extend<T: IntoIterator<Item = Child>>(&mut self, iter: T) {
        self.inner
            .extend(iter.into_iter().map(|child| (child.id(), child)));
    }
}

/// Register the `SIGCHLD` signal handler. Once the signal is received,
/// the flag will be set true. User needs to manually clear the flag.
pub fn register_child_hook(flag: Arc<AtomicBool>) {
    let _ = signal_hook::flag::register(signal_hook::consts::signal::SIGCHLD, flag)
        .map_err(|err| log::error!("Cannot register SIGCHLD signal handler: {:?}", err));
}

/// Sends command to shell for execution
/// Assumes STDIN/STDOUT unwanted.
pub fn exec_shell(command: &str, manager: &mut Manager) -> Option<u32> {
    let child = Command::new("sh")
        .arg("-c")
        .arg(&command)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .ok()?;
    let pid = child.id();
    manager.children.insert(child);
    Some(pid)
}
