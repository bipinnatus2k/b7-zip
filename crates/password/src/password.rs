use std::fmt;
use secrecy::{ExposeSecret, SecretString};

/// A password that is zeroized on drop and redacted in debug output.
#[derive(Clone)]
pub struct Password(SecretString);

unsafe impl Send for Password {}
unsafe impl Sync for Password {}

impl Password {
    pub fn new(password: impl Into<String>) -> Self {
        Self(password.into().into())
    }

    pub fn as_str(&self) -> &str {
        self.0.expose_secret()
    }

    pub fn is_empty(&self) -> bool {
        self.0.expose_secret().is_empty()
    }

    pub fn empty() -> Self {
        Self("".into())
    }
}

impl fmt::Debug for Password {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Password([redacted])")
    }
}
