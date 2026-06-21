use std::io::BufRead;
use std::path::PathBuf;

use serde_json_path::JsonPath;

use crate::RmlError;
use crate::sources::SourceRow;

/// JSON row — wraps one JSON object (or any JSON value) from the source.
///
/// References are JSONPath expressions evaluated against the wrapped value.
/// Bare field names without a `$` prefix (e.g. `"name"`) are auto-prefixed
/// to `"$.name"` before evaluation.
pub struct JsonRow(pub serde_json::Value);

impl SourceRow for JsonRow {
    fn get_str(&self, reference: &str) -> Option<String> {
        let path_str = if reference.starts_with('$') {
            reference.to_string()
        } else {
            format!("$.{reference}")
        };
        let path = JsonPath::parse(&path_str).ok()?;
        let node = path.query(&self.0).first()?;
        match node {
            serde_json::Value::String(s) => {
                if s.is_empty() { None } else { Some(s.clone()) }
            }
            serde_json::Value::Number(n) => Some(n.to_string()),
            serde_json::Value::Bool(b) => Some(b.to_string()),
            _ => None,
        }
    }
}

pub enum JsonFormat {
    Json,
    Jsonl,
}

pub struct JsonSource {
    pub path: PathBuf,
    pub format: JsonFormat,
    pub iterator: Option<String>,
}

impl JsonSource {
    pub fn new(path: PathBuf) -> Self {
        let format = if path.extension().map_or(false, |e| e == "jsonl" || e == "ndjson") {
            JsonFormat::Jsonl
        } else {
            JsonFormat::Json
        };
        JsonSource { path, format, iterator: None }
    }

    pub fn with_iterator(mut self, iterator: String) -> Self {
        self.iterator = Some(iterator);
        self
    }

    pub fn rows(&self) -> Box<dyn Iterator<Item = Result<JsonRow, RmlError>> + '_> {
        match self.collect_rows() {
            Ok(rows) => Box::new(rows.into_iter().map(Ok)),
            Err(e) => Box::new(std::iter::once(Err(e))),
        }
    }

    fn collect_rows(&self) -> Result<Vec<JsonRow>, RmlError> {
        match self.format {
            JsonFormat::Json => self.collect_json_rows(),
            JsonFormat::Jsonl => self.collect_jsonl_rows(),
        }
    }

    fn collect_json_rows(&self) -> Result<Vec<JsonRow>, RmlError> {
        let content = std::fs::read_to_string(&self.path)?;
        let doc: serde_json::Value =
            serde_json::from_str(&content).map_err(|e| RmlError::Json {
                file: self.path.clone(),
                source: e,
            })?;

        if let Some(iter_path) = &self.iterator {
            let path = JsonPath::parse(iter_path).map_err(|e| {
                RmlError::MappingParse(format!("invalid JSONPath iterator '{iter_path}': {e}"))
            })?;
            Ok(path.query(&doc).iter().map(|v| JsonRow((*v).clone())).collect())
        } else {
            match doc {
                serde_json::Value::Array(arr) => Ok(arr.into_iter().map(JsonRow).collect()),
                other => Ok(vec![JsonRow(other)]),
            }
        }
    }

    fn collect_jsonl_rows(&self) -> Result<Vec<JsonRow>, RmlError> {
        let file = std::fs::File::open(&self.path)?;
        let reader = std::io::BufReader::new(file);
        let mut rows = Vec::new();

        for line in reader.lines() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let value: serde_json::Value =
                serde_json::from_str(trimmed).map_err(|e| RmlError::Json {
                    file: self.path.clone(),
                    source: e,
                })?;

            if let Some(iter_path) = &self.iterator {
                let path = JsonPath::parse(iter_path).map_err(|e| {
                    RmlError::MappingParse(format!("invalid JSONPath iterator '{iter_path}': {e}"))
                })?;
                for node in path.query(&value).iter() {
                    rows.push(JsonRow((*node).clone()));
                }
            } else {
                rows.push(JsonRow(value));
            }
        }

        Ok(rows)
    }
}
