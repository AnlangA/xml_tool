# XML Tool

A modern, high-performance XML/EXI viewer and editor built with Rust and egui.

## Features

- **XML Viewing & Editing**: Parse, view, and edit XML documents with a intuitive tree interface
- **EXI Support**: Efficient XML Interchange format compression and decompression
- **JSON Export**: Export XML documents to JSON format
- **Search**: Fast search with result caching
- **Undo/Redo**: Full document history support
- **Theme Support**: Beautiful Catppuccin theme integration
- **High Performance**: Optimized with LRU caching and efficient parsing

## Installation

### Prerequisites

- Rust 1.88 or later
- Cargo

### Build

```bash
git clone https://github.com/AnlangA/xml_tool.git
cd xml_tool
cargo build --release
```

## Usage

Run the application:

```bash
cargo run --release
```

### Keyboard Shortcuts

- `Ctrl+O` - Open XML file
- `Ctrl+S` - Save file
- `Ctrl+Z` - Undo
- `Ctrl+Y` / `Ctrl+Shift+Z` - Redo
- `Ctrl+F` - Search

## License

MIT License - see [LICENSE](LICENSE) for details.
