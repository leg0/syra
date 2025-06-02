use std::io;
use std::path::{Path, PathBuf};

use crate::error::Error;

pub struct Symlink {
    pub path: PathBuf,
    pub target: PathBuf,
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
        if src.is_dir() {
            symlink_dir(src, dst)
        } else {
            symlink_file(src, dst)
        }
    }
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
        let mut iter = package_dir.read_dir()?;
        while let Some(entry) = iter.next() {
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

        Ok(Self { path: package_path.canonicalize()? })
    }
}

pub struct Target {
    path: PathBuf,
}

impl Target {
    // pub fn new(path: &Path) -> Result<Self, Error> {
    //     if !path.is_absolute() {
    //         return Err(Error::PathNotAbsolute);
    //     }
    //     Ok(Self {
    //         path: path.into(),
    //     })
    // }

    // pub fn get_installed_package_contents<P: Package>(&self, package: &P) -> Result<Vec<PathBuf>, Error> {
    //     // return paths to items that are installed in the target directory.
    //     todo!("Implement logic to get installed package contents");
    // }

    // pub fn path(&self) -> &Path {
    //     &self.path
    // }
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
}
