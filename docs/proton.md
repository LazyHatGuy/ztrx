# Running victory on Linux under Proton

The Linux host runs the tools, the tests and the analysis commands natively.
Actually *running the title* needs a Windows process environment with real
D3D12 — which Proton provides: Wine's loader plus dxvk/vkd3d-proton giving
the GPU stack. So the supported Linux path is: **build `victory.exe` here,
run it inside a Proton prefix.** The title then loads inside that process
exactly as it does on Windows.

Everything below is the full command list for an Arch box; adapt package
names for your distro.

## 1. Build the Windows binary on Linux

```bash
# toolchains
sudo pacman -S --needed rustup mingw-w64-gcc mingw-w64-winpthreads

# the toolchain pinned in rust-toolchain.toml, plus the Windows targets
rustup toolchain install 1.98.1
rustup target add x86_64-pc-windows-gnu --toolchain 1.98.1   # builds and links victory.exe

# build
cargo win        # alias: build -p victory --target x86_64-pc-windows-gnu --release
                 # lands at target/x86_64-pc-windows-gnu/release/victory.exe
```

`cargo win-check` (`check --workspace --all-targets --target
x86_64-pc-windows-gnu`) type-checks the whole Windows tree — tests included
— on the Linux box. A true MSVC-target check needs `clang`/`lld` (or a
Windows box), so the Windows CI job remains that check.

## 2. Run it under Proton

Install a Proton build under Steam's compatibility tools (GE-Proton works
well here), then:

```bash
proton/run.sh config                       # smoke test: prints resolved paths
proton/run.sh --z /mnt/games inspect "Z:\\games\\scripts\\x64\\final.js.c.c.gmpc"
proton/run.sh --z /mnt/games run "Z:\\games\\x64\\final.exe" --execute
```

`proton/run.sh` picks the newest `GE-Proton*` first, then the newest plain
`Proton-*`, keeps its prefix at `$XDG_DATA_HOME/victory/proton`
(idempotent), and takes `--prefix`, `--exe`, `--proton` (a specific Proton
dir or plain `wine`), `--z`, `--config`, `--d3dcompiler`, `--texture-wait`
and `--texture-no-wait`.

The first run creates the prefix and its `dosdevices/`; on a plain-Wine
or pre-10 prefix it links `z:` to `../drive_z`, and a second run reuses
all of it. GE-Proton 10+ maps `z:` to `/` instead, so the whole host
filesystem is already visible under `Z:` — `--z` is a no-op there, and a
game directory is named as the host path (`Z:\mnt\games`, not
`Z:\games`). On an older prefix `--z DIR` links `DIR` into `drive_z` as
`Z:\<basename>`.

`run.sh` launches through the Proton script's `waitforexitandrun` verb with
`UMU_ID` set: Proton treats that as a non-Steam UMU launch and, given a
Windows-side exe path, runs Wine on it directly instead of injecting
through `steam.exe`, so stdout, stderr and the exit code come back to the
terminal. It deliberately does not use `runinprefix`: that runs Wine too,
but it skips Proton's prefix setup, so a fresh prefix never receives
dxvk/vkd3d.

Two environment levers decide whether the GPU path works, and `run.sh`
sets the first for you:

- **Native d3d12/dxgi.** Wine's built-in `d3d12` stub answers
  `E_INVALIDARG` and draws nothing; the GPU path needs the dxvk and
  vkd3d-proton copies Proton syncs into the prefix. Recent Proton forces
  this itself; `run.sh` defaults `WINEDLLOVERRIDES=d3d12=n;d3d12core=n;dxgi=n`
  when you haven't set it, so a hand-made prefix behaves the same. (The
  same requirement, with RADV frame measurements, is in the upstream
  Linux/Proton notes in fxceless/victory-fork#41.)
