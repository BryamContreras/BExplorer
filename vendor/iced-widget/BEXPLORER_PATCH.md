# BExplorer Iced widget patch

This is `iced_widget` 0.14.2 with one focused scrollbar extension:

- `Scrollbar::scroller_min_length` makes the minimum draggable thumb length
  configurable instead of fixing it at 2 px.
- Rich-text span decorations use the same paragraph anchor as the rendered
  text, keeping search highlights aligned in centered, multi-line icon labels.

BExplorer uses this to keep scrollbars compact at rest and to make the thumb a
little longer and wider while it is revealed or hovered. The default remains
2 px, so upstream behavior is unchanged for callers that do not opt in.
