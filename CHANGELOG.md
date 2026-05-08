# Changelog

## v0.3.0

- Add mouse navigation to the sidebar: click to expand/collapse folders or open files, scroll-wheel to scroll independently of the selection, drag the separator column to resize
- Add syntax highlighting for HTML, CSS, Shell, TypeScript/TSX, C#, Java, Go, Kotlin, Groovy, YAML, Properties, and TOML
- Allow scrolling past the cursor: mouse-wheel and Ctrl+Up/Down now move the viewport freely; the cursor stays put and snaps back into view on the next edit or cursor move
- Prompt before launching LSP binaries (trust on first use): SHA-256 hashes are stored in `~/.config/txt/trusted_binaries.json`, and changed binaries re-prompt; mise/asdf shims are canonicalized so toolchain bumps don't re-trigger the prompt
- Bound the undo stack at 1000 entries so long sessions don't grow memory without limit
- Cap search match collection at 10,000 ranges and show truncation in the count (`1/10000+`) to keep pathological queries from stalling the UI
- Add Go To Matching Bracket (Alt+M) — jump the cursor between paired `{}`, `()`, `[]`
- Add Center Viewport On Cursor (Alt+L) — re-center the view without moving the cursor

## v0.2.2

- Fix multi-cursor mode: secondary cursors now render and move correctly
- Fix multi-cursor editing: typing or deleting with multiple cursors no longer produces garbled text

## v0.2.1

- Add markdown syntax highlighting support
- Added customizable keymaps. 3 pre-installed (TXT, VS Code, and IntelliJ IDEA)
- Added kill line action. Copies the line to the paste buffer
- Lots of minor bug fixes and improvments

## v0.1.2

- Bug fixes and improvements

## v0.1.1

- Bug fixes and improvements

## v0.1.0

- Bug fixes and improvements

## v0.0.6

- Initial release

## v0.0.5

- Initial release

## v0.0.4

- Initial release

## v0.0.3

- Initial release

## v0.0.2

- Initial release

## v0.0.1

- Initial release
