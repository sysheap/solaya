use crate::klibc::runtime_initialized::RuntimeInitializedData;
use alloc::vec::Vec;

use super::page_tables::MappingDescription;

static RUNTIME_MAPPINGS: RuntimeInitializedData<Vec<MappingDescription>> =
    RuntimeInitializedData::new();

pub fn initialize_runtime_mappings(mappings: &[MappingDescription]) {
    RUNTIME_MAPPINGS.initialize(mappings.to_vec());
}

pub fn get_runtime_mappings() -> &'static [MappingDescription] {
    &RUNTIME_MAPPINGS
}
