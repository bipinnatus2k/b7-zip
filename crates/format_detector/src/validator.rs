use crate::archive_format::ArchiveFormat;
use std::collections::HashMap;
use std::path::Path;

/// Pure-Rust structural validator for a single archive format.
pub trait FormatValidator: Send + Sync {
    fn format(&self) -> ArchiveFormat;
    fn validate(&self, path: &Path) -> Result<(), Vec<String>>;
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

    pub fn validate(&self, fmt: ArchiveFormat, path: &Path) -> Result<(), Vec<String>> {
        match self.validators.get(&fmt) {
            Some(v) => v.validate(path),
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
    use super::*;
    use crate::archive_format::ArchiveFormat;

    struct MockZipValidator;

    impl FormatValidator for MockZipValidator {
        fn format(&self) -> ArchiveFormat {
            ArchiveFormat::Zip
        }
        fn validate(&self, _path: &Path) -> Result<(), Vec<String>> {
            Ok(())
        }
    }

    #[test]
    fn test_registry_validate_passes() {
        let mut registry = ValidatorRegistry::new();
        registry.register(Box::new(MockZipValidator));
        assert!(registry.validate(ArchiveFormat::Zip, Path::new("")).is_ok());
    }

    #[test]
    fn test_registry_no_validator_is_ok() {
        let registry = ValidatorRegistry::new();
        assert!(
            registry
                .validate(ArchiveFormat::SevenZip, Path::new(""))
                .is_ok()
        );
    }
}
