

#[derive(Debug)]
enum Contains<T, E> {
    VALUE(T),
    ERROR(E),
    NOTHING,
}

/// TODO(ptc) implement Try
#[derive(Debug)]
pub struct Try<T, E> {
    contains : Contains<T, E>,
}

impl<T, E> Try<T, E> {
    pub fn new() -> Try<T, E> {
        Try {
            contains : Contains::NOTHING,
        }
    }

    pub fn new_error(err : E) -> Try<T, E> {
        Try {
            contains : Contains::ERROR(err),
        }
    }

    pub fn new_value(val : T) -> Try<T, E> {
        Try {
            contains : Contains::VALUE(val),
        }
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
}

#[cfg(test)]
mod tests {

    use std::io;

    use super::{Try};

    #[test]
    fn test_has_error_has_value() {
        let empty : Try<usize, io::Error> = Try::new();
        assert_eq!(empty.has_value(), false);
        assert_eq!(empty.has_error(), false);
        let value : Try<usize, io::Error> = Try::new_value(10);
        assert_eq!(value.has_value(), true);
        assert_eq!(value.has_error(), false);
        let error : Try<usize, io::Error> = Try::new_error(
            io::Error::new(io::ErrorKind::Other, "error"));
        assert_eq!(error.has_value(), false);
        assert_eq!(error.has_error(), true);
    }
}
