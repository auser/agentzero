use agentzero_storage::EncryptedJsonStore;
use anyhow::{anyhow, Context};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Peripheral {
    pub id: String,
    pub kind: String,
    pub connection: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeripheralRegistry {
    pub peripherals: Vec<Peripheral>,
}

impl PeripheralRegistry {
    pub fn load(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let store = store_for(path)?;
        store
            .load_optional::<Self>()
            .with_context(|| {
                format!(
                    "failed to parse peripheral registry {}",
                    store.path().display()
                )
            })?
            .map_or_else(|| Ok(Self::default()), Ok)
    }

    pub fn save(&self, path: impl AsRef<Path>) -> anyhow::Result<()> {
        let path = path.as_ref();
        let store = store_for(path)?;
        store.save(self).with_context(|| {
            format!(
                "failed to write peripheral registry {}",
                store.path().display()
            )
        })
    }

    pub fn add(&mut self, peripheral: Peripheral) -> anyhow::Result<()> {
        if peripheral.id.trim().is_empty() {
            return Err(anyhow!("peripheral id cannot be empty"));
        }
        if self.peripherals.iter().any(|p| p.id == peripheral.id) {
            return Err(anyhow!("peripheral already exists: {}", peripheral.id));
        }
        self.peripherals.push(peripheral);
        Ok(())
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.peripherals.len();
        self.peripherals.retain(|p| p.id != id);
        before != self.peripherals.len()
    }
}

fn store_for(path: &Path) -> anyhow::Result<EncryptedJsonStore> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow!("invalid peripheral registry path: {}", path.display()))?;
    EncryptedJsonStore::in_config_dir(parent, file_name)
}

#[cfg(test)]
mod tests {
    use super::{Peripheral, PeripheralRegistry};
    use std::fs;

    #[test]
    fn add_save_load_remove_success_path() {
        let tmp = tempfile::tempdir().expect("tempdir should be created");
        let path = tmp.path().join("peripherals.json");

        let mut reg = PeripheralRegistry::default();
        reg.add(Peripheral {
            id: "uart0".to_string(),
            kind: "uart".to_string(),
            connection: "serial:///dev/ttyUSB0".to_string(),
        })
        .expect("add should succeed");
        reg.save(&path).expect("save should succeed");

        let mut loaded = PeripheralRegistry::load(&path).expect("load should succeed");
        assert_eq!(loaded.peripherals.len(), 1);
        assert!(loaded.remove("uart0"));
    }

    #[test]
    fn add_duplicate_rejected_negative_path() {
        let mut reg = PeripheralRegistry::default();
        reg.add(Peripheral {
            id: "uart0".to_string(),
            kind: "uart".to_string(),
            connection: "serial:///dev/ttyUSB0".to_string(),
        })
        .expect("first add should succeed");

        let err = reg
            .add(Peripheral {
                id: "uart0".to_string(),
                kind: "uart".to_string(),
                connection: "serial:///dev/ttyUSB0".to_string(),
            })
            .expect_err("duplicate add should fail");
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn registry_is_encrypted_at_rest_success_path() {
        let tmp = tempfile::tempdir().expect("tempdir should be created");
        let path = tmp.path().join("peripherals.json");

        let mut reg = PeripheralRegistry::default();
        reg.add(Peripheral {
            id: "uart0".to_string(),
            kind: "uart".to_string(),
            connection: "serial:///dev/ttyUSB0".to_string(),
        })
        .expect("add should succeed");
        reg.save(&path).expect("save should succeed");

        let on_disk = fs::read_to_string(&path).expect("registry file should be readable");
        assert!(!on_disk.contains("serial:///dev/ttyUSB0"));
        assert!(!on_disk.contains("uart0"));
    }
}
