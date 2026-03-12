# AL Symbols MCP Server (Rust)

A high-performance [Model Context Protocol (MCP)](https://modelcontextprotocol.io/) server for analyzing Business Central AL packages (`.app` files). Built in Rust for maximum performance and minimal resource usage.

This is a Rust reimplementation of [StefanMaron/AL-Dependency-MCP-Server](https://github.com/StefanMaron/AL-Dependency-MCP-Server), delivering the same functionality with significantly better performance characteristics.

## Features

- **Auto-discovery** of `.alpackages` directories and VS Code `al.packageCachePath` settings
- **ZIP extraction** from compiled `.app` files (handles 40-byte NAVX headers and signed packages)
- **Full symbol parsing** of `SymbolReference.json` with namespace support
- **Dependency resolution** with topological sorting and circular dependency detection
- **In-memory indexed database** with O(1) lookups by name, type, and ID
- **7 MCP tools** for comprehensive AL object analysis
- **Free ID lookup** from `app.json` `idRanges` with per-type filtering

## Performance

| Metric | TypeScript | Rust |
|--------|-----------|------|
| Binary size | ~100MB (Node.js) | ~3.8MB |
| Startup time | ~500ms | ~5ms |
| Memory usage | Higher (V8 GC) | Minimal (zero-copy where possible) |
| Package loading | Sequential/limited concurrency | Parallel with Rayon |

## Installation

### From source

```bash
cargo install --path .
```

### Build from release

```bash
cargo build --release
# Binary at: target/release/al-symbols-mcp
```

## Usage

### MCP Configuration

Add to your MCP client configuration (e.g., Claude Desktop, Cursor):

```json
{
  "mcpServers": {
    "al-symbols": {
      "command": "al-symbols-mcp",
      "args": []
    }
  }
}
```

The server communicates over **stdio** using the MCP JSON-RPC protocol.

### Environment

Set `RUST_LOG` to control logging verbosity:

```bash
RUST_LOG=debug al-symbols-mcp
```

## MCP Tools

### `al_search_objects`

Search AL objects by name pattern and type with pagination.

| Parameter | Type | Description |
|-----------|------|-------------|
| `pattern` | string? | Search pattern (wildcards: `*`, `?`) |
| `object_type` | string? | Filter: Table, Page, Codeunit, Report, Enum, etc. |
| `limit` | number | Max results (default: 50) |
| `offset` | number | Pagination offset |
| `summary_mode` | bool | Token-efficient summaries |

### `al_get_object_definition`

Get complete object definition including fields, procedures, keys, and properties.

| Parameter | Type | Description |
|-----------|------|-------------|
| `object_type` | string | Object type (required) |
| `id` | number? | Object ID |
| `name` | string? | Object name |

### `al_find_references`

Find all references to an object or field across the entire symbol database.

| Parameter | Type | Description |
|-----------|------|-------------|
| `object_name` | string | Target object name |
| `object_type` | string? | Type filter |
| `field_name` | string? | Field-level references |

### `al_search_object_members`

Unified search for procedures, fields, controls, and data items.

| Parameter | Type | Description |
|-----------|------|-------------|
| `object_name` | string? | Object to search within |
| `object_type` | string? | Type filter |
| `member_type` | string? | procedure, field, control, dataitem, all |
| `pattern` | string? | Member name pattern |
| `limit` | number | Max results (default: 50) |

### `al_get_object_summary`

Token-efficient categorized overview with intelligent procedure grouping.

| Parameter | Type | Description |
|-----------|------|-------------|
| `object_type` | string | Object type |
| `name` | string | Object name |

### `al_get_free_id`

Get the next free object ID(s) for your AL app. Reads `idRanges` from `app.json` and scans **only your app's own `.al` source files** (excluding `.alpackages/`, `.snapshots/`) to find unused IDs within the allowed ranges.

| Parameter | Type | Description |
|-----------|------|-------------|
| `object_type` | string? | Only consider IDs used by this type as occupied |
| `count` | number | How many free IDs to return (default: 1, max: 100) |
| `app_json_path` | string? | Explicit path to `app.json` (auto-discovered if omitted) |

**Example response:**

```json
{
  "freeIds": [70003, 70004, 75000],
  "objectType": "table",
  "idRanges": [
    { "from": 70000, "to": 74999 },
    { "from": 75000, "to": 79999 }
  ],
  "totalCapacity": 10000,
  "usedInRanges": 3,
  "availableInRanges": 9997,
  "usedObjects": [
    { "type": "table", "id": 70000, "name": "My Table", "file": "src/MyTable.al" },
    { "type": "table", "id": 70001, "name": "My Table 2", "file": "src/MyTable2.al" },
    { "type": "table", "id": 70002, "name": "My Table 3", "file": "src/MyTable3.al" }
  ]
}
```

### `al_packages`

Package management operations.

| Parameter | Type | Description |
|-----------|------|-------------|
| `action` | string | `load`, `list`, or `stats` |
| `path` | string? | Directory path (for `load`) |

## How It Works

1. **Discovery**: Scans for `.alpackages` directories and `.app` files
2. **Extraction**: Reads `.app` files (ZIP archives with a 40-byte NAVX header), extracts `NavxManifest.xml` and `SymbolReference.json`
3. **Parsing**: Processes symbol JSON supporting both modern namespaced and legacy flat formats
4. **Indexing**: Builds an in-memory database with multiple indices for fast lookups
5. **Serving**: Exposes indexed data through 7 MCP tools over stdio

## Supported AL Object Types

Tables, Pages, Codeunits, Reports, Queries, XmlPorts, Enums, Interfaces, PermissionSets, Profiles, Entitlements, and all extension types (TableExtension, PageExtension, ReportExtension, EnumExtension, etc.)

## License

MIT
