use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Value {
    Text(String),
    Number(f64),
    Bool(bool),
    Null,
    Array(Vec<Value>),
    Object(HashMap<String, Value>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DataKind {
    CsvRow,
    JsonNode,
    TomlEntry,
    YamlNode,
    TsvRow,
    Custom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataTile {
    pub id: Uuid,
    pub kind: DataKind,
    pub values: HashMap<String, Value>,
    pub index: u64,
    pub schema_key: String,
    pub meta: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataStats {
    pub row_count: usize,
    pub field_count: usize,
    pub null_counts: HashMap<String, usize>,
    pub type_counts: HashMap<String, usize>,
}

pub struct DataDecomposer;

impl DataDecomposer {
    /// Detect the data format from the first bytes of input.
    pub fn detect_format(input: &str) -> DataKind {
        let trimmed = input.trim_start();
        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            DataKind::JsonNode
        } else if trimmed.contains('\t') {
            // Heuristic: if tabs are present before any comma, likely TSV
            let first_line = trimmed.lines().next().unwrap_or("");
            if first_line.contains('\t') && !first_line.contains(',') {
                DataKind::TsvRow
            } else {
                DataKind::CsvRow
            }
        } else {
            DataKind::CsvRow
        }
    }

    /// Parse CSV input into a vector of DataTiles.
    pub fn parse_csv(input: &str) -> Vec<DataTile> {
        let mut lines = input.lines().filter(|l| !l.trim().is_empty()).peekable();
        let header_line = match lines.next() {
            Some(h) => h,
            None => return Vec::new(),
        };
        let headers = split_csv_row(header_line);
        let schema_key = headers.join(",");

        lines
            .enumerate()
            .map(|(i, line)| {
                let fields = split_csv_row(line);
                let mut values = HashMap::new();
                for (j, header) in headers.iter().enumerate() {
                    let raw = fields.get(j).cloned().unwrap_or_default();
                    values.insert(header.clone(), parse_value(&raw));
                }
                DataTile {
                    id: Uuid::new_v4(),
                    kind: DataKind::CsvRow,
                    values,
                    index: i as u64,
                    schema_key: schema_key.clone(),
                    meta: HashMap::new(),
                }
            })
            .collect()
    }

    /// Parse JSON input into a vector of DataTiles.
    /// Top-level objects become tiles; arrays of objects become multiple tiles.
    pub fn parse_json(input: &str) -> Vec<DataTile> {
        let val: serde_json::Value = match serde_json::from_str(input) {
            Ok(v) => v,
            Err(_) => return Vec::new(),
        };
        match val {
            serde_json::Value::Array(arr) => arr
                .into_iter()
                .enumerate()
                .filter_map(|(i, v)| json_to_tile(v, i as u64))
                .collect(),
            other => {
                let mut tiles = Vec::new();
                if let Some(t) = json_to_tile(other, 0) {
                    tiles.push(t);
                }
                tiles
            }
        }
    }

    /// Parse TSV input into a vector of DataTiles.
    pub fn parse_tsv(input: &str) -> Vec<DataTile> {
        let mut lines = input.lines().filter(|l| !l.trim().is_empty()).peekable();
        let header_line = match lines.next() {
            Some(h) => h,
            None => return Vec::new(),
        };
        let headers: Vec<String> = header_line.split('\t').map(|s| s.trim().to_string()).collect();
        let schema_key = headers.join("\t");

        lines
            .enumerate()
            .map(|(i, line)| {
                let fields: Vec<&str> = line.split('\t').collect();
                let mut values = HashMap::new();
                for (j, header) in headers.iter().enumerate() {
                    let raw = fields.get(j).map(|s| s.trim()).unwrap_or("");
                    values.insert(header.clone(), parse_value(raw));
                }
                DataTile {
                    id: Uuid::new_v4(),
                    kind: DataKind::TsvRow,
                    values,
                    index: i as u64,
                    schema_key: schema_key.clone(),
                    meta: HashMap::new(),
                }
            })
            .collect()
    }

    /// Filter tiles by a field value using the given comparison operation.
    /// Ops: "eq", "gt", "lt", "contains"
    pub fn filter(tiles: &[DataTile], field: &str, op: &str, value: &str) -> Vec<DataTile> {
        tiles
            .iter()
            .filter(|t| {
                let tile_val = match t.values.get(field) {
                    Some(v) => v,
                    None => return false,
                };
                match op {
                    "eq" => value_eq(tile_val, value),
                    "gt" => value_gt(tile_val, value),
                    "lt" => value_lt(tile_val, value),
                    "contains" => value_contains(tile_val, value),
                    _ => false,
                }
            })
            .cloned()
            .collect()
    }

    /// Sort tiles by a field. If `desc` is true, sort descending.
    pub fn sort(tiles: &mut [DataTile], field: &str, desc: bool) -> Vec<DataTile> {
        tiles.sort_by(|a, b| {
            let va = a.values.get(field);
            let vb = b.values.get(field);
            let ord = compare_values(va, vb);
            if desc { ord.reverse() } else { ord }
        });
        tiles.to_vec()
    }

    /// Aggregate a numeric field across tiles.
    /// Ops: "sum", "mean", "min", "max", "count"
    pub fn aggregate(tiles: &[DataTile], field: &str, op: &str) -> f64 {
        let nums: Vec<f64> = tiles
            .iter()
            .filter_map(|t| {
                if let Some(Value::Number(n)) = t.values.get(field) {
                    Some(*n)
                } else if let Some(Value::Text(s)) = t.values.get(field) {
                    s.parse::<f64>().ok()
                } else {
                    None
                }
            })
            .collect();

        match op {
            "sum" => nums.iter().sum(),
            "mean" => {
                if nums.is_empty() {
                    0.0
                } else {
                    nums.iter().sum::<f64>() / nums.len() as f64
                }
            }
            "min" => nums.iter().cloned().fold(f64::INFINITY, f64::min),
            "max" => nums.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
            "count" => nums.len() as f64,
            _ => 0.0,
        }
    }

    /// Reconstruct CSV string from tiles.
    pub fn reconstruct_csv(tiles: &[DataTile]) -> String {
        if tiles.is_empty() {
            return String::new();
        }
        let fields = Self::schema(tiles);
        let mut lines = Vec::new();
        lines.push(fields.join(","));

        for tile in tiles {
            let row: Vec<String> = fields
                .iter()
                .map(|f| {
                    match tile.values.get(f) {
                        Some(Value::Text(s)) => {
                            if s.contains(',') || s.contains('"') || s.contains('\n') {
                                format!("\"{}\"", s.replace('"', "\"\""))
                            } else {
                                s.clone()
                            }
                        }
                        Some(Value::Number(n)) => format!("{}", n),
                        Some(Value::Bool(b)) => b.to_string(),
                        Some(Value::Null) => String::new(),
                        Some(Value::Array(arr)) => format!("{:?}", arr),
                        Some(Value::Object(obj)) => format!("{:?}", obj),
                        None => String::new(),
                    }
                })
                .collect();
            lines.push(row.join(","));
        }
        lines.join("\n")
    }

    /// Reconstruct JSON string from tiles (array of objects).
    pub fn reconstruct_json(tiles: &[DataTile]) -> String {
        let arr: Vec<serde_json::Map<String, serde_json::Value>> = tiles
            .iter()
            .map(|tile| {
                let mut map = serde_json::Map::new();
                for (k, v) in &tile.values {
                    map.insert(k.clone(), tile_value_to_json(v));
                }
                map
            })
            .collect();
        serde_json::to_string_pretty(&serde_json::Value::Array(
            arr.into_iter().map(serde_json::Value::Object).collect(),
        ))
        .unwrap_or_else(|_| "[]".to_string())
    }

    /// Extract all unique field names across tiles.
    pub fn schema(tiles: &[DataTile]) -> Vec<String> {
        let mut fields: Vec<String> = tiles
            .iter()
            .flat_map(|t| t.values.keys().cloned())
            .collect();
        fields.sort();
        fields.dedup();
        fields
    }

    /// Compute stats over a set of tiles.
    pub fn stats(tiles: &[DataTile]) -> DataStats {
        let fields = Self::schema(tiles);
        let mut null_counts = HashMap::new();
        let mut type_counts = HashMap::new();

        for field in &fields {
            let nulls = tiles
                .iter()
                .filter(|t| matches!(t.values.get(field), Some(Value::Null) | None))
                .count();
            null_counts.insert(field.clone(), nulls);
        }

        for tile in tiles {
            for (_, v) in &tile.values {
                let type_name = match v {
                    Value::Text(_) => "Text",
                    Value::Number(_) => "Number",
                    Value::Bool(_) => "Bool",
                    Value::Null => "Null",
                    Value::Array(_) => "Array",
                    Value::Object(_) => "Object",
                };
                *type_counts.entry(type_name.to_string()).or_insert(0) += 1;
            }
        }

        DataStats {
            row_count: tiles.len(),
            field_count: fields.len(),
            null_counts,
            type_counts,
        }
    }
}

// --- Helper functions ---

fn split_csv_row(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();

    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    current.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                current.push(c);
            }
        } else if c == '"' {
            in_quotes = true;
        } else if c == ',' {
            fields.push(current.trim().to_string());
            current = String::new();
        } else {
            current.push(c);
        }
    }
    fields.push(current.trim().to_string());
    fields
}

