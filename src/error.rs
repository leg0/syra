use std::{io, path::PathBuf};

pub enum Error
{
    Io(io::Error),
    PathNotAbsolute,
    DefaultTargetNotAvailable,
    MissingPackages,
    LinkPathExists(PathBuf),
    LinkNotOwnedByPackage(PathBuf, String),
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::Io(err)
    }
}

impl std::fmt::Debug for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "IO error: {}", e),
            Error::PathNotAbsolute => write!(f, "Path is not absolute"),
            Error::DefaultTargetNotAvailable => write!(f, "Default target directory is not available"),
            Error::MissingPackages => write!(f, "At least one package is required"),
            Error::LinkPathExists(path) => write!(f, "Link path already exists: {}", path.display()),
            Error::LinkNotOwnedByPackage(path, pkg) => write!(
                f,
                "Link path '{}' is not owned by package '{}'",
                path.display(),
                pkg
            ),
        }
    }
}