- **A native `d3dcompiler_47`.** Wine's builtin is a vendored
  vkd3d-shader: it has no `RWByteAddressBuffer.Interlocked*()` or
  `Load*()` methods and no `[RootSignature(...)]` handling, so `cs_5_1`
  compiles fail with `E5005` and the game's shaders can fail too. The
  working Linux path is the current `winetricks -q -f d3dcompiler_47`:
  it installs a native Microsoft DLL (from Mozilla's fxc2) that keeps the
  root signature. An old June-2010 redistributable is not enough — it
  refuses the 5_1 profile (`X3506`) and embeds no root signature at 5_0.
  `run.sh --d3dcompiler FILE` pins a copy from your own Windows 10/11
  `system32` instead, for a machine without winetricks or network. Either
  way keep the `d3dcompiler_47=n` override, and note the DLL is your
  data: the asset policy keeps it out of the repo.

## 3. The config for a Proton run

`victory.toml` is read from the process' working directory, and Wine
resolves the Unix cwd through its own Z: mapping — so relative config names
work on both sides. Windows paths inside the file, however, must be Windows
paths. Keep a second, git-ignored config:

```bash
cp victory.example.toml victory.proton.toml
```

```toml
[paths]
# GE-Proton 10+ maps Z: to /, so a host path /mnt/games is Z:\mnt\games.
game_dir = 'Z:\mnt\games'
# cache_dir/data_dir default to the prefix's LOCALAPPDATA, which is fine;
# or point them at Z:\somewhere for easy browsing from the Linux side.
```

and pass `--config victory.proton.toml`. The asset-policy rules are
unchanged: data lives outside the repo, and `cargo xtask policy` still
insists on it.

## 4. What works, what is Wine-dependent

Verified 2026-11-24 on GE-Proton11-11 (Wine 11.0 staging, dxvk/vkd3d-proton
synced into the prefix as native DLLs, `ntsync` kernel driver), with a
Win10-era native `d3dcompiler_47` as above:

- `proton/smoke.sh` passes: `config` resolves and prints the `host Wine`
  banner, the title directory is reachable through the prefix, and `census`
  runs against it.
- The whole `victory-gpu` test suite runs as a Windows binary under the same
  prefix: **109 of 111 pass**, covering device creation, the newer device
  levels, fences, heaps, copies, queue submission and the indirect-argument
  clamp — i.e. Proton really does provide the D3D12 ground victory hosts on.
  The one failure:
  - `pipeline::library::tests::a_rejected_blob_gives_an_empty_library`:
    vkd3d-proton errors on `OpenPipelineLibrary` of a garbage blob where
    real D3D12 hands back an empty library. A platform expectation, not a
    regression — it fails the same on plain `main` (fxceless/victory-fork#64).

  With only the June-2010 redistributable instead, four fail: the three
  that compile `*_5_1` inline shaders (the 2010 build refuses the 5_1
  profile — `X3506` — and even at 5_0 embeds no root signature), plus the
  library test. `crate::host::compile` retries 5_1 at 5_0 for compilers too
  old for the 5_1 profile, which helps Win8-era copies; it cannot rescue
  the 2010 one, so use the current winetricks DLL or a Win10-era copy.

Known gaps and unverified areas (file an issue if you hit one):

- **Shader validation**: `DxcCreateInstance` needs dxcompiler — Wine has no
  DXC. Translation output skips validation as it does on Windows without
  `dxil.dll`; treat translation results under Wine as unvalidated.
- **Diagnostics build** (`--features diagnostics`, `--diagnostics-impls`):
  the write-watch queries and the debug layer have had no Wine testing.
- **Input/audio**: hle's gamepad and audio paths were built against Windows
  behaviour; under Wine they ride whatever Wine presents. Unverified.
- **The start screen needs input.** Without a pad, the title sits at 30 fps
  and 13 draws a frame on the start screen for ever; the log looks like a
  loader that never finishes and shows no error. A diagnostics build
  (`--features diagnostics`) can press Enter on a schedule:

  ```bash
  VICTORY_KEYS=25:Enter,40:Enter,55:Enter,70:Enter proton/run.sh ...
  ```

  Two presses are not always enough; four at roughly 15-second spacing
  worked. The release build has no scripted keys, so use a pad or the
  diagnostics build. A run that never pressed anything is not evidence
  that loading is broken.
- **HDR**: with HDR on, vkd3d-proton present throttles to ~400 ms a frame
  and the 30 s watchdog ends a run within the first minute — keep HDR off
  on the display for Proton runs (#41 measures the same on RADV). 10-bit
  output remains untested.
- A full `victory run` under Proton works on AMD/RADV: menus, intro,
  loading, then world streaming at 1 777–5 292 draws a frame, 0.1–0.5 fps
  in the streaming phase, 87–241 ms of GPU time (CachyOS, RX 7900 XT,
  Proton Experimental, #41). The two levers are the native d3d12/dxgi
  override `run.sh` sets and HDR off on the display. The exact numbers are
  machine-specific; the smoke box for the suite above has not recorded a
  full run.
- **Texture-lane waits**: under Wine the texture lane's remap-lock wait is
  a mapping lock that costs up to seconds per window;
  `VICTORY_TEXTURE_NO_WAIT=1` is on by default in `run.sh` (opt out with
  `--texture-wait`) and takes it to zero at the cost of up to two textures
  a frame deferred to the next one — measurements in
  fxceless/victory-fork#178.
- **`inspect` examples are illustrative**: the `final.js.c.c.gmpc` blob
  name is build-specific. Use a file that actually exists under your game
  directory, or skip `inspect` and start with `config` and `census`.

## 5. Tests and CI

The Ubuntu job runs the native Linux test suite; the Windows job runs the
same suite plus clippy. `cargo test -p xtask` checks the launch scripts
parse, their usage text holds, a fresh prefix gets its `dosdevices/z:` link,
and GE-Proton wins the default selection over an older plain Proton;
`proton/smoke.sh` is the Proton-path smoke test — run it after touching
`proton/`, and paste its output into bug reports.
