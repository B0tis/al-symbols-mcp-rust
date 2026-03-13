# AL Symbols MCP Server (Rust)

A high-performance [Model Context Protocol (MCP)](https://modelcontextprotocol.io/) server for analyzing Business Central AL packages (`.app` files). Built in Rust for maximum performance and minimal resource usage.

This is a Rust reimplementation of [StefanMaron/AL-Dependency-MCP-Server](https://github.com/StefanMaron/AL-Dependency-MCP-Server), delivering the same functionality with significantly better performance characteristics.

## Features

- **Auto-discovery** of `.alpackages` directories and VS Code `al.packageCachePath` settings
- **ZIP extraction** from compiled `.app` files (handles 40-byte NAVX headers and signed packages)
- **AL CLI integration** — automatically falls back to `AL CreateSymbolPackage` for runtime packages that lack `SymbolReference.json` (e.g. the Base Application containing Sales Header, Purchase Header, etc.)
- **Full symbol parsing** of `SymbolReference.json` with namespace support
- **Dependency resolution** with topological sorting and circular dependency detection
- **In-memory indexed database** with O(1) lookups by name, type, and ID
- **8 MCP tools** for comprehensive AL object analysis
- **Free ID lookup** from `app.json` `idRanges` with per-type filtering

## Performance

| Metric | TypeScript | Rust |
|--------|-----------|------|
| Binary size | ~100MB (Node.js) | ~3.8MB |
| Startup time | ~500ms | ~5ms |
| Memory usage | Higher (V8 GC) | Minimal (zero-copy where possible) |
| Package loading | Sequential/limited concurrency | Parallel with Rayon |

## Installation

### Pre-built binaries (easiest)

Download the latest binary for your platform from [GitHub Releases](https://github.com/B0tis/al-symbols-mcp-rust/releases):

| Platform | File |
|----------|------|
| Windows x64 | `al-symbols-mcp-x86_64-pc-windows-msvc.zip` |
| Linux x64 | `al-symbols-mcp-x86_64-unknown-linux-gnu.tar.gz` |
| macOS Apple Silicon | `al-symbols-mcp-aarch64-apple-darwin.tar.gz` |
| macOS Intel | `al-symbols-mcp-x86_64-apple-darwin.tar.gz` |

Extract the binary and place it somewhere on your `PATH` (e.g. `C:\Tools\` on Windows, `/usr/local/bin/` on Linux/macOS).

### From source (requires Rust)

```bash
cargo install --git https://github.com/B0tis/al-symbols-mcp-rust
```

> **Windows note:** If you see `linker 'link.exe' not found`, either install
> [Build Tools for Visual Studio](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
> with the **"Desktop development with C++"** workload, or use the GNU toolchain instead:
>
> ```
> rustup target add x86_64-pc-windows-gnu
> cargo install --git https://github.com/B0tis/al-symbols-mcp-rust --target x86_64-pc-windows-gnu
> ```

## Usage with Cursor / Windsurf / Claude Desktop

### Cursor

Create `.cursor/mcp.json` in your project root (or open **Settings > MCP > Add new MCP server**):

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

If the binary is not on your PATH, use the full path:

```json
{
  "mcpServers": {
    "al-symbols": {
      "command": "C:\\Tools\\al-symbols-mcp.exe",
      "args": []
    }
  }
}
```

### Windsurf

Edit `~/.codeium/windsurf/mcp_config.json` (or open **Settings > search MCP > Edit in mcp_config.json**):

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

### Claude Desktop

Edit `claude_desktop_config.json` (see [MCP docs](https://modelcontextprotocol.io/quickstart/user)):

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

The server communicates over **stdio** which all MCP clients support natively. Open your AL project folder before using the tools so the server can auto-discover `.alpackages` and `app.json`.

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

Get the next free object ID(s) for your AL app. Reads `idRanges` from `app.json` and scans **only your app's own `.al` source files** (excluding `.alpackages/`, `.snapshots/`). When no `object_type` is specified, returns the next free ID **for every object type** so the agent knows exactly which ID to use.

| Parameter | Type | Description |
|-----------|------|-------------|
| `object_type` | string? | Filter to a single type (table, page, codeunit, etc.) |
| `count` | number | How many free IDs to return per type (default: 1, max: 100) |
| `app_json_path` | string? | Explicit path to `app.json` (auto-discovered if omitted) |

**Example response** (no type filter -- shows all types):

```json
{
  "perObjectType": [
    { "objectType": "table",          "nextFreeId": 70002, "freeIds": [70002], "usedCount": 2 },
    { "objectType": "tableextension", "nextFreeId": 70000, "freeIds": [70000], "usedCount": 0 },
    { "objectType": "page",           "nextFreeId": 70001, "freeIds": [70001], "usedCount": 1 },
    { "objectType": "pageextension",  "nextFreeId": 70000, "freeIds": [70000], "usedCount": 0 },
    { "objectType": "codeunit",       "nextFreeId": 70002, "freeIds": [70002], "usedCount": 2 },
    { "objectType": "enum",           "nextFreeId": 70001, "freeIds": [70001], "usedCount": 1 }
  ],
  "idRanges": [ { "from": 70000, "to": 74999 } ],
  "totalCapacity": 5000,
  "totalUsed": 6,
  "usedObjects": [
    { "objectType": "table", "id": 70000, "name": "My Table", "file": "src/MyTable.al" },
    { "objectType": "table", "id": 70001, "name": "My Table 2", "file": "src/MyTable2.al" }
  ]
}
```

### `al_packages`

Package management operations.

| Parameter | Type | Description |
|-----------|------|-------------|
| `action` | string | `load`, `list`, or `stats` |
| `path` | string? | Directory path (for `load`) |

### `al_cli_status`

Check AL CLI availability or attempt auto-installation. The AL CLI is required for loading runtime packages (like the Base Application) that don't embed `SymbolReference.json`.

| Parameter | Type | Description |
|-----------|------|-------------|
| `action` | string | `status` (check availability) or `install` (auto-install via dotnet) |

## AL CLI Integration

Some `.app` packages — particularly the Microsoft Base Application which contains core tables like **Sales Header** (Table 36), **Sales Line**, **Purchase Header**, **G/L Entry**, etc. — are distributed as **runtime packages** that don't include `SymbolReference.json` inside the ZIP archive. These packages require the AL CLI tool to convert them into symbol packages.

The server automatically:
1. Tries direct ZIP extraction first (fast, no dependencies)
2. Falls back to `AL CreateSymbolPackage` when direct extraction fails
3. Logs a helpful message with installation instructions when AL CLI is needed but not found

### Installing the AL CLI

```bash
# Windows
dotnet tool install --global Microsoft.Dynamics.BusinessCentral.Development.Tools --prerelease

# Linux
dotnet tool install --global Microsoft.Dynamics.BusinessCentral.Development.Tools.Linux --prerelease

# macOS
dotnet tool install --global Microsoft.Dynamics.BusinessCentral.Development.Tools.Osx --prerelease
```

Or use the `al_cli_status` MCP tool with `action: "install"` to auto-install.

You can also set the `AL_CLI_PATH` environment variable to point to a custom AL binary location.

## How It Works

1. **Discovery**: Scans for `.alpackages` directories and `.app` files
2. **Extraction**: Reads `.app` files (ZIP archives with a 40-byte NAVX header), extracts `NavxManifest.xml` and `SymbolReference.json`
3. **AL CLI fallback**: For runtime packages without `SymbolReference.json`, uses `AL CreateSymbolPackage` to generate symbol references
4. **Parsing**: Processes symbol JSON supporting both modern namespaced and legacy flat formats
5. **Indexing**: Builds an in-memory database with multiple indices for fast lookups
6. **Serving**: Exposes indexed data through 8 MCP tools over stdio

## Supported AL Object Types

Tables, Pages, Codeunits, Reports, Queries, XmlPorts, Enums, Interfaces, PermissionSets, Profiles, Entitlements, and all extension types (TableExtension, PageExtension, ReportExtension, EnumExtension, etc.)

## License

MIT
