use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use ropey::Rope;

use crate::RyvmResult;

pub struct FlyControl {
    pub file: PathBuf,
    pub line: usize,
    pub column: usize,
    pub rope: Arc<Mutex<Rope>>,
}

const FLY_PATTERN: &str = "#";

impl FlyControl {
    pub fn find<P>(path: P) -> RyvmResult<Vec<Self>>
    where
        P: AsRef<Path>,
    {
        let mut file_str = fs::read_to_string(&path)?;
        if !file_str.contains(FLY_PATTERN) {
            return Ok(Vec::new());
        }

        unimplemented!()
    }
}
