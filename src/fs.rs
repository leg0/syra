use std::fs::{create_dir, read_link, remove_dir, remove_file};
use std::io;
use std::path::{Path, PathBuf};

use crate::error::Error;

#[cfg(test)]
use std::path::Component;

pub struct Symlink {
    pub path: PathBuf,
    pub target: PathBuf,
}

pub enum Action {
    RemoveSymlink(PathBuf),
    CreateDirectory(PathBuf),
    CreateSymlink(Symlink),
    RemoveDirectory(PathBuf),
}

pub struct BasePath<'a>(pub &'a Path);
pub struct TargetPath<'a>(pub &'a Path);

/// Returns the path to `target` relative to `base`.
///
/// For example:
/// target: /home/user/project/src
/// base:   /home/user/docs
/// result: ../project/src
pub fn relative_path(target: TargetPath, base: BasePath) -> Result<PathBuf, Error> {
    let BasePath(base) = base;
    let TargetPath(target) = target;

    if !target.is_absolute() || !base.is_absolute() {
        return Err(Error::PathNotAbsolute);
    }

    let target_components: Vec<_> = target.components().collect();
    let base_components: Vec<_> = base.components().collect();

    let common_prefix_len = target_components
        .iter()
        .zip(&base_components)
        .take_while(|(a, b)| a == b)
        .count();

    let mut result = PathBuf::new();

    // Add ".." for each remaining component in `base`
    for _ in base_components.iter().skip(common_prefix_len) {
        result.push("..");
    }

    // Add the remaining components from `target`
    for comp in target_components.iter().skip(common_prefix_len) {
        result.push(comp.as_os_str());
    }

    Ok(result)
}

/// normalize - like canonicalize, but does not fail if the path does not exist
#[cfg(test)]
pub fn normalize_path<P: AsRef<Path>>(path: P) -> PathBuf {
    let path = path.as_ref();
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir => normalized.push("/"),
            Component::Normal(part) => {
                if !part.is_empty() {
                    normalized.push(part)
                }
            }
            Component::ParentDir => {
                normalized.pop();
            }
            Component::CurDir => {}
            _ => {}
        }
    }
    normalized
}

/// Creates a symbolic link from `src` to `dst`.
/// Automatically detects whether the source is a file or directory on Windows.
pub fn symlink<P: AsRef<Path>, Q: AsRef<Path>>(src: P, dst: Q) -> Result<(), io::Error> {
    let src = src.as_ref();
    let dst = dst.as_ref();

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        symlink(src, dst)
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::{symlink_dir, symlink_file};
        let source_path = if src.is_absolute() {
            src.to_path_buf()
        } else {
            dst.parent()
                .ok_or_else(|| io::Error::other("symlink destination has no parent"))?
                .join(src)
        };
        if source_path.is_dir() {
            symlink_dir(src, dst)
        } else {
            symlink_file(src, dst)
        }
    }
}

pub fn resolve_link(path: &Path) -> Result<PathBuf, Error> {
    let target = read_link(path)?;
    let resolved = if target.is_absolute() {
        target
    } else {
        path.parent()
            .ok_or(Error::DefaultTargetNotAvailable)?
            .join(target)
    };
    Ok(resolved.canonicalize()?)
}

pub fn is_owned_by_stow_directory(path: &Path, stow_dir: &Path) -> bool {
    let Ok(path) = path.canonicalize() else {
        return false;
    };
    let Ok(stow_dir) = stow_dir.canonicalize() else {
        return false;
    };
    let Ok(relative) = path.strip_prefix(&stow_dir) else {
        return false;
    };
    let Some(package_name) = relative.components().next() else {
        return false;
    };
    stow_dir.join(package_name.as_os_str()).is_dir()
}

