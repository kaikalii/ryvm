/*!
This module contains the various types that can be put in ryvm spec files
*/

use std::collections::HashMap;

use serde_derive::{Deserialize, Serialize};

use crate::Spec;

/// Things that can be put inside a spec file
pub enum FileContents {
    /// A single spec
    Spec(Spec),
    /// A mapping of names to specs
    SpecMap(SpecMap),
}

/// A mapping of names to specs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecMap(HashMap<String, Spec>);

#[allow(clippy::implicit_hasher)]
impl From<SpecMap> for HashMap<String, Spec> {
    fn from(map: SpecMap) -> Self {
        map.0
    }
}
