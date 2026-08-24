use std::io;
use std::path::Path;
use collections::HashMap;

pub fn init_paths() -> HashMap<io::ErrorKind, Vec<&'static Path>> {
    [
        paths::config_dir(),
        paths::extensions_dir(),
        paths::languages_dir(),
        paths::debug_adapters_dir(),
        paths::database_dir(),
        paths::logs_dir(),
        paths::temp_dir(),
        paths::hang_traces_dir(),
    ]
        .into_iter()
        .fold(HashMap::default(), |mut errors, path| {
            if let Err(e) = std::fs::create_dir_all(path) {
                errors.entry(e.kind()).or_insert_with(Vec::new).push(path);
            }
            errors
        })
}