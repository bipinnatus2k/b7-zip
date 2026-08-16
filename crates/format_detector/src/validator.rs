use std::collections::HashMap;
use std::path::Path;
use crate::archive_format::ArchiveFormat;

/// Pure-Rust structural validator for a single archive format.
#[async_trait::async_trait]
pub trait FormatValidator: Send + Sync {
    fn format(&self) -> ArchiveFormat;
    async fn validate(&self, path: &Path, data: &[u8]) -> Result<(), Vec<String>>;
}

/// Registry of registered format validators.
pub struct ValidatorRegistry {
    validators: HashMap<ArchiveFormat, Box<dyn FormatValidator>>,
}

impl ValidatorRegistry {
    pub fn new() -> Self {
        Self {
            validators: HashMap::new(),
        }
    }

    pub fn register(&mut self, v: Box<dyn FormatValidator>) {
        self.validators.insert(v.format(), v);
    }

    pub fn get(&self, fmt: ArchiveFormat) -> Option<&dyn FormatValidator> {
        self.validators.get(&fmt).map(|b| b.as_ref())
    }

    pub fn validate(
        &self,
        fmt: ArchiveFormat,
        path: &Path,
        data: &[u8],
    ) -> Result<(), Vec<String>> {
        match self.validators.get(&fmt) {
            Some(v) => futures::executor::block_on(v.validate(path, data)),
            None => Ok(()),
        }
    }
}

impl Default for ValidatorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use crate::archive_format::ArchiveFormat;
    use super::*;

    struct MockZipValidator;

    #[async_trait::async_trait]
    impl FormatValidator for MockZipValidator {
        fn format(&self) -> ArchiveFormat {
            ArchiveFormat::Zip
        }
        async fn validate(&self, _path: &Path, data: &[u8]) -> Result<(), Vec<String>> {
            if data.len() < 4 {
                return Err(vec!["too short".to_string()]);
            }
            Ok(())
        }
    }

    #[test]
    fn test_registry_validate_passes() {
        let mut registry = ValidatorRegistry::new();
        registry.register(Box::new(MockZipValidator));
        assert!(registry
            .validate(
                ArchiveFormat::Zip,
                Path::new(""),
                &[0x50, 0x4B, 0x03, 0x04]
            )
            .is_ok());
    }

    #[test]
    fn test_registry_validate_fails() {
        let mut registry = ValidatorRegistry::new();
        registry.register(Box::new(MockZipValidator));
        assert!(registry
            .validate(ArchiveFormat::Zip, Path::new(""), &[0x00])
            .is_err());
    }

    #[test]
    fn test_registry_no_validator_is_ok() {
        let registry = ValidatorRegistry::new();
        assert!(registry
            .validate(ArchiveFormat::SevenZip, Path::new(""), &[])
            .is_ok());
    }
}
