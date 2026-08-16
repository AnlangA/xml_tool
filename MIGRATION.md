# Migration guide 0.2 → 0.3

## Library API

The `xml_tool` crate restructures its API surface. The legacy facade stays
available; new code should move to the engine layer.

| 0.2 API | 0.3 status | Replacement |
|---|---|---|
| `xml::parse_xml(&str)` | kept (facade) | `core::document::XmlDocument::parse(&[u8])` |
| `xml::parse_xml_file(&Path)` | kept (facade) | read bytes → `XmlDocument::parse` |
| `xml::serialize_xml(&XmlDocument)` | kept (facade) | `services::document_io::document_bytes` |
| `exi::encode_xml_to_exi` | kept | `services::exi_workbench::encode_with_settings` |
| `exi::decode_exi_to_xml` | kept | `services::exi_workbench::decode_with_report` |

New public surfaces:

- `core`: `XmlDocument` (private fields, accessor/`Command` mutations),
  `Command`, `History`, `Diagnostic`, `NodeId`, `Revision`.
- `services`: task manager, workspace sessions, atomic I/O, recovery,
  file watching, search index, outline flattening, XPath, XSD validation,
  structural diff, batch replace, EXI workbench, session caches.
- `ui`: `AppShell` replaces `MainPanel` (removed).

## Behavioral changes

- Files open as **bytes**: UTF-16 documents load and save correctly;
  unsupported declared encodings now error instead of failing later.
- Duplicate/malformed attributes are **errors**, not warnings-and-skip.
- Internal DTD entities expand (0.2 rejected them); external entities are
  still never loaded.
- Documents over 20 MiB or 200,000 elements open **read-only**; over
  256 MiB refused. 0.2 attempted to edit anything.
- Undo is command-based (bounded memory); 0.2 stored full snapshots.
- JSON export defaults unchanged (`export_to_json`); a lossless mode is
  available via `export_to_json_lossless`.

## Data compatibility

- XML files: fully compatible; unedited saves are byte-identical.
- EXI streams produced by 0.2 decode unchanged.
- User settings and recent files from 0.2 have no persistent store in
  0.3.0 (session-scoped); a settings file lands with the first packaged
  release.

## Licensing

erxi (EXI backend) is PolyForm-Noncommercial-1.0.0. Distributing xml_tool
with EXI enabled is restricted to non-commercial use; commercial builds
must replace or remove the EXI backend. Everything else is MIT.
