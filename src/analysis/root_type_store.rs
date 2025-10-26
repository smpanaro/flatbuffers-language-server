use crate::symbol_table::RootTypeInfo;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Default)]
pub struct RootTypeStore {
    pub root_types: HashMap<PathBuf, RootTypeInfo>,
}
