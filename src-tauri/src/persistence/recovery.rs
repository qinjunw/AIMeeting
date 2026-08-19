use std::path::{Path, PathBuf};

pub fn recovery_parts(directory: &Path) -> std::io::Result<Vec<PathBuf>> {
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let mut parts = std::fs::read_dir(directory)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "part")
        })
        .collect::<Vec<_>>();
    parts.sort();
    Ok(parts)
}
