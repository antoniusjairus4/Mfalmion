# Milestone 01 runtime stabilization — 2026-11-16

## Findings and root cause

Session: Kali Linux, GNOME, XDG_SESSION_TYPE=wayland, WAYLAND_DISPLAY=wayland-0, DISPLAY=:0. Vulkan exposes Intel Iris Xe (Mesa 26.1.6), NVIDIA RTX 3050 Laptop (550.163.01), and Mesa llvmpipe.

Focused diagnostic logging and a backtrace reproduced the original failure. eframe 0.36.2 requests HighPerformance by default; wgpu selected NVIDIA Vulkan and failed at Surface::configure before application creation. Keeping Wayland but requesting low-power Vulkan selected Intel and initialized the app without Invalid surface. This isolates the practical startup failure to the NVIDIA Vulkan/Wayland presentation path on this installed stack. The exact defect inside the proprietary driver/compositor combination is not established. This is not evidence that every Wayland adapter is broken.

The earlier apparent navigation freeze also included an automation problem: xdotool windowactivate did not give the client keyboard focus under this GNOME Wayland session. Explicit windowfocus followed by Tab/Return navigated all six pages; screenshots and application navigation logs agreed. Pointer injection and compositor shortcuts remained unreliable and are not proof of a UI defect.

## Minimal compatibility policy

- Keep Rust, egui/eframe and wgpu. Default the graphics API to Vulkan and adapter preference to LowPower, with existing WGPU_BACKEND/WGPU_POWER_PREF overrides honored.
- Prefer the X11 client backend when DISPLAY exists. In the current session this is XWayland, **not a tested native Xorg session**. It is the interaction path verified here, not proof that all X11 configurations outperform Wayland.
- Allow MFALMION_WINDOW_SYSTEM=x11, wayland, or auto. With no DISPLAY, leave native window-system selection to winit. Invalid values return a configuration error.
- Select the event-loop backend through winit's supported builder hook. Do not mutate process environment, force a machine-specific ICD path, retry silently, or change graphics frameworks.
- Log session policy, actual adapter, page changes and normal shutdown to stderr. MFALMION_DIAGNOSTICS=1 enables focused graphics debug logs using the already cached log crate. No log files are written by the application.
- The winit and log crates were already transitive dependencies; adding direct dependencies required no new downloads or full dependency rebuild.

## Runtime evidence

1. Original Wayland/high-performance path: failed, NVIDIA Vulkan, Invalid surface at Surface::configure.
2. Wayland/low-power Vulkan: initialized Intel, no surface panic during the bounded diagnostic run; native Wayland interaction/close not verified.
3. Updated normal launch, no graphics environment overrides: selected X11 and Intel Vulkan, rendered successfully.
4. Keyboard interaction: Dashboard → Game Review → Game Library → Play vs Stockfish → AI Coach → Settings → Dashboard verified in screenshots and navigation logs.
5. Resize: inspected rendered 760×540 and 1440×900 frames. Compact sidebar and text wrapping worked; resizing back restored the full layout.
6. Normal close: sent the window's advertised WM_DELETE_WINDOW protocol, observed event=shutdown and process exit status 0. This exercises the normal native close-request handler, not a process kill. Physical title-bar click remains a manual check.
7. No test application processes were left running. Qwen/llama.cpp was not signaled, queried, restarted, or reconfigured.

## Checks and storage

Formatting, offline workspace check, offline workspace tests (one compatibility-policy test), and Clippy with warnings denied passed. Git whitespace review passed. Existing artifacts were reused. Before: approximately 8.6 GB free, target about 1.2 GB. After: approximately 8.5 GB free, target about 1.4 GB (rounded df/du). No caches were removed and no system packages installed.

## Manual acceptance

From the repository in a graphical terminal:

```sh
cargo run -p mfalmion-app --offline
```

1. Confirm startup logs show window_system=X11 and Intel Vulkan on this laptop.
2. Click each of the six sidebar entries. Confirm heading and cyan selection change together; page-change logs should match. Confirm controls still respond after repeated switches.
3. Resize between 1440×900 and 760×540. On Settings, scroll to Appearance; check there is no clipped inaccessible content. Test desktop scaling/zoom if needed.
4. Close with the title-bar X. Confirm event=shutdown, prompt returns, and `echo $?` is 0. Relaunch to confirm startup still works.
5. Optional native Wayland comparison: `MFALMION_WINDOW_SYSTEM=wayland cargo run -p mfalmion-app --offline`, and repeat these checks. Do not force high-performance NVIDIA as a workaround.

Remaining limits: physical mouse interaction, Settings scrolling and physical title-bar closing need user verification because pointer/compositor automation was unreliable. Native Xorg was not tested. The original NVIDIA Wayland path remains incompatible; no system driver fix was attempted.

No commits, pushes, system-wide graphics changes, AI asset changes, or Milestone 02 work were performed.