fn parse_value(raw: &str) -> Value {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Value::Null;
    }
    if trimmed.eq_ignore_ascii_case("true") {
        return Value::Bool(true);
    }
    if trimmed.eq_ignore_ascii_case("false") {
        return Value::Bool(false);
    }
    if let Ok(n) = trimmed.parse::<f64>() {
        return Value::Number(n);
    }
    Value::Text(trimmed.to_string())
}

fn json_to_tile(val: serde_json::Value, index: u64) -> Option<DataTile> {
    match val {
        serde_json::Value::Object(map) => {
            let mut values = HashMap::new();
            for (k, v) in map {
                values.insert(k, json_value_to_value(v));
            }
            let schema_key = {
                let mut keys: Vec<&String> = values.keys().collect();
                keys.sort();
                keys.iter().map(|k| k.as_str()).collect::<Vec<_>>().join(",")
            };
            Some(DataTile {
                id: Uuid::new_v4(),
                kind: DataKind::JsonNode,
                values,
                index,
                schema_key,
                meta: HashMap::new(),
            })
        }
        serde_json::Value::Array(arr) => {
            let values = HashMap::from([(
                "items".to_string(),
                Value::Array(arr.into_iter().map(json_value_to_value).collect()),
            )]);
            Some(DataTile {
                id: Uuid::new_v4(),
                kind: DataKind::JsonNode,
                values,
                index,
                schema_key: "items".to_string(),
                meta: HashMap::new(),
            })
        }
        _ => None,
    }
}

