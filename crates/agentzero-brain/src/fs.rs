use crate::BrainFs;

/// Real filesystem implementation of `BrainFs`.
pub struct RealBrainFs;

impl RealBrainFs {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RealBrainFs {
    fn default() -> Self {
        Self::new()
    }
}

impl BrainFs for RealBrainFs {
    fn read_file(&self, path: &str) -> Result<String, String> {
        std::fs::read_to_string(path).map_err(|e| format!("read {path}: {e}"))
    }

    fn write_file(&self, path: &str, content: &str) -> Result<bool, String> {
        std::fs::write(path, content).map_err(|e| format!("write {path}: {e}"))?;
        Ok(true)
    }

    fn append_file(&self, path: &str, content: &str) -> Result<bool, String> {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| format!("append {path}: {e}"))?;
        file.write_all(content.as_bytes())
            .map_err(|e| format!("append write {path}: {e}"))?;
        Ok(true)
    }

    fn list_dir(&self, path: &str) -> Result<Vec<String>, String> {
        let entries = std::fs::read_dir(path).map_err(|e| format!("list_dir {path}: {e}"))?;
        let mut result = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| format!("read entry: {e}"))?;
            if let Some(name) = entry.file_name().to_str() {
                result.push(name.to_string());
            }
        }
        result.sort();
        Ok(result)
    }

    fn create_dir(&self, path: &str) -> Result<bool, String> {
        std::fs::create_dir_all(path).map_err(|e| format!("create_dir {path}: {e}"))?;
        Ok(true)
    }

    fn file_exists(&self, path: &str) -> Result<bool, String> {
        Ok(std::path::Path::new(path).exists())
    }

    fn now(&self) -> String {
        chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()
    }
}
