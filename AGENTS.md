# Mfalmion engineering rules

Mfalmion is an offline-first, local-first chess analysis desktop application for Kali Linux Rolling. Long-term scope includes an interactive board, Stockfish analysis and play, game reviews and library, statistics, Chess.com sync, and local AI coaching.

- Implement one approved milestone at a time. Milestone 01 is the desktop shell only; obtain explicit approval before later milestones.
- Use Rust and Cargo, egui/eframe with wgpu, and clear, testable module boundaries. Future integrations: Stockfish C++ over UCI, llama.cpp local HTTP, rusqlite, and a maintained Rust chess library.
- No JavaScript tooling, browser UI, Node, npm, React, Electron, Tauri, Python frontends, or unapproved Docker.
- Keep the UI responsive; use proper error handling and structured logs. Avoid speculative abstractions and unnecessary dependencies or downloads.
- Keep Stockfish evaluations authoritative and separate from AI-generated explanations. Never fabricate user data or make unverified performance claims.
- Check free disk space before and after compilation. Stop if insufficient. Keep build artifacts in project target/. Avoid release builds, duplicate downloads, cache deletion, cargo clean, and destructive cleanup. Never install system packages without approval.
- Never modify external AI models, external drives, or the CUDA installation. Do not start llama.cpp for shell work.
- Verify each milestone with cargo fmt --all -- --check, cargo check --workspace, cargo test --workspace, and cargo clippy --workspace --all-targets -- -D warnings when feasible. Attempt native launch; report GUI testing limitations honestly.
- Preserve README content and Git history. Keep Cargo.lock tracked. Never commit, push, or change remotes automatically.
