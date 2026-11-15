# Milestone 01 initial verification — 2026-11-16

Accepted runtime update: the user has manually verified startup, navigation, resizing and normal shutdown with X11/Intel Vulkan. This is Milestone 01’s accepted runtime configuration. The original Wayland Invalid surface issue remains unresolved.

Historical initial results. The later [runtime stabilization report](runtime-stabilization.md) supersedes the runtime caveats below.

Implemented a single Rust workspace application using cached eframe 0.36.2, default fonts, wgpu, Wayland and X11. No engine, AI, network, database or chess functionality was added. Original README heading and Git history are preserved. No system packages were installed and all Cargo operations used offline cached dependencies.

## Build checks

- cargo fmt --all -- --check: passed.
- cargo check --workspace --offline: passed.
- cargo test --workspace --offline: passed, zero unit tests (UI shell only).
- cargo clippy --workspace --all-targets --offline -- -D warnings: passed.
- git diff --check: passed.

## Runtime

The default Wayland launch panicked within wgpu Surface::configure with Invalid surface. Vulkan reports Intel Iris Xe, NVIDIA RTX 3050 (550.163.01), and Mesa llvmpipe adapters. No drivers or system configuration were modified.

Explicit X11 runs, including Intel-only Vulkan and llvmpipe diagnostic selections, produced visible frames. Dashboard, Game Library and Settings frames were inspected. Settings was visually inspected at 1440×900 and 760×540 with a narrower sidebar and wrapped content. Automated clicking did not reliably advance pages and close requests did not reliably exit; sustained navigation, scrolling and clean shutdown remain unverified. Test processes were terminated explicitly. No claim is made that the default desktop launch is working or that the issue is proven to be a specific GPU driver.

See README for the exact diagnostic launch command. Manual graphical-session verification is required before accepting runtime completion.

## Storage and Git

Free space before compilation: 9.8 GB (df -h). After compilation: approximately 8.6 GB. Project target/: approximately 1.2 GB. Debug symbols and incremental artifacts disabled; build jobs limited to two. No cleanup was performed. No commits, pushes, remote changes, external drive writes or AI/CUDA asset changes.

At the time of these initial results, Milestone 02 was unapproved. Milestone 02A is now explicitly approved; see [its report](milestone-02a.md). Legal moves in 02B remain unapproved.