fn json_value_to_value(v: serde_json::Value) -> Value {
    match v {
        serde_json::Value::String(s) => Value::Text(s),
        serde_json::Value::Number(n) => Value::Number(n.as_f64().unwrap_or(0.0)),
        serde_json::Value::Bool(b) => Value::Bool(b),
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Array(arr) => {
            Value::Array(arr.into_iter().map(json_value_to_value).collect())
        }
        serde_json::Value::Object(map) => {
            Value::Object(map.into_iter().map(|(k, v)| (k, json_value_to_value(v))).collect())
        }
    }
}

fn tile_value_to_json(v: &Value) -> serde_json::Value {
    match v {
        Value::Text(s) => serde_json::Value::String(s.clone()),
        Value::Number(n) => serde_json::json!(*n),
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Null => serde_json::Value::Null,
        Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(tile_value_to_json).collect())
        }
        Value::Object(map) => serde_json::Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), tile_value_to_json(v)))
                .collect(),
        ),
    }
}

fn value_eq(v: &Value, target: &str) -> bool {
    match v {
        Value::Text(s) => s == target,
        Value::Number(n) => target.parse::<f64>().map(|t| (n - t).abs() < f64::EPSILON).unwrap_or(false),
        Value::Bool(b) => target.eq_ignore_ascii_case(&b.to_string()),
        Value::Null => target.is_empty() || target.eq_ignore_ascii_case("null"),
        _ => false,
    }
}

fn value_gt(v: &Value, target: &str) -> bool {
    match v {
        Value::Number(n) => target.parse::<f64>().map(|t| *n > t).unwrap_or(false),
        Value::Text(s) => s.as_str() > target,
        _ => false,
    }
}

fn value_lt(v: &Value, target: &str) -> bool {
    match v {
        Value::Number(n) => target.parse::<f64>().map(|t| *n < t).unwrap_or(false),
        Value::Text(s) => s.as_str() < target,
        _ => false,
    }
}

fn value_contains(v: &Value, target: &str) -> bool {
    match v {
        Value::Text(s) => s.contains(target),
        _ => false,
    }
}

