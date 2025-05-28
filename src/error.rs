use std::io;

pub enum Error
{
    Io(io::Error),
    PathNotAbsolute,
    MissingPackages,
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
            Error::MissingPackages => write!(f, "At least one package is required"),
        }
    }
}