pub fn execute_actions(actions: &[Action], simulate: bool, verbose: bool) -> Result<(), Error> {
    for action in actions {
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
            Action::CreateSymlink(Symlink { path, target }) => {
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

pub trait Package {
    fn get_package_contents(&self) -> Result<Vec<PathBuf>, Error>;
    fn path(&self) -> &Path;
}

pub struct PackageImpl {
    path: PathBuf,
}

impl Package for PackageImpl {
    fn get_package_contents(&self) -> Result<Vec<PathBuf>, Error> {
        let package_dir = &self.path;
        if !package_dir.is_absolute() {
            return Err(Error::PathNotAbsolute);
        }

        let mut contents = Vec::new();
        for entry in package_dir.read_dir()? {
            contents.push(PathBuf::from(entry?.file_name()));
        }

        Ok(contents)
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl PackageImpl {
    pub fn new(package_dir: &Path, name: &str) -> Result<Self, Error> {
        if !package_dir.is_absolute() {
            return Err(Error::PathNotAbsolute);
        }

        let package_path = package_dir.join(name);
        if !package_path.exists() {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!(
                    "Package '{}' not found in '{}'",
                    name,
                    package_dir.display()
                ),
            )));
        }

        Ok(Self {
            path: package_path.canonicalize()?,
        })
    }
}

pub trait Target {
    fn path(&self) -> &Path;
}

pub struct TargetImpl {
    path: PathBuf,
}

impl Target for TargetImpl {
    fn path(&self) -> &Path {
        &self.path
    }
}

impl TargetImpl {
    pub fn new(path: &Path) -> Result<Self, Error> {
        if !path.is_absolute() {
            Err(Error::PathNotAbsolute)
        } else {
            let path = path.canonicalize()?;
            Ok(Self { path })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_common_prefix() {
        let target = Path::new("/home/user/project/src");
        let base = Path::new("/home/user/docs");
        assert_eq!(
            relative_path(TargetPath(&target), BasePath(&base)).unwrap(),
            PathBuf::from("../project/src")
        );
    }

    #[test]
    fn test_no_common_prefix() {
        let target = Path::new("/a/b/c");
        let base = Path::new("/x/y/z");
        assert_eq!(
            relative_path(TargetPath(&target), BasePath(&base)).unwrap(),
            PathBuf::from("../../../a/b/c")
        );
    }

    #[test]
    fn test_identical_paths() {
        let target = Path::new("/same/path");
        let base = Path::new("/same/path");
        assert_eq!(
            relative_path(TargetPath(&target), BasePath(&base)).unwrap(),
            PathBuf::from("")
        );
    }

    #[test]
    fn test_target_inside_base() {
        let target = Path::new("/a/b/c/d");
        let base = Path::new("/a/b");
        assert_eq!(
            relative_path(TargetPath(&target), BasePath(&base)).unwrap(),
            PathBuf::from("c/d")
        );
    }

    #[test]
    fn test_base_inside_target() {
        let target = Path::new("/a/b");
        let base = Path::new("/a/b/c/d");
        assert_eq!(
            relative_path(TargetPath(&target), BasePath(&base)).unwrap(),
            PathBuf::from("../../")
        );
    }

    #[test]
    fn test_error_on_relative_target() {
        let target = Path::new("a/b/c");
        let base = Path::new("/a/b");
        match relative_path(TargetPath(&target), BasePath(&base)) {
            Err(Error::PathNotAbsolute) => (),
            _ => assert!(false, "Expected PathNotAbsolute error"),
        }
    }

    #[test]
    fn test_error_on_relative_base() {
        let target = Path::new("/a/b/c");
        let base = Path::new("a/b");
        match relative_path(TargetPath(&target), BasePath(&base)) {
            Err(Error::PathNotAbsolute) => (),
            _ => assert!(false, "Expected PathNotAbsolute error"),
        }
    }

    #[test]
    fn test_normalize_path1() {
        let path = Path::new("/");
        let normalized = normalize_path(path);
        assert_eq!(normalized, PathBuf::from("/"));
    }
    #[test]
    fn test_normalize_path2() {
        let path = Path::new("/abc");
        let normalized = normalize_path(path);
        assert_eq!(normalized, PathBuf::from("/abc"));
    }
    #[test]
    fn test_normalize_path3() {
        let path = Path::new("/abc/..");
        let normalized = normalize_path(path);
        assert_eq!(normalized, PathBuf::from("/"));
    }
    #[test]
    fn test_normalize_path4() {
        let path = Path::new("/abc/.");
        let normalized = normalize_path(path);
        assert_eq!(normalized, PathBuf::from("/abc"));
    }
    #[test]
    fn test_normalize_path5() {
        let path = Path::new("/abc///def");
        let normalized = normalize_path(path);
        assert_eq!(normalized, PathBuf::from("/abc/def"));
    }
    #[test]
    fn test_normalize_path6() {
        let path = Path::new("/abc/def/../../../../qwe");
        let normalized = normalize_path(path);
        assert_eq!(normalized, PathBuf::from("/qwe"));
    }
}
