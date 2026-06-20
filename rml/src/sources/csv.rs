use std::path::PathBuf;

use super::RawRow;
use crate::RmlError;

pub struct CsvSource {
    pub path: PathBuf,
    pub delimiter: u8,
}

impl CsvSource {
    pub fn new(path: PathBuf) -> Self {
        CsvSource { path, delimiter: b',' }
    }

    pub fn with_delimiter(mut self, delimiter: u8) -> Self {
        self.delimiter = delimiter;
        self
    }

    pub fn rows(&self) -> Box<dyn Iterator<Item = Result<RawRow, RmlError>> + '_> {
        todo!()
    }
}
