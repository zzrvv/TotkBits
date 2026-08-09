# Repository Guidelines

## Project Structure & Module Organization

Never read, write or execute anything outside W:\coding\TotkBits. Use W:\coding\TotkBits\tmp folder for temporary and test files. TotkBits is a Windows-focused Tauri application. The React/Vite frontend lives in `src/`; `main.jsx` is the entry point, UI components use PascalCase filenames, and shared styles are in `App.css` and `styles.css`. Static images belong in `public/`, while release screenshots are kept in `preview/`.

The Rust backend is under `src-tauri/src/`. Tauri commands are defined in `TauriCommands.rs`, application logic in `TotkApp.rs`, and format handlers in `file_format/`. Runtime helper scripts and binary lookup data live in `src-tauri/misc/`. Skip and never read `*.rs` files in `src-tauri/misc/` - those are meant for backup.

## Build, Test, and Development Commands

- `npm install` installs frontend and Tauri CLI dependencies.
- `npm run dev` starts the Vite frontend for UI-only work.
- `npm run tauri dev` runs the complete desktop application.
- `npm run build` creates the frontend production bundle.
- `cargo check --manifest-path src-tauri/Cargo.toml` checks backend compilation quickly.
- `cargo test --manifest-path src-tauri/Cargo.toml` runs Rust tests (the current tree has no dedicated test modules).
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` verifies Rust formatting.
- `python tauri_build.py` performs the Windows release workflow and requires its documented Python/.NET tooling.

## Coding Style & Naming Conventions

Use four spaces in JSX and follow the existing semicolon-based JavaScript style. Name React components and their files in PascalCase (`DirectoryTree.jsx`); use camelCase for functions, props, and state. Let `rustfmt` define Rust layout. Existing Rust modules use PascalCase filenames, so match neighboring modules even though functions and variables remain `snake_case`. Keep format-specific parsing inside `src-tauri/src/file_format/`.

## Testing Guidelines

There is no configured JavaScript test runner or coverage threshold. For UI changes, run the relevant app mode and manually exercise opening, editing, saving, and error states. For Rust changes, add focused `#[cfg(test)]` modules near the implementation when practical, then run `cargo test` and `cargo check`. Never commit generated `target/`, `dist/`, or local configuration artifacts.

## Commit & Pull Request Guidelines

History favors short, imperative summaries such as `fixed vulnerabilities` and `Xlink editing support`. Keep subjects concise but more specific where possible, for example `Fix RSTB save path handling`. Pull requests should explain the user-visible effect, list validation commands, and identify affected file formats. Link relevant issues and include before/after screenshots for UI changes. Call out new runtime dependencies or configuration changes explicitly.
