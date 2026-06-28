use std::path::PathBuf;

use super::RawRow;
use crate::RmlError;

pub struct CsvSource {
    pub path: PathBuf,
    pub delimiter: u8,
    /// Override for the default MAX_SOURCE_BYTES limit (used in tests). See [#86](https://github.com/daghovland/rdf-datalog/issues/86).
    pub size_limit: Option<u64>,
    /// Override for the default MAX_SOURCE_ROWS limit (used in tests). See [#86](https://github.com/daghovland/rdf-datalog/issues/86).
    pub row_limit: Option<usize>,
}

impl CsvSource {
    pub fn new(path: PathBuf) -> Self {
        CsvSource {
            path,
            delimiter: b',',
            size_limit: None,
            row_limit: None,
        }
    }

    pub fn with_delimiter(mut self, delimiter: u8) -> Self {
        self.delimiter = delimiter;
        self
    }

    /// Set a custom byte size limit (overrides [`crate::MAX_SOURCE_BYTES`]).
    pub fn with_size_limit(mut self, bytes: u64) -> Self {
        self.size_limit = Some(bytes);
        self
    }

    /// Set a custom row count limit (overrides [`crate::MAX_SOURCE_ROWS`]).
    pub fn with_row_limit(mut self, rows: usize) -> Self {
        self.row_limit = Some(rows);
        self
    }

    pub fn rows(&self) -> Box<dyn Iterator<Item = Result<RawRow, RmlError>> + '_> {
        match self.collect_rows() {
            Ok(rows) => Box::new(rows.into_iter().map(Ok)),
            Err(e) => Box::new(std::iter::once(Err(e))),
        }
    }

    fn collect_rows(&self) -> Result<Vec<RawRow>, RmlError> {
        let mut reader = csv::ReaderBuilder::new()
            .delimiter(self.delimiter)
            .from_path(&self.path)
            .map_err(|e| RmlError::Csv {
                file: self.path.clone(),
                source: e,
            })?;

        let headers: Vec<String> = reader
            .headers()
            .map_err(|e| RmlError::Csv {
                file: self.path.clone(),
                source: e,
            })?
            .iter()
            .map(|s| s.to_string())
            .collect();

        let mut rows = Vec::new();
        for record in reader.records() {
            let record = record.map_err(|e| RmlError::Csv {
                file: self.path.clone(),
                source: e,
            })?;
            let mut row = RawRow::new();
            for (header, value) in headers.iter().zip(record.iter()) {
                row.insert(header.clone(), value.to_string());
            }
            rows.push(row);
        }

        Ok(rows)
    }
}
