# XML Tool

A modern, high-performance XML/EXI viewer and editor built with Rust and egui.

## Features

- **XML Viewing & Editing**: Parse, view, and edit XML documents with a intuitive tree interface
- **Encoded Image Preview**: Display PNG, JPEG, GIF, WebP, BMP, and ICO images stored as Base64, image data URIs, or hexadecimal XML data
- **Icon Converter**: Convert image files to Base64, data URIs, or ESI icon hexadecimal text; copy, save, or fill a selected XML element's text draft
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

Select a leaf element whose text contains an encoded image to see its preview in the Details
panel. Plain Base64, `data:image/...;base64,...`, and hexadecimal image bytes (such as EtherCAT
`ImageData16x14`) are supported, including values split across multiple XML lines.

### Convert and import icons

Open **Tools → Icon Converter…** to convert an image without opening an XML document.
Choose an image file or drop one file into the converter window. Base64 is the default;
Data URI includes the MIME type detected from the file contents. Both preserve the complete
original file, including animation frames and ICO entries. The preview shows the decoder's
default frame or icon. Expand **Encoded text** to inspect a sample, or use **Copy** and
**Save Text…** for the complete output.

To import into XML, select an existing leaf element and click **Import Icon…** beside
**Text Content**. **Fill Text Draft** replaces only the text draft, preserving name and
attribute drafts. Review the preview in Details, then **Apply Changes** or **Reset**.
Applied imports support Undo/Redo and normal XML saving. Recompress edited documents
before saving EXI. Changing the selected node or document clears the converter's import
target; reopen **Import Icon…** on the intended element to bind it again.

Selecting `ImageData16x14` automatically chooses **ESI icon (hex)**. This preset converts
the source image into a 16×14, 16-color indexed BMP (4bpp), then encodes it as uppercase
hexadecimal. It preserves the aspect ratio, centers the image with transparent padding,
and reduces the palette only when needed. Magenta (`#FF00FF`) and alpha below 50% become
transparent. The preview shows the actual output. Animated images and ICO files use the
decoder's default frame/icon for ESI conversion. Already compliant 16×14, 4bpp BMP files
keep their original bytes. XML nodes are not created automatically.

Inputs are limited to 8 MiB, 4096 pixels per side, and 8,388,608 total pixels, with a
64 MiB decoder allocation limit. SVG, clipboard image input, and batch conversion are
not supported. A locally installed CJK font is used when available to display Chinese
file names and XML text.

### Keyboard Shortcuts

- `Ctrl+O` - Open XML file
- `Ctrl+S` - Save file
- `Ctrl+Z` - Undo
- `Ctrl+Y` / `Ctrl+Shift+Z` - Redo
- `Ctrl+F` - Search

## License

MIT License - see [LICENSE](LICENSE) for details.
