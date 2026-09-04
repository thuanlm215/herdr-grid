use crate::model::{TemplateError, TemplateNode, MAX_TEMPLATE_SLOTS};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const MAX_SAVED_LAYOUTS: usize = 9;
pub const MAX_LAYOUT_NAME_CHARS: usize = 40;
const MAX_CATALOG_BYTES: u64 = 1024 * 1024;
const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedLayout {
    pub id: String,
    pub name: String,
    pub anchor_slot: u16,
    pub tree: TemplateNode,
    pub created_unix_ms: u64,
    pub updated_unix_ms: u64,
}

impl SavedLayout {
    pub fn slots(&self) -> usize {
        self.tree.slot_count()
    }

    pub fn validate(&self) -> Result<(), CatalogError> {
        validate_name(&self.name)?;
        if self.id.trim().is_empty() || self.id.len() > 128 {
            return Err(CatalogError::InvalidId);
        }
        self.tree.validate(self.anchor_slot)?;
        if self.slots() > MAX_TEMPLATE_SLOTS {
            return Err(CatalogError::InvalidLayout("too many slots".into()));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SavedCatalog {
    pub layouts: Vec<SavedLayout>,
}

#[derive(Serialize, Deserialize)]
struct CatalogFile {
    schema_version: u32,
    layouts: Vec<SavedLayout>,
}

#[derive(thiserror::Error, Debug)]
pub enum CatalogError {
    #[error("custom layout name cannot be empty")]
    EmptyName,
    #[error("custom layout name must be at most {MAX_LAYOUT_NAME_CHARS} characters")]
    NameTooLong,
    #[error("a custom layout named '{0}' already exists")]
    DuplicateName(String),
    #[error("custom layout limit reached ({MAX_SAVED_LAYOUTS})")]
    TooManyLayouts,
    #[error("invalid custom layout id")]
    InvalidId,
    #[error("invalid custom layout: {0}")]
    InvalidLayout(String),
    #[error("unsupported custom layout schema {0}; this file was not changed")]
    UnsupportedSchema(u32),
    #[error("custom layout file is larger than 1 MiB")]
    FileTooLarge,
    #[error("custom layout data is invalid: {0}")]
    InvalidCatalog(String),
    #[error("read custom layouts: {0}")]
    Read(#[source] std::io::Error),
    #[error("write custom layouts: {0}")]
    Write(#[source] std::io::Error),
    #[error(transparent)]
    Template(#[from] TemplateError),
}

impl SavedCatalog {
    pub fn add(
        &mut self,
        name: &str,
        tree: TemplateNode,
        anchor_slot: u16,
    ) -> Result<(), CatalogError> {
        let name = normalized_name(name)?;
        if self.layouts.len() >= MAX_SAVED_LAYOUTS {
            return Err(CatalogError::TooManyLayouts);
        }
        self.ensure_unique_name(&name, None)?;
        tree.validate(anchor_slot)?;
        let now = now_ms();
        let layout = SavedLayout {
            id: format!("layout-{}-{}", now_nanos(), std::process::id()),
            name,
            anchor_slot,
            tree,
            created_unix_ms: now,
            updated_unix_ms: now,
        };
        layout.validate()?;
        self.layouts.push(layout);
        Ok(())
    }

    pub fn rename(&mut self, index: usize, name: &str) -> Result<(), CatalogError> {
        let name = normalized_name(name)?;
        self.ensure_unique_name(&name, Some(index))?;
        let layout = self
            .layouts
            .get_mut(index)
            .ok_or_else(|| CatalogError::InvalidCatalog("layout no longer exists".into()))?;
        layout.name = name;
        layout.updated_unix_ms = now_ms();
        Ok(())
    }

    pub fn delete(&mut self, index: usize) -> Result<(), CatalogError> {
        if index >= self.layouts.len() {
            return Err(CatalogError::InvalidCatalog(
                "layout no longer exists".into(),
            ));
        }
        self.layouts.remove(index);
        Ok(())
    }

    fn ensure_unique_name(&self, name: &str, except: Option<usize>) -> Result<(), CatalogError> {
        if self.layouts.iter().enumerate().any(|(index, layout)| {
            Some(index) != except && layout.name.to_lowercase() == name.to_lowercase()
        }) {
            return Err(CatalogError::DuplicateName(name.into()));
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), CatalogError> {
        if self.layouts.len() > MAX_SAVED_LAYOUTS {
            return Err(CatalogError::TooManyLayouts);
        }
        let mut ids = HashSet::new();
        let mut names = HashSet::new();
        for layout in &self.layouts {
            layout.validate()?;
            if !ids.insert(layout.id.clone()) {
                return Err(CatalogError::InvalidCatalog(format!(
                    "duplicate id {}",
                    layout.id
                )));
            }
            if !names.insert(layout.name.to_lowercase()) {
                return Err(CatalogError::DuplicateName(layout.name.clone()));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct PresetStore {
    path: PathBuf,
}

impl PresetStore {
    pub fn from_env() -> Result<Self, CatalogError> {
        let directory = std::env::var_os("HERDR_PLUGIN_CONFIG_DIR")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("XDG_CONFIG_HOME")
                    .map(PathBuf::from)
                    .map(|path| path.join("herdr/plugins/config/herdr-grid"))
            })
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .map(|path| path.join(".config/herdr/plugins/config/herdr-grid"))
            })
            .ok_or_else(|| CatalogError::InvalidCatalog("no config directory available".into()))?;
        Ok(Self::new(directory.join("custom-layouts.json")))
    }

    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<SavedCatalog, CatalogError> {
        let metadata = match fs::metadata(&self.path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(SavedCatalog::default())
            }
            Err(error) => return Err(CatalogError::Read(error)),
        };
        if metadata.len() > MAX_CATALOG_BYTES {
            return Err(CatalogError::FileTooLarge);
        }
        let bytes = fs::read(&self.path).map_err(CatalogError::Read)?;
        let file: CatalogFile = serde_json::from_slice(&bytes)
            .map_err(|error| CatalogError::InvalidCatalog(error.to_string()))?;
        if file.schema_version != SCHEMA_VERSION {
            return Err(CatalogError::UnsupportedSchema(file.schema_version));
        }
        let catalog = SavedCatalog {
            layouts: file.layouts,
        };
        catalog.validate()?;
        Ok(catalog)
    }

    pub fn save(&self, catalog: &SavedCatalog) -> Result<(), CatalogError> {
        catalog.validate()?;
        let bytes = serde_json::to_vec_pretty(&CatalogFile {
            schema_version: SCHEMA_VERSION,
            layouts: catalog.layouts.clone(),
        })
        .map_err(|error| CatalogError::InvalidCatalog(error.to_string()))?;
        if bytes.len() as u64 > MAX_CATALOG_BYTES {
            return Err(CatalogError::FileTooLarge);
        }
        let parent = self
            .path
            .parent()
            .ok_or_else(|| CatalogError::InvalidCatalog("invalid storage path".into()))?;
        fs::create_dir_all(parent).map_err(CatalogError::Write)?;
        let temp = parent.join(format!(
            ".custom-layouts.{}.{}.tmp",
            std::process::id(),
            now_ms()
        ));
        let result = (|| {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temp)
                .map_err(CatalogError::Write)?;
            file.write_all(&bytes).map_err(CatalogError::Write)?;
            file.sync_all().map_err(CatalogError::Write)?;
            fs::rename(&temp, &self.path).map_err(CatalogError::Write)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }
}

fn normalized_name(name: &str) -> Result<String, CatalogError> {
    let name = name.trim();
    validate_name(name)?;
    Ok(name.into())
}

fn validate_name(name: &str) -> Result<(), CatalogError> {
    if name.trim().is_empty() {
        return Err(CatalogError::EmptyName);
    }
    if name.chars().count() > MAX_LAYOUT_NAME_CHARS {
        return Err(CatalogError::NameTooLong);
    }
    Ok(())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LayoutNode;

    fn catalog() -> SavedCatalog {
        let tree = LayoutNode::Pane {
            pane_id: "p1".into(),
        };
        let (tree, anchor) = TemplateNode::capture(&tree, "p1").unwrap();
        let mut catalog = SavedCatalog::default();
        catalog.add("Solo", tree, anchor).unwrap();
        catalog
    }

    #[test]
    fn round_trip_and_atomic_replace() {
        let directory = tempfile::tempdir().unwrap();
        let store = PresetStore::new(directory.path().join("custom-layouts.json"));
        let expected = catalog();
        store.save(&expected).unwrap();
        assert_eq!(store.load().unwrap(), expected);
        assert!(directory.path().read_dir().unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp")));
    }

    #[test]
    fn malformed_and_unknown_schema_are_never_rewritten_by_load() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("custom-layouts.json");
        fs::write(&path, b"not json").unwrap();
        let store = PresetStore::new(path.clone());
        assert!(matches!(store.load(), Err(CatalogError::InvalidCatalog(_))));
        assert_eq!(fs::read(&path).unwrap(), b"not json");

        fs::write(&path, br#"{"schema_version":99,"layouts":[]}"#).unwrap();
        assert!(matches!(
            store.load(),
            Err(CatalogError::UnsupportedSchema(99))
        ));
        assert_eq!(
            fs::read_to_string(path).unwrap(),
            r#"{"schema_version":99,"layouts":[]}"#
        );
    }

    #[test]
    fn names_are_trimmed_unique_and_rename_delete_work() {
        let mut catalog = catalog();
        assert!(matches!(
            catalog.add(
                " solo ",
                catalog.layouts[0].tree.clone(),
                catalog.layouts[0].anchor_slot
            ),
            Err(CatalogError::DuplicateName(_))
        ));
        catalog.rename(0, "Focused").unwrap();
        assert_eq!(catalog.layouts[0].name, "Focused");
        catalog.delete(0).unwrap();
        assert!(catalog.layouts.is_empty());
    }

    #[test]
    fn catalog_is_limited_to_one_nine_card_gallery() {
        let mut catalog = catalog();
        let tree = catalog.layouts[0].tree.clone();
        let anchor = catalog.layouts[0].anchor_slot;
        for index in 2..=MAX_SAVED_LAYOUTS {
            catalog
                .add(&format!("Layout {index}"), tree.clone(), anchor)
                .unwrap();
        }
        assert!(matches!(
            catalog.add("One too many", tree, anchor),
            Err(CatalogError::TooManyLayouts)
        ));
    }
}
