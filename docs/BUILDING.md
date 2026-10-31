# Building Tingskriver from Source

This guide explains source builds on each supported platform. Start with the build notes below, then use the platform instructions that match your machine.

## Build Notes

Install the committed frontend dependencies before building:

```bash
npm install -g pnpm@9.15.9
cd frontend
pnpm install --frozen-lockfile
```

Frozen installation keeps the lockfile and installed dependency set aligned. When intentionally changing dependencies, update and commit `pnpm-lock.yaml`.

- **Linux:** Tingskriver is built from source; choose acceleration for the environment where you build.
- **Windows packages:** Distribution builds use Vulkan-enabled Whisper and require an AVX2-capable x64 CPU. AVX-512 is not required.
- **CUDA:** NVIDIA CUDA support requires a compatible source build and CUDA toolchain; the standard Windows installer does not select it automatically.

## Workspace, tests and CI for agents

The repository is a Cargo workspace (see [arkitektur.md](arkitektur.md)): `crates/tingskriver-core` holds the logic, `crates/tingskriver-app` the GPUI app, `crates/tingskriver-overlay` the Windows overlay, `crates/tingskriver-host` the hook, shortcut and tray host and `frontend/src-tauri` the current Tauri app.

```bash
cargo test -p tingskriver-core          # platform independent tests, runs on Linux
cargo test -p tingskriver-core dictation
cargo test --workspace                  # everything; the Tauri crate needs the Linux Tauri libraries
```

Dictation insertion has a test against a real window and the real clipboard. It only runs on a Windows desktop session and on request, since it types into a test window (do not touch the keyboard while it runs):

```powershell
cargo test -p tingskriver-core --lib insertion_window -- --ignored --test-threads=1
```

It inserts 50 dictations in a row with a 24 MB image and text on the clipboard and checks the text and the clipboard after each one, and checks that a field which ignores paste gets the text typed.

The Tauri crate's build script checks that its sidecars exist. For a local `cargo check`/`cargo test` outside a full build, create an empty placeholder: `touch frontend/src-tauri/binaries/llama-helper-$(rustc -vV | sed -n 's/host: //p')`.

### Checking Windows code from Linux (cargo xwin)

