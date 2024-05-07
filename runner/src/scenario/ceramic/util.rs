use goose::GooseError;

pub fn goose_error(err: anyhow::Error) -> GooseError {
    GooseError::Io(std::io::Error::new(std::io::ErrorKind::Other, err))
}

/// Macro to transform errors from an expression to a goose transaction failure
#[macro_export]
macro_rules! goose_try {
    ($user:ident, $tag:expr, $request:expr, $func:expr) => {
        match $func {
            Ok(ret) => Ok(ret),
            Err(e) => {
                let err = e.to_string();
                if let Err(e) = $user.set_failure($tag, $request, None, Some(&err)) {
                    Err(e)
                } else {
                    panic!("Unreachable")
                }
            }
        }
    };
}
