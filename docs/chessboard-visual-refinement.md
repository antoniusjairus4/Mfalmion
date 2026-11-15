# Chessboard visual refinement — 2026-11-16

Native visual acceptance is pending. No native desktop application, OS GUI automation, display/driver configuration command, engine or AI server was launched. Milestone 02C was not started. All changes, including the preceding local 02B work, remain uncommitted and unpushed.

## Changes in this refinement

- `crates/mfalmion-app/src/board/artwork.rs` (new): embeds all twelve original Cburnett SVGs; maps each side/piece kind to its dedicated asset; parses and rasterizes offline using resvg 0.50.0.
- `board/view.rs`: replaces Unicode piece text with image painting for both stationary and dragged pieces. Groups highlight inputs and adds dark outlines around selection and legal-move markers for readability on the lighter palette. Existing interaction events and orientation mapping are retained.
- `board/position.rs`: removes only the obsolete glyph-string method. The shakmaty adapter is unchanged.
- `board/mod.rs`: artwork cache/highlight exports.
- `ui/game_review.rs`: owns the persistent artwork cache and passes the same authoritative position/status/highlights into painting. Rule actions, promotion choices, undo/new-game logic and position history are unchanged.
- `app.rs`: removes obsolete chess-font registration. The surrounding text retains egui's normal typography.
- `theme.rs`: light squares #EEEED2, dark squares #769656; obsolete glyph fill constants removed. The dark application theme and panels are unchanged.
- Cargo manifest/lockfile: adds resvg with default features disabled. No system fonts, SVG text rendering, SVGZ or raster-image decoder features are enabled for these path-only assets.
- README, this report, and `assets/pieces/cburnett/`: artwork, attribution, full license, source notice and checksums.

The previous DejaVu subset and license remain on disk as historical assets; no code references or embeds the font and it is not loaded at runtime. No existing user files or caches were deleted.

## Asset source and obligations

All 12 SVGs are unchanged copies of `lichess-org/lila/public/piece/cburnett` at revision `4a427ef69b93816aac87e0df59b44f38ea2e49fe`. The assets occupy about 9 KB of SVG content; artwork and licensing directory about 84 KB on disk, rounded.

The [pinned asset-specific notice](https://github.com/lichess-org/lila/blob/4a427ef69b93816aac87e0df59b44f38ea2e49fe/COPYING.md) credits **Colin M. L. Burnett** under **GPLv2+**. That is the selected redistribution grant for these exact files. It is not treated as the lila application's general AGPL grant, nor as a CC BY-SA grant inferred from a different repository. The bundled [ATTRIBUTION.md](../assets/pieces/cburnett/ATTRIBUTION.md) records the source, artist, selected grant and redistribution obligations. The full GPLv2 text, source-license row and SHA-256 hashes are alongside the original editable SVGs. Retain these notices, license and source availability when redistributing; apply the selected GPL's copyleft/change-notice requirements to modified/non-source distributions as applicable. No endorsement is implied.

## Rendering/cache behavior

The cache belongs to Game Review and survives navigation and ordinary frames. SVGs are parsed lazily once per piece; decoded trees are reused. Physical texture dimensions are derived from logical piece size times egui pixels-per-point, rounded upward to 16-pixel buckets and bounded at 512 pixels per side. This supports ordinary resizing/high-DPI rendering while bounding the texture budget at 12 MiB of RGBA pixels for twelve maximum-size textures, excluding GPU/allocator overhead.

Each of the twelve entries retains only its latest size/texture handle. Replacing a texture releases the preceding handle rather than retaining every resize variant. Repeated draws, including dragged pieces, reuse the same texture. SVG parse/render failures are logged once per cached failure and show a visible question-mark error indicator; there is no Unicode chess-piece fallback. tiny-skia's premultiplied RGBA is passed to egui with its matching premultiplied constructor to preserve transparent edges. No filesystem/network fetch is required at runtime.

Quiet legal destinations retain mint dots, capture destinations mint rings (including en passant). Dark outlines give these and the cyan selection border contrast against both board colors. Check indication remains red. Rules, castling, en passant, promotion, flip mapping, Undo and New Game are preserved.

## Verification

Passed:

```sh
cargo fmt --all -- --check
cargo check --workspace --offline
cargo test --workspace --offline
cargo clippy --workspace --all-targets --offline -- -D warnings
git diff --check
```

29 tests passed, zero failed. Existing 27 rules/geometry/controller checks remain passing. Two new artwork tests verify that all twelve uniquely mapped assets parse, rasterize with nonempty transparent output at 32 and 160 pixels, and that textures are reused for identical size buckets while each piece retains only one current texture. The cache test also checks physical-density sizing and the resolution cap. These tests are CPU-only and do not open native windows or connect to a GPU/display.

The rules (`board/game.rs`), selection/orientation (`board/state.rs`) and square mapping (`board/square.rs`) were additionally checksum-compared against their contents at the start of this refinement and are byte-for-byte unchanged.

## Resources

Free internal SSD space before compilation: about 8.2 GB. After: about 7.9 GB (rounded df -h). Project target/: about 2.0 GB (rounded du). The build reused existing dependencies and added 15 small SVG-rendering packages; no release build, cache deletion, cargo clean or system package installation was performed. External archives/models and CUDA assets were untouched.

## Manual visual approval

Use the existing accepted X11/Intel Vulkan command:

```sh
env -u WAYLAND_DISPLAY MFALMION_WINDOW_SYSTEM=x11 VK_DRIVER_FILES=/usr/share/vulkan/icd.d/intel_icd.json WGPU_BACKEND=vulkan cargo run -p mfalmion-app --offline
```

- [ ] Confirm classic Cburnett silhouettes for all six white and six black types, correctly placed. Board squares must be cream #EEEED2 / green #769656, with a1 dark.
- [ ] Resize between large and compact windows and change zoom/display scaling if desired: look for crisp contours, transparent edges and no missing assets.
- [ ] Select, move, capture and drag pieces; confirm that the image follows the pointer and illegal moves return to the correct square.
- [ ] Flip the board and repeat selection/movement. Verify readable selection borders, legal dots, capture rings and checked-king indication on both square colors.
- [ ] Recheck castling, en passant, promotion, Undo Last Move and New Game using the 02B acceptance checklist.
- [ ] Confirm the application's surrounding dark theme, sidebar, panels and typography remain familiar; close normally.

Known limits: native visual quality and physical interaction require this manual approval; no native screenshot/GUI verification was performed. At extreme scaling beyond the 512px-per-piece cap, textures may soften; this cap bounds cache memory. The pre-existing Wayland compatibility issue and 02B's conservative dead-position limitation are unchanged.

## Subsequent Git authorization

The user subsequently requested committing and pushing this work to GitHub. This supersedes the original no-commit/no-push instruction; native manual acceptance remains pending.