Most new Rust code is Windows-only (`#[cfg(windows)]`), and Linux `cargo check` skips it. [cargo-xwin](https://github.com/rust-cross/cargo-xwin) type-checks it against the MSVC target with the Windows SDK it downloads on first use (accepting Microsoft's licence):

```bash
rustup target add x86_64-pc-windows-msvc
cargo install cargo-xwin --locked
sudo apt-get install clang llvm lld ninja-build cmake   # Debian: llvm-19 lld-19, then add /usr/lib/llvm-19/bin to PATH
cargo xwin check --target x86_64-pc-windows-msvc -p tingskriver-core -p tingskriver-overlay -p tingskriver-host -p tingskriver-app
```

whisper.cpp builds fine this way. The Tauri crate itself does not: its build script verifies the downloaded `ffmpeg.exe` by running it. For a local check only, a stub that answers `-version` gets past that:

```bash
printf '#!/bin/sh\necho "ffmpeg version stub"\n' > frontend/src-tauri/binaries/ffmpeg-x86_64-pc-windows-msvc.exe
chmod +x frontend/src-tauri/binaries/ffmpeg-x86_64-pc-windows-msvc.exe
touch frontend/src-tauri/binaries/llama-helper-x86_64-pc-windows-msvc.exe
cargo xwin check --target x86_64-pc-windows-msvc -p tingskriver
```

Delete the stub before building an installer on that machine. CI runs the same xwin check (without the Tauri crate) in `.github/workflows/rust.yml`.

### Dependency licences (cargo deny)

`deny.toml` lists the licences a Cargo dependency may have. GPL, LGPL and AGPL crates are rejected, because they would end up in the MIT-licensed binary. CI runs the check in `.github/workflows/rust.yml`; run it locally with:

```bash
cargo install cargo-deny --version 0.20.2 --locked
cargo deny --locked check licenses bans
```

Bundled programs and downloaded models are not Cargo dependencies; their licences are listed in [THIRD_PARTY_LICENSES.md](../THIRD_PARTY_LICENSES.md).

### Reading CI results without a token

`scripts/ci-status.sh <branch> [sha]` waits for the GitHub Actions runs of the newest commit on a branch (or the given commit, short SHA allowed) through the public API, then prints each workflow's result. When something failed it lists the failed jobs and steps and the job annotations; cargo errors and test panics show up there because the workflow steps run through `.github/scripts/annotate-errors.sh` (job logs need admin rights, annotations do not). Exit code 0 means green, 1 a failure, 2 a timeout or API problem. Without a token GitHub allows 60 requests an hour; the script polls every 60 s with ETags, and answers that did not change do not count. `GITHUB_TOKEN` raises the limit, `CI_POLL_SECONDS` and `CI_TIMEOUT_MINUTES` tune the wait.

```bash
git push && scripts/ci-status.sh "$(git branch --show-current)"
```

<details>
<summary>Linux</summary>

## 🐧 Building on Linux

This guide helps you build Tingskriver on Linux with **automatic GPU acceleration**. The build system detects your hardware and configures the best performance automatically.

---

### 🚀 Quick Start (Recommended for Beginners)

If you're new to building on Linux, start here. These simple commands work for most users:

#### 1. Install Basic Dependencies

```bash
# Ubuntu/Debian
sudo apt update
sudo apt install build-essential cmake git

# Fedora/RHEL
sudo dnf install gcc-c++ cmake git

# Arch Linux
sudo pacman -S base-devel cmake git
```

#### 2. Build and Run

```bash
# Development mode (with hot reload)
./dev-gpu.sh

# Production build
./build-gpu.sh
```

**That's it!** The scripts automatically detect your GPU and configure acceleration.

### What Happens Automatically?

- ✅ **NVIDIA GPU** → CUDA acceleration (if toolkit installed)
- ✅ **AMD GPU** → ROCm acceleration (if ROCm installed)
- ✅ **No GPU** → Optimized CPU mode (still works great!)

> 💡 **Tip:** If you have an NVIDIA or AMD GPU but want better performance, jump to the [GPU Setup](#-gpu-setup-guides-intermediate) section below.

---

### 🧠 Understanding Auto-Detection

The build scripts (`dev-gpu.sh` and `build-gpu.sh`) orchestrate the entire build process. Here's how they work:

1.  **Detect location:** Find `package.json` (works from project root or `frontend/`)
2.  **Auto-detect GPU:** Run `scripts/auto-detect-gpu.js` (or use `TAURI_GPU_FEATURE` if set)
3.  **Build Sidecar:** Build `llama-helper` with the detected feature (debug or release)
4.  **Copy Binary:** Copy the built sidecar to `src-tauri/binaries` with the target triple
5.  **Run Tauri:** Call `npm run tauri:dev` or `tauri:build` with the feature flag passed via env var

#### Detection Priority

| Priority | Hardware        | What It Checks                                               | Result                  |
| -------- | --------------- | ------------------------------------------------------------ | ----------------------- |
| 1️⃣       | **NVIDIA CUDA** | `nvidia-smi` exists + (`CUDA_PATH` or `nvcc` found)          | `--features cuda`       |
| 2️⃣       | **AMD ROCm**    | `rocm-smi` exists + (`ROCM_PATH` or `hipcc` found)           | `--features hipblas`    |
| 3️⃣       | **Vulkan**      | `vulkaninfo` exists + `VULKAN_SDK` + `BLAS_INCLUDE_DIRS` set | `--features vulkan`     |
| 4️⃣       | **OpenBLAS**    | `BLAS_INCLUDE_DIRS` set                                      | `--features openblas`   |
| 5️⃣       | **CPU-only**    | None of the above                                            | (no features, pure CPU) |

#### Common Scenarios

| Your System               | Auto-Detection Result       | Why                          |
| ------------------------- | --------------------------- | ---------------------------- |
| Clean Linux install       | CPU-only                    | No GPU SDK detected          |
| NVIDIA GPU + drivers only | CPU-only                    | CUDA toolkit not installed   |
| NVIDIA GPU + CUDA toolkit | **CUDA acceleration** ✅    | Full detection successful    |
| AMD GPU + ROCm            | **HIPBlas acceleration** ✅ | Full detection successful    |
| Vulkan drivers only       | CPU-only                    | Vulkan SDK + env vars needed |
| Vulkan SDK configured     | **Vulkan acceleration** ✅  | All requirements met         |

> 💡 **Key Insight:** Having GPU drivers alone isn't enough. You need the **development SDK** (CUDA toolkit, ROCm, or Vulkan SDK) for acceleration.

---

### 🔧 GPU Setup Guides (Intermediate)

Want better performance? Follow these guides to enable GPU acceleration.

#### 🟢 NVIDIA CUDA Setup

**Prerequisites:** NVIDIA GPU with compute capability 5.0+ (check: `nvidia-smi --query-gpu=compute_cap --format=csv`)

##### Step 1: Install CUDA Toolkit

```bash
# Ubuntu/Debian (CUDA 12.x)
sudo apt install nvidia-driver-550 nvidia-cuda-toolkit

# Verify installation
nvidia-smi          # Shows GPU info
nvcc --version      # Shows CUDA version
```

##### Step 2: Build with CUDA

```bash
# Set your GPU's compute capability
# Example: RTX 3080 = 8.6 → use "86"
# Example: GTX 1080 = 6.1 → use "61"

CMAKE_CUDA_ARCHITECTURES=75 \
CMAKE_CUDA_STANDARD=17 \
CMAKE_POSITION_INDEPENDENT_CODE=ON \
./build-gpu.sh
```

> 💡 **Finding Your Compute Capability:**
>
> ```bash
> nvidia-smi --query-gpu=compute_cap --format=csv
> ```
>
> Convert `7.5` → `75`, `8.6` → `86`, etc.

**Why these flags?**

- `CMAKE_CUDA_ARCHITECTURES`: Optimizes for your specific GPU
- `CMAKE_CUDA_STANDARD=17`: Ensures C++17 compatibility
- `CMAKE_POSITION_INDEPENDENT_CODE=ON`: Fixes linking issues on modern systems

---

#### 🔵 Vulkan Setup (Cross-Platform Fallback)

Vulkan works on NVIDIA, AMD, and Intel GPUs. Good choice if CUDA/ROCm don't work.

##### Step 1: Install Vulkan SDK and BLAS

```bash
# Ubuntu/Debian
sudo apt install vulkan-sdk libopenblas-dev

# Fedora
sudo dnf install vulkan-devel openblas-devel

# Arch Linux
sudo pacman -S vulkan-devel openblas
```

##### Step 2: Configure Environment

```bash
# Add to ~/.bashrc or ~/.zshrc
export VULKAN_SDK=/usr
export BLAS_INCLUDE_DIRS=/usr/include/x86_64-linux-gnu

# Apply changes
source ~/.bashrc
```

##### Step 3: Build

```bash
./build-gpu.sh
```

The script will automatically detect Vulkan and build with `--features vulkan`.

---

#### 🔴 AMD ROCm Setup (AMD GPUs Only)

**Prerequisites:** AMD GPU with ROCm support (RX 5000+, Radeon VII, etc.)

```bash
# Ubuntu/Debian
# Add ROCm repository (see https://rocm.docs.amd.com for latest)
sudo apt install rocm-smi hipcc

# Set environment
export ROCM_PATH=/opt/rocm

# Verify
rocm-smi            # Shows GPU info
hipcc --version     # Shows ROCm version

# Build
./build-gpu.sh
```

---

### 🎯 Advanced Usage

#### Manual Feature Override

Want to force a specific acceleration method? Use the `TAURI_GPU_FEATURE` environment variable with the shell scripts:

```bash
# Force CUDA (ignore auto-detection)
TAURI_GPU_FEATURE=cuda ./dev-gpu.sh
TAURI_GPU_FEATURE=cuda ./build-gpu.sh

# Force Vulkan
TAURI_GPU_FEATURE=vulkan ./dev-gpu.sh
TAURI_GPU_FEATURE=vulkan ./build-gpu.sh

# Force ROCm (HIPBlas)
TAURI_GPU_FEATURE=hipblas ./dev-gpu.sh
TAURI_GPU_FEATURE=hipblas ./build-gpu.sh

# Force CPU-only (for testing)
TAURI_GPU_FEATURE="" ./dev-gpu.sh
TAURI_GPU_FEATURE="" ./build-gpu.sh

# Force OpenBLAS (CPU-optimized)
TAURI_GPU_FEATURE=openblas ./dev-gpu.sh
TAURI_GPU_FEATURE=openblas ./build-gpu.sh
```

#### Build Output Location

After successful build:

```
src-tauri/target/release/bundle/appimage/Tingskriver_<version>_amd64.AppImage
```

---

### 🧭 Troubleshooting

#### "CUDA toolkit not found"

- **Fix:** Install `nvidia-cuda-toolkit` or set `CUDA_PATH` environment variable
- **Check:** `nvcc --version` should work

#### "Vulkan detected but missing dependencies"

- **Fix:** Set both `VULKAN_SDK` and `BLAS_INCLUDE_DIRS` environment variables
- **Example:**
  ```bash
  export VULKAN_SDK=/usr
  export BLAS_INCLUDE_DIRS=/usr/include/x86_64-linux-gnu
  ```

#### "AppImage build stripping symbols"

- **Fix:** Already handled! `build-gpu.sh` sets `NO_STRIP=true` automatically
- **Why:** Prevents runtime errors from missing symbols

#### Build works but no GPU acceleration

- **Check detection:** Look at the build output for GPU detection messages
- **Verify:** `nvidia-smi` (NVIDIA) or `rocm-smi` (AMD) should work
- **Missing SDK:** Install the development toolkit, not just drivers

</details>

<details>
<summary>macOS</summary>

## 🍎 Building on macOS

On macOS, the build process is simplified as GPU acceleration (Metal) is enabled by default.

### 1. Install Dependencies

```bash
# Install Homebrew (if not already installed)
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"

# Install required tools
brew install cmake node pnpm
```

### 2. Build and Run

```bash
# Development mode (with hot reload)
pnpm tauri:dev

# Production build
pnpm tauri:build
```

The application will be built with Metal GPU acceleration automatically.

</details>

<details>
<summary>Windows</summary>

## 🪟 Building on Windows

### 1. Install Dependencies

- **Node.js:** Download and install from [nodejs.org](https://nodejs.org/).
- **Rust:** Install from [rust-lang.org](https://www.rust-lang.org/tools/install).
- **pnpm:** Install version 9.15.9 with `npm install -g pnpm@9.15.9`.
- **Visual Studio Build Tools:** Install the "Desktop development with C++" workload from the Visual Studio Installer.
- **CMake:** Download and install from [cmake.org](https://cmake.org/download/).

### 2. Build and Run

```powershell
pnpm install --frozen-lockfile
```

```powershell
# Development mode (with hot reload)
pnpm tauri:dev

# Production build
pnpm tauri:build
```

By default, the application will be built with CPU-only processing. To enable GPU acceleration, see the [GPU Acceleration Guide](GPU_ACCELERATION.md).

### Windows Distribution Builds

The commands above create a local source build. Use the production Windows build workflow for an installer intended for other computers: it enables Vulkan. Rust targets `x86-64-v2`; native Whisper retains AVX2 with host-native specialization and AVX-512 disabled.

The distribution workflows (`build.yml`, `build-windows.yml`, and `build-devtest.yml`) set `CMAKE_PROJECT_INCLUDE` to `.github/force-portable-ggml.cmake`, which forces `GGML_NATIVE=OFF`, and use `RUSTFLAGS=-C target-cpu=x86-64-v2`. The hook configures Whisper's native C/C++ build; Rust flags do not. `WHISPER_NATIVE=OFF` and plain `GGML_*` variables are not replacements.

</details>
