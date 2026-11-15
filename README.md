# Guitar Rig

A modular guitar pedalboard + amp plugin written in Rust with
[nih-plug](https://github.com/robbert-vdh/nih-plug). Builds as **VST3** and
**CLAP** on macOS and Linux, plus an optional standalone app.

## Modules (v0.1)

| Module     | Controls                                              | Notes |
|------------|-------------------------------------------------------|-------|
| Tuner      | Reference (430–450 Hz), Mute output                   | YIN pitch detection on a decimated signal; note, octave, cents, Hz |
| Distortion | Drive, Tone, Level                                    | Tight HPF + mid hump, asymmetric anti-aliased (ADAA) tanh clipping |
| Amp        | Gain, Bass, Mid, Treble, Presence, Master, Cabinet    | Two preamp stages, tone stack, power-amp saturation, 4x12 cab sim |
| Delay      | Time (20–2000 ms), Feedback, Tone, Mix                | Filtered, softly saturated feedback; smoothed time gives tape-style bends |
| Reverb     | Size, Damping, Pre-delay, Mix                         | Freeverb-style (8 combs + 4 allpasses), band-limited wet signal |

Plus global Input/Output gain with peak meters.

## GUI

- **Pedalboard**: the chain is drawn as pedals connected by patch cables from
  `IN` to `OUT`. Mini knobs on each pedal show the current settings.
- **Drag** a pedal to reorder the chain; **click** to edit it; click its
  **footswitch** to bypass/engage it (crossfaded, so no clicks).
- **Toolbar**: `+ Module` re-connects a removed module; `◀ Move`, `Move ▶`,
  `Remove` act on the selected one.
- **Parameter panel**: knobs for the selected module. Drag vertically (Shift =
  fine), scroll to nudge, double-click to reset.

The chain order is saved with the plugin state/session. Every parameter
(including each module's on/off) is host-automatable.

## Building

Requires a recent stable Rust toolchain.

```sh
# Plugin bundles -> target/bundled/Guitar Rig.vst3 and Guitar Rig.clap
cargo xtask bundle guitar_rig --release

# Standalone app (CoreAudio on macOS, JACK on Linux)
cargo run --release --features standalone --bin guitar_rig_standalone
# List options (input device, sample rate, buffer size, ...):
cargo run --release --features standalone --bin guitar_rig_standalone -- --help

# Tests
cargo test --release
```

**Linux** additionally needs X11/OpenGL dev packages, e.g. on Debian/Ubuntu:
`sudo apt install libx11-xcb-dev libxcursor-dev libxcb-icccm4-dev libxcb-dri2-0-dev libgl-dev libasound2-dev libjack-jackd2-dev`.

### Installing

- macOS: copy `Guitar Rig.vst3` to `~/Library/Audio/Plug-Ins/VST3/` and
  `Guitar Rig.clap` to `~/Library/Audio/Plug-Ins/CLAP/`.
- Linux: `~/.vst3/` and `~/.clap/`.

## Architecture

```
src/
  lib.rs           Plugin: I/O, sub-block processing, chain execution, bypass crossfades
  params.rs        All parameters (IDs are stable — don't rename them)
  chain.rs         Chain order, packed lock-free into an AtomicU64 shared GUI <-> audio
  dsp/             filters (biquads), shaper (ADAA tanh), tuner, distortion, amp, delay, reverb
  editor.rs        egui GUI: pedalboard, toolbar, parameter panels, tuner display
  editor/widgets.rs  Knob, toggle and meter widgets bound to parameters
```

- The guitar signal is processed in mono (stereo inputs are averaged) and sent
  to all output channels.
- The audio thread never locks or allocates; buffers are allocated in
  `initialize()`. Debug builds enforce this via `assert_process_allocs`.

To add a module: add a variant to `ModuleKind`, a params struct to
`GuitarRigParams`, a DSP struct under `dsp/`, and the match arms in `lib.rs`
and `editor.rs`.

## License

nih-plug's VST3 bindings are GPLv3, so a distributed VST3 build of this plugin
must be GPLv3 too (see `Cargo.toml`). The CLAP build alone has no such requirement.
