use std::io;

#[derive(Debug)]
pub(crate) enum Error {
    Io(io::Error),
    Cargo(failure::Error),
    Other(String),
}

impl From<failure::Error> for Error {
    fn from(src: failure::Error) -> Error {
        Error::Cargo(src)
    }
}

impl From<io::Error> for Error {
    fn from(src: io::Error) -> Error {
        Error::Io(src)
    }
}
