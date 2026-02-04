# MIDI Seed Generator - Piano Roll Edition

Minimal seeded MIDI melody generator with visual piano roll and real-time playback.

## Features

- **CLI Mode**: Generate and save MIDI files (original functionality preserved)
- **GUI Mode**: Visual piano roll with real-time MIDI playback
- Deterministic generation from seed values
- Clean, minimal interface - just what you need to see random melodies

## Build

```bash
cargo build --release
```

## Usage

### CLI Mode (Save MIDI file)

```bash
# Generate with defaults
./target/release/midi_seed_gen

# Custom parameters
./target/release/midi_seed_gen --seed 0xDEADBEEF --bpm 140 --bars 8 --root "A3" --scale major

# Save to specific path
./target/release/midi_seed_gen -o my_melody.mid
```

### GUI Mode (Piano Roll + Playback)

```bash
# Launch GUI with defaults
./target/release/midi_seed_gen --gui

# Custom parameters in GUI
./target/release/midi_seed_gen --gui --seed 0xC0FFEE --bpm 140 --bars 8
```

## GUI Controls

- **PLAY** - Start/stop playback (red line shows position)
- **REGENERATE** - Generate new random melody with different seed
- **Visual piano roll** - Notes colored by velocity, time grid shows beats

## MIDI Output

The GUI requires a MIDI output device. On Linux, you can use:
- FluidSynth: `fluidsynth -a alsa -m alsa_seq /usr/share/sounds/sf2/FluidR3_GM.sf2`
- Virtual MIDI port: `modprobe snd-virmidi`

On Windows/Mac, it will use your default MIDI output device.

## Parameters

All CLI parameters work in both modes:

- `--seed` - RNG seed (same seed = same melody)
- `--bpm` - Tempo (default: 120)
- `--bars` - Length in bars (default: 16)
- `--root` - Root note like "C4", "F#3", "Bb5" (default: "C4")
- `--scale` - major, natural-minor, minor-pentatonic, major-pentatonic
- `--channel` - MIDI channel 0-15 (default: 0)
- `--program` - GM instrument 0-127 (default: 0 = piano)

## Example Session

```bash
# Generate a quick random melody in GUI
./target/release/midi_seed_gen --gui

# Like what you heard? Save it to file
./target/release/midi_seed_gen --seed 0xTHATSEED -o keeper.mid

# Try different scales
./target/release/midi_seed_gen --gui --scale major --bpm 160 --bars 4
```

## Architecture

- **macroquad** - Lightweight game framework for piano roll rendering
- **midir** - Cross-platform MIDI I/O for real-time playback
- **midly** - MIDI file format handling (save to .mid)
- **ChaCha8Rng** - Deterministic random generation (same seed = same output)

## Notes

- Piano roll shows note velocity as opacity
- Playback thread runs independently from rendering
- All original CLI functionality preserved - just add `--gui` flag
- No DAW replacement - just a quick melody viewer/player fren

---

Built for rapid random melody generation. Keep it simple. 🎹✨
