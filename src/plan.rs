use std::collections::{BTreeMap, HashMap};
use std::ffi::OsString;
use std::fs::{create_dir, read_dir, remove_dir, remove_file};
use std::path::{Path, PathBuf};

use crate::cli::StowArgs;
use crate::error::Error;
use crate::fs::{
    BasePath, TargetPath, is_owned_by_stow_directory, relative_path, resolve_link, symlink,
};

enum Action {
    RemoveSymlink(PathBuf),
    CreateDirectory(PathBuf),
    CreateSymlink { path: PathBuf, target: PathBuf },
    RemoveDirectory(PathBuf),
}

#[derive(Clone)]
pub enum Entry {
    Missing,
    File,
    Directory,
    Symlink(PathBuf),
}

#[derive(Clone)]
struct OverlayEntry {
    entry: Entry,
    read_real_directory: bool,
}

pub struct Plan {
    stow_dir: PathBuf,
    target_dir: PathBuf,
    actions: Vec<Action>,
    overlay: HashMap<PathBuf, OverlayEntry>,
}

impl Plan {
    pub fn from_args(args: &StowArgs) -> Result<Self, Error> {
        let cwd = std::env::current_dir()?;
        let stow_dir = args.package_dir.as_ref().unwrap_or(&cwd).canonicalize()?;
        let target_dir = match &args.target_dir {
            Some(target_dir) => target_dir.canonicalize()?,
            None => stow_dir
                .parent()
                .ok_or(Error::DefaultTargetNotAvailable)?
                .canonicalize()?,
        };

        Ok(Self {
            stow_dir,
            target_dir,
            actions: Vec::new(),
            overlay: HashMap::new(),
        })
    }

    pub fn stow_dir(&self) -> &Path {
        &self.stow_dir
    }

    pub fn target_dir(&self) -> &Path {
        &self.target_dir
    }

    pub fn entry(&self, path: &Path) -> Result<Entry, Error> {
        if let Some(entry) = self.overlay.get(path) {
            return Ok(entry.entry.clone());
        }

        if path.is_symlink() {
            Ok(Entry::Symlink(resolve_link(path)?))
        } else if path.is_dir() {
            Ok(Entry::Directory)
        } else if path.is_file() || path.exists() {
            Ok(Entry::File)
        } else {
            Ok(Entry::Missing)
        }
    }

    pub fn read_directory(&self, path: &Path) -> Result<BTreeMap<OsString, PathBuf>, Error> {
        let mut entries = BTreeMap::new();
        let read_real = self
            .overlay
            .get(path)
            .is_none_or(|entry| entry.read_real_directory);

        if read_real && path.is_dir() && !path.is_symlink() {
            for entry in read_dir(path)? {
                let entry = entry?;
                entries.insert(entry.file_name(), entry.path());
            }
        }

        for (entry_path, state) in &self.overlay {
            if entry_path.parent() != Some(path) {
                continue;
            }
            let Some(name) = entry_path.file_name() else {
                continue;
            };
            match state.entry {
                Entry::Missing => {
                    entries.remove(name);
                }
                _ => {
                    entries.insert(name.to_os_string(), entry_path.clone());
                }
            }
        }

        Ok(entries)
    }

    pub fn remove_symlink(mut self, path: PathBuf) -> Self {
        self.actions.push(Action::RemoveSymlink(path.clone()));
        self.overlay.insert(
            path,
            OverlayEntry {
                entry: Entry::Missing,
                read_real_directory: false,
            },
        );
        self
    }

    pub fn create_directory(mut self, path: PathBuf) -> Self {
        self.actions.push(Action::CreateDirectory(path.clone()));
        self.overlay.insert(
            path,
            OverlayEntry {
                entry: Entry::Directory,
                read_real_directory: false,
            },
        );
        self
    }

    pub fn create_symlink(mut self, path: PathBuf, source: &Path) -> Result<Self, Error> {
        let resolved_source = source.canonicalize()?;
        let parent = path.parent().ok_or(Error::DefaultTargetNotAvailable)?;
        let target = relative_path(TargetPath(source), BasePath(parent))?;
        self.actions.push(Action::CreateSymlink {
            path: path.clone(),
            target,
        });
        self.overlay.insert(
            path,
            OverlayEntry {
                entry: Entry::Symlink(resolved_source),
                read_real_directory: false,
            },
        );
        Ok(self)
    }

    pub fn remove_directory(mut self, path: PathBuf) -> Self {
        self.actions.push(Action::RemoveDirectory(path.clone()));
        self.overlay.insert(
            path,
            OverlayEntry {
                entry: Entry::Missing,
                read_real_directory: false,
            },
        );
        self
    }

    pub fn is_owned_source(&self, source: &Path) -> bool {
        is_owned_by_stow_directory(source, &self.stow_dir)
    }

    pub fn execute(self, simulate: bool, verbose: bool) -> Result<(), Error> {
        for action in self.actions {
            match action {
                Action::RemoveSymlink(path) => {
                    if simulate {
                        println!("remove symlink({:?})", path);
                    } else {
                        if verbose {
                            println!("Removing symlink: {:?}", path);
                        }
                        if path.is_dir() {
                            remove_dir(path)?;
                        } else {
                            remove_file(path)?;
                        }
                    }
                }
                Action::CreateDirectory(path) => {
                    if simulate {
                        println!("mkdir({:?})", path);
                    } else {
                        if verbose {
                            println!("Creating directory: {:?}", path);
                        }
                        create_dir(path)?;
                    }
                }
                Action::CreateSymlink { path, target } => {
                    if simulate {
                        println!("symlink({:?}, {:?})", path, target);
                    } else {
                        if verbose {
                            println!("Creating symlink: {:?} -> {:?}", path, target);
                        }
                        symlink(target, path)?;
                    }
                }
                Action::RemoveDirectory(path) => {
                    if simulate {
                        println!("rmdir({:?})", path);
                    } else {
                        if verbose {
                            println!("Removing directory: {:?}", path);
                        }
                        remove_dir(path)?;
                    }
                }
            }
        }

        Ok(())
    }
}