fn compare_values(a: Option<&Value>, b: Option<&Value>) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, _) => Ordering::Less,
        (_, None) => Ordering::Greater,
        (Some(Value::Number(na)), Some(Value::Number(nb))) => na.partial_cmp(nb).unwrap_or(Ordering::Equal),
        (Some(Value::Text(sa)), Some(Value::Text(sb))) => sa.cmp(sb),
        (Some(Value::Bool(ba)), Some(Value::Bool(bb))) => ba.cmp(bb),
        _ => Ordering::Equal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_csv_basic() {
        let input = "name,age,city\nAlice,30,NYC\nBob,25,LA";
        let tiles = DataDecomposer::parse_csv(input);
        assert_eq!(tiles.len(), 2);
        assert_eq!(tiles[0].kind, DataKind::CsvRow);
        assert!(matches!(tiles[0].values.get("name"), Some(Value::Text(s)) if s == "Alice"));
        assert!(matches!(tiles[0].values.get("age"), Some(Value::Number(n)) if *n == 30.0));
    }

    #[test]
    fn test_parse_csv_quoted() {
        let input = "name,desc\nAlice,\"Hello, World\"\nBob,\"He said \"\"hi\"\"\"";
        let tiles = DataDecomposer::parse_csv(input);
        assert_eq!(tiles.len(), 2);
        assert!(matches!(tiles[0].values.get("desc"), Some(Value::Text(s)) if s == "Hello, World"));
        assert!(matches!(tiles[1].values.get("desc"), Some(Value::Text(s)) if s == "He said \"hi\""));
    }

    #[test]
    fn test_parse_csv_empty() {
        let tiles = DataDecomposer::parse_csv("");
        assert!(tiles.is_empty());
    }

    #[test]
    fn test_parse_json_array() {
        let input = r#"[{"name":"Alice","age":30},{"name":"Bob","age":25}]"#;
        let tiles = DataDecomposer::parse_json(input);
        assert_eq!(tiles.len(), 2);
        assert_eq!(tiles[0].kind, DataKind::JsonNode);
        assert!(matches!(tiles[0].values.get("name"), Some(Value::Text(s)) if s == "Alice"));
    }

    #[test]
    fn test_parse_json_single_object() {
        let input = r#"{"name":"Alice","age":30}"#;
        let tiles = DataDecomposer::parse_json(input);
        assert_eq!(tiles.len(), 1);
        assert!(matches!(tiles[0].values.get("name"), Some(Value::Text(s)) if s == "Alice"));
    }

    #[test]
    fn test_parse_json_nested() {
        let input = r#"[{"name":"Alice","addr":{"city":"NYC"}}]"#;
        let tiles = DataDecomposer::parse_json(input);
        assert_eq!(tiles.len(), 1);
        assert!(matches!(tiles[0].values.get("addr"), Some(Value::Object(_))));
    }

    #[test]
    fn test_parse_tsv() {
        let input = "name\tage\tcity\nAlice\t30\tNYC\nBob\t25\tLA";
        let tiles = DataDecomposer::parse_tsv(input);
        assert_eq!(tiles.len(), 2);
        assert_eq!(tiles[0].kind, DataKind::TsvRow);
        assert!(matches!(tiles[0].values.get("name"), Some(Value::Text(s)) if s == "Alice"));
        assert!(matches!(tiles[0].values.get("age"), Some(Value::Number(n)) if *n == 30.0));
    }

    #[test]
    fn test_detect_csv() {
        assert_eq!(DataDecomposer::detect_format("a,b\n1,2"), DataKind::CsvRow);
    }

    #[test]
    fn test_detect_json() {
        assert_eq!(DataDecomposer::detect_format("[1,2]"), DataKind::JsonNode);
        assert_eq!(DataDecomposer::detect_format("{\"a\":1}"), DataKind::JsonNode);
    }

    #[test]
    fn test_detect_tsv() {
        assert_eq!(DataDecomposer::detect_format("a\tb\n1\t2"), DataKind::TsvRow);
    }

    #[test]
    fn test_filter_eq() {
        let input = "name,age\nAlice,30\nBob,25\nAlice,35";
        let tiles = DataDecomposer::parse_csv(input);
        let filtered = DataDecomposer::filter(&tiles, "name", "eq", "Alice");
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_filter_gt() {
        let input = "name,age\nAlice,30\nBob,25\nCarol,35";
        let tiles = DataDecomposer::parse_csv(input);
        let filtered = DataDecomposer::filter(&tiles, "age", "gt", "28");
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_filter_contains() {
        let input = "name,city\nAlice,New York\nBob,Los Angeles\nCarol,New Orleans";
        let tiles = DataDecomposer::parse_csv(input);
        let filtered = DataDecomposer::filter(&tiles, "city", "contains", "New");
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_sort_asc() {
        let input = "name,age\nAlice,30\nBob,25\nCarol,35";
        let mut tiles = DataDecomposer::parse_csv(input);
        let sorted = DataDecomposer::sort(&mut tiles, "age", false);
        assert!(matches!(sorted[0].values.get("name"), Some(Value::Text(s)) if s == "Bob"));
        assert!(matches!(sorted[2].values.get("name"), Some(Value::Text(s)) if s == "Carol"));
    }

    #[test]
    fn test_sort_desc() {
        let input = "name,age\nAlice,30\nBob,25\nCarol,35";
        let mut tiles = DataDecomposer::parse_csv(input);
        let sorted = DataDecomposer::sort(&mut tiles, "age", true);
        assert!(matches!(sorted[0].values.get("name"), Some(Value::Text(s)) if s == "Carol"));
    }

    #[test]
    fn test_aggregate_sum() {
        let input = "name,score\nAlice,90\nBob,80\nCarol,70";
        let tiles = DataDecomposer::parse_csv(input);
        let sum = DataDecomposer::aggregate(&tiles, "score", "sum");
        assert!((sum - 240.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_aggregate_mean() {
        let input = "name,score\nAlice,90\nBob,80\nCarol,70";
        let tiles = DataDecomposer::parse_csv(input);
        let mean = DataDecomposer::aggregate(&tiles, "score", "mean");
        assert!((mean - 80.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_aggregate_count() {
        let input = "name,score\nAlice,90\nBob,80";
        let tiles = DataDecomposer::parse_csv(input);
        let count = DataDecomposer::aggregate(&tiles, "score", "count");
        assert_eq!(count, 2.0);
    }

    #[test]
    fn test_reconstruct_csv() {
        let input = "name,age\nAlice,30\nBob,25";
        let tiles = DataDecomposer::parse_csv(input);
        let out = DataDecomposer::reconstruct_csv(&tiles);
        // schema() sorts alphabetically, so header is "age,name"
        assert!(out.contains("age,name"));
        assert!(out.contains("30,Alice"));
        assert!(out.contains("25,Bob"));
    }

    #[test]
    fn test_reconstruct_json() {
        let input = r#"[{"name":"Alice","age":30}]"#;
        let tiles = DataDecomposer::parse_json(input);
        let out = DataDecomposer::reconstruct_json(&tiles);
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(parsed.is_array());
        assert_eq!(parsed[0]["name"], "Alice");
    }

    #[test]
    fn test_schema() {
        let input = "name,age,city\nAlice,30,NYC";
        let tiles = DataDecomposer::parse_csv(input);
        let schema = DataDecomposer::schema(&tiles);
        assert_eq!(schema, vec!["age", "city", "name"]);
    }

    #[test]
    fn test_stats() {
        let input = "name,age\nAlice,30\nBob,\nCarol,25";
        let tiles = DataDecomposer::parse_csv(input);
        let stats = DataDecomposer::stats(&tiles);
        assert_eq!(stats.row_count, 3);
        assert_eq!(stats.field_count, 2);
        assert_eq!(*stats.null_counts.get("age").unwrap_or(&0), 1);
    }

    #[test]
    fn test_parse_csv_whitespace_rows() {
        let input = "name,age\n\nAlice,30\n\nBob,25\n";
        let tiles = DataDecomposer::parse_csv(input);
        assert_eq!(tiles.len(), 2);
    }

    #[test]
    fn test_filter_lt() {
        let input = "name,age\nAlice,30\nBob,25\nCarol,35";
        let tiles = DataDecomposer::parse_csv(input);
        let filtered = DataDecomposer::filter(&tiles, "age", "lt", "30");
        assert_eq!(filtered.len(), 1);
        assert!(matches!(filtered[0].values.get("name"), Some(Value::Text(s)) if s == "Bob"));
    }

    #[test]
    fn test_aggregate_min_max() {
        let input = "name,val\nA,10\nB,50\nC,30";
        let tiles = DataDecomposer::parse_csv(input);
        let min = DataDecomposer::aggregate(&tiles, "val", "min");
        let max = DataDecomposer::aggregate(&tiles, "val", "max");
        assert!((min - 10.0).abs() < f64::EPSILON);
        assert!((max - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_json_bool_null() {
        let input = r#"[{"active":true,"note":null}]"#;
        let tiles = DataDecomposer::parse_json(input);
        assert_eq!(tiles.len(), 1);
        assert!(matches!(tiles[0].values.get("active"), Some(Value::Bool(true))));
        assert!(matches!(tiles[0].values.get("note"), Some(Value::Null)));
    }

    #[test]
    fn test_parse_value_bool_and_null() {
        assert!(matches!(parse_value("true"), Value::Bool(true)));
        assert!(matches!(parse_value("FALSE"), Value::Bool(false)));
        assert!(matches!(parse_value(""), Value::Null));
        assert!(matches!(parse_value("42.5"), Value::Number(n) if (n - 42.5).abs() < f64::EPSILON));
    }
}
