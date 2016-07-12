use std::io;

#[derive(Debug)]
enum Contains<T, E> {
    VALUE(T),
    ERROR(E),
    NOTHING,
}

/// TODO(ptc) implement Try
#[derive(Debug)]
pub struct Try<T> {
    contains: Contains<T, io::Error>,
}

impl<T> Try<T> {
    pub fn new() -> Try<T> {
        Try { contains: Contains::NOTHING }
    }

    pub fn new_error(err: io::Error) -> Try<T> {
        Try { contains: Contains::ERROR(err) }
    }

    pub fn new_value(val: T) -> Try<T> {
        Try { contains: Contains::VALUE(val) }
    }

    pub fn has_error(&self) -> bool {
        match self.contains {
            Contains::ERROR(_) => true,
            _ => false,
        }
    }

    pub fn has_value(&self) -> bool {
        match self.contains {
            Contains::VALUE(_) => true,
            _ => false,
        }
    }

    pub fn get_error(self) -> io::Error {
        match self.contains {
            Contains::VALUE(_) => {
                io::Error::new(io::ErrorKind::Other, "Calling get_error on a succesful Try")
            }
            Contains::ERROR(err) => err,
            Contains::NOTHING => io::Error::new(io::ErrorKind::Other, "Using Uninitialized Try"),
        }
    }

    pub fn value(self) -> Result<T, io::Error> {
        match self.contains {
            Contains::VALUE(val) => Ok(val),
            Contains::ERROR(err) => Err(err),
            Contains::NOTHING => {
                Err(io::Error::new(io::ErrorKind::Other, "Using Uninitialized Try"))
            }
        }
    }
}

#[cfg(test)]
mod tests {

    use std::io;

    use super::Try;

    #[test]
    fn test_has_error_has_value() {
        let empty: Try<usize> = Try::new();
        assert_eq!(empty.has_value(), false);
        assert_eq!(empty.has_error(), false);
        let value: Try<usize> = Try::new_value(10);
        assert_eq!(value.has_value(), true);
        assert_eq!(value.has_error(), false);
        let error: Try<usize> = Try::new_error(io::Error::new(io::ErrorKind::Other, "error"));
        assert_eq!(error.has_value(), false);
        assert_eq!(error.has_error(), true);
    }
}
