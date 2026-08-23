# English (en-US) UI strings for XML Tool. Keys must match zh-CN.ftl exactly.

app-name = XML Tool
menu-file = File
menu-edit = Edit
menu-search = Search
menu-xml = XML
menu-exi = EXI
menu-view = View
menu-help = Help

action-new = New
action-open = Open…
action-recent = Recent Files
action-save = Save
action-save-as = Save As…
action-save-all = Save All
action-reload = Reload
action-close-tab = Close Tab
action-close-others = Close Others
action-exit = Exit
action-undo = Undo
action-redo = Redo
action-find = Find
action-format = Format Document
action-xpath = XPath Query…
action-validate-with = Validate with schema…
action-diff = Compare with file…
xpath-result-nodes = { $count } nodes
action-validate = Validate…
action-about = About
action-exit-cancel = Cancel

panel-outline = Outline
panel-search = Search
panel-inspector = Inspector
panel-problems = Problems
panel-tasks = Tasks
panel-source = Source
panel-diff = Diff
panel-exi = EXI Result
panel-empty = No document open
welcome-title = Welcome to XML Tool
welcome-hint = Open an XML file or create a new document to get started.

tab-untitled = Untitled-{ $number }
tab-dirty = { $name } ●
tab-close = Close tab

toolbar-open-file = Open a file
toolbar-save-file = Save the current file
toolbar-new-file = Create a new document
toolbar-format = Format the document
toolbar-show-outline = Show outline panel
toolbar-show-inspector = Show inspector panel

outline-expand-all = Expand all
outline-collapse-all = Collapse all
outline-duplicate = Duplicate node
outline-delete = Delete node
outline-empty = Nothing to show
outline-copy-xpath = Copy XPath
outline-copy-xml = Copy XML fragment

inspector-qname = Name
inspector-namespace = Namespace URI
inspector-attributes = Attributes
inspector-add-attribute = Add attribute
inspector-remove-attribute = Remove attribute
inspector-text = Text
inspector-cdata = CDATA
inspector-comment = Comment
inspector-pi-target = Target
inspector-pi-data = Data
inspector-no-selection = Select a node in the outline
inspector-read-only = Read-only mode: editing disabled for large documents

problems-empty = No problems
toolbar-undo = Undo
toolbar-redo = Redo
problems-filter-errors = Errors
problems-filter-warnings = Warnings
problems-filter-infos = Info
problems-filtered-empty = All alerts are filtered out
problems-clear = Clear all
problems-at-position = line { $line }, column { $column }
problems-jump = Click to jump to the source position
source-jump-hint = Jump target: line { $line }, column { $column }
problems-count = { $count ->
    [one] 1 problem
   *[other] { $count } problems
}

status-ready = Ready
status-read-only = Read-only
status-edited = Edited
status-clean = Clean
status-position = Line { $line }, Column { $column }
status-encoding = Encoding: { $name }
status-elements = { $count } elements

dialog-delete-title = Delete node?
dialog-delete-body = Delete { $name } and its { $descendants ->
    [one] 1 descendant
   *[other] { $descendants } descendants
}? This can be undone.
dialog-confirm = Delete
dialog-cancel = Cancel
dialog-run = Run

dialog-about-title = About { $name }
dialog-about-version = Version { $version }
dialog-about-backends = XML backend: { $xml } · EXI backend: { $exi }
dialog-about-license = License: { $license }

dialog-unsaved-title = Unsaved changes
dialog-unsaved-body = { $count } tab(s) have unsaved changes.
dialog-unsaved-save-selected = Save Selected
dialog-unsaved-discard-selected = Discard Selected
dialog-unsaved-cancel = Cancel

dialog-reload-title = File changed on disk
dialog-reload-clean-body = { $name } changed on disk. Reload it?
dialog-reload-dirty-body = { $name } changed on disk and has unsaved edits.
dialog-reload-compare = Compare
dialog-reload-reload = Reload
dialog-reload-keep = Keep Mine

dialog-recovery-title = Recover documents?
dialog-recovery-body = { $count } document(s) were recovered after the last session.
dialog-recovery-open = Recover
dialog-recovery-discard = Discard

source-apply = Apply Source
source-discard-draft = Discard Draft
source-draft-active = Draft contains unapplied changes
source-jump-line = Jump to line

exi-open-workbench = Open EXI workbench
exi-dialog-title = EXI Workbench
exi-encode = Encode document
exi-decode = Decode EXI file…
exi-report = {preset} · {input} B → {output} B ({percent}%) · {ms} ms
exi-preset-fidelity = Fidelity Bit-Packed
exi-preset-byte = Byte Aligned
exi-preset-precompression = Pre-Compression
exi-preset-max = Maximum Compression
exi-fidelity-warning = These options drop: {items}

search-placeholder = Search…
search-hits = { $count } hits
search-replace-placeholder = Replace with…
search-replace-all = Replace all
search-no-hits = No matches
search-case-sensitive = Match case

theme-system = System
theme-light = Light
theme-dark = Dark
font-scale = Font size: { $percent }%

view-theme = Theme
view-font-scale = Font Scale
view-language = Language
language-english = English
language-chinese = 简体中文

readonly-reason = Document exceeds the editing thresholds (20 MiB or 200,000 elements); open in read-only mode.

shortcut-help = Keyboard Shortcuts
shortcut-new = New document
shortcut-open = Open document
shortcut-save = Save document
shortcut-save-as = Save as
shortcut-close = Close tab
shortcut-undo = Undo
shortcut-redo = Redo
shortcut-find = Find
shortcut-replace = Replace
shortcut-next-match = Next match
shortcut-prev-match = Previous match
shortcut-cycle-focus = Cycle focus
shortcut-help-key = Shortcut help
shortcut-tree-nav = Tree: arrow keys navigate, Enter/Right expand, Left collapse, Space toggle

error-parse-failed = Failed to parse document
error-io = File error: { $message }
error-too-large = File exceeds the 256 MiB open limit
error-readonly-edit = This document is read-only
