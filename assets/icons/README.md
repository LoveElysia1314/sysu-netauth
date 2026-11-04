# Shared SYSU NetAuth icons

`source.json` is the sole editable geometry and status-palette source for macOS and Windows. Coordinates use an 18×18, y-up canvas. Numeric path commands are move (0), line (1), cubic curve (2), close (3) and ellipse (4). Keep the shield and centre marks legible at 16–18px; never communicate state through colour alone.

| State | Mark | Accent | Windows resource ID |
|---|---|---|---|
| ready | Ethernet port | blue | 2 |
| authenticating | opposing arrows | orange | 5 |
| authenticated | check | green | 4 |
| failed | cross | red | 6 |
| disconnected / stopped | minus | gray | 3 |
| unavailable | exclamation | gray | 7 (available for future use) |

The application/installer icon uses a white rounded tile with a single graphite shield and Ethernet port (resource ID 1). The `application` style defines the tile inset, corner radius, ink and artwork scale. macOS offers template monochrome and shared accent colours with an adaptive shield outline; Windows uses the brand-blue outline so the same ICO works on light and dark taskbars.

## Generated outputs

- `generated/svg`: portable vector previews and product icon.
- `generated/windows`: 19 ICO containers (application, six colour states, six black and six white states) with independent vector renders at 16, 20, 24, 32, 40, 48, 64, 128 and 256px; PNG RGBA entries supported by Windows 10/11.
- `macos/Sources/NetAuthArtwork/GeneratedIconSource.swift`: embedded source data for AppKit, generated rather than separately hand-maintained. No runtime file paths or external JSON resources are required.

Regenerate from the repository root on macOS (Command Line Tools, no third-party Python packages):

```sh
python3 scripts/generate-icons.py
swift run --package-path macos -c release NetAuthIconPreview --export-windows
python3 scripts/generate-icons.py --record-windows
python3 scripts/generate-icons.py --check
python3 -m unittest discover -s tests -p test_icon_assets.py
```

Commit the source and generated outputs together. The manifest records source/output hashes; CI rejects stale SVG, Swift data or ICO provenance. ICO structure, dimensions, offsets, RGBA type and PNG CRCs are checked without native Windows APIs. Actual Windows rendering/resource compilation remains a Windows CI or device check.

Generate the macOS light/dark/selection gallery with `swift run --package-path macos -c release NetAuthIconPreview dist/menu-bar-icons.png`.

Windows defaults to monochrome and selects black/white variants from the taskbar theme. Colour remains an optional tray-menu preference. macOS Dock/Finder ICNS is generated during `build-macos-gui.sh` from the same application artwork using `--export-macos`; no separate editable bitmap source is maintained.
