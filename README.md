# forge-data

Structured data decomposition into tiles for Plato agents.

## Overview

`forge-data` takes common data formats (CSV, JSON, TSV) and decomposes them into **DataTiles** — structured, schema-aware units that can be filtered, sorted, aggregated, and reconstructed.

Each tile carries:
- A unique ID (`Uuid`)
- A `DataKind` tag (CsvRow, JsonNode, TsvRow, etc.)
- A `HashMap<String, Value>` of field → value pairs
- An index (original position)
- A schema key (ordered field names)
- Arbitrary metadata

## Installation

```toml
[dependencies]
forge-data = { git = "https://github.com/SuperInstance/forge-data" }
```

## Usage

```rust
use forge_data::{DataDecomposer, DataTile, DataKind};

// Parse CSV
let tiles = DataDecomposer::parse_csv("name,age\nAlice,30\nBob,25");

// Detect format
let kind = DataDecomposer::detect_format("[1,2,3]"); // JsonNode

// Filter
let filtered = DataDecomposer::filter(&tiles, "age", "gt", "25");

// Sort
let sorted = DataDecomposer::sort(&mut tiles.clone(), "age", true);

// Aggregate
let avg = DataDecomposer::aggregate(&tiles, "age", "mean");

// Reconstruct
let csv_out = DataDecomposer::reconstruct_csv(&tiles);
let json_out = DataDecomposer::reconstruct_json(&tiles);

// Schema & stats
let fields = DataDecomposer::schema(&tiles);
let stats = DataDecomposer::stats(&tiles);
```

## API

| Method | Description |
|--------|-------------|
| `parse_csv(input)` | Parse CSV into tiles |
| `parse_json(input)` | Parse JSON into tiles |
| `parse_tsv(input)` | Parse TSV into tiles |
| `detect_format(input)` | Sniff input format |
| `filter(tiles, field, op, value)` | Filter by field (eq/gt/lt/contains) |
| `sort(tiles, field, desc)` | Sort by field |
| `aggregate(tiles, field, op)` | Aggregate (sum/mean/min/max/count) |
| `reconstruct_csv(tiles)` | Tiles → CSV string |
| `reconstruct_json(tiles)` | Tiles → JSON string |
| `schema(tiles)` | Unique field names |
| `stats(tiles)` | Row/field counts, null/type distributions |

## Dependencies

- `serde` + `serde_json` — serialization
- `uuid` — unique tile IDs

No external CSV/YAML parsing libraries. All formats handled manually.

## License

MIT
