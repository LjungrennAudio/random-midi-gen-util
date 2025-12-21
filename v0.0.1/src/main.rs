use clap::{Parser, ValueEnum};
use midly::{Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind};

use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use std::fs;

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ScaleOpt {
    Major,
    NaturalMinor,
    MinorPentatonic,
    MajorPentatonic,
}

#[derive(Debug, Clone, Copy)]
struct Note(u8);

#[derive(Debug, Parser)]
#[command(
    name = "midi-seed-gen",
    version,
    about = "Seeded random MIDI (format 0) generator"
)]
struct Cli {
    /// Output .mid path (if omitted, a timestamped name is generated)
    #[arg(short, long)]
    out: Option<String>,

    /// RNG seed (same seed => same MIDI)
    #[arg(long, default_value_t = 0xC0FFEEu64)]
    seed: u64,

    /// Tempo in BPM
    #[arg(long, default_value_t = 120u32)]
    bpm: u32,

    /// Bars (assumes 4/4)
    #[arg(long, default_value_t = 16u32)]
    bars: u32,

    /// Ticks per quarter note (PPQN)
    #[arg(long, default_value_t = 480u16)]
    ppqn: u16,

    /// Root note in scientific pitch notation (e.g. C4, A3, F#5, Db2)
    #[arg(long, default_value = "C4")]
    root: Note,
    // [formerly] Root MIDI note number (60=C4)
    // #[arg(long, default_value_t = 60u8)]
    // root: u8,
    /// Scale / mode
    #[arg(long, value_enum, default_value_t = ScaleOpt::MinorPentatonic)]
    scale: ScaleOpt,

    /// MIDI channel (0..15). (Channel 9 is the “10th channel” used for drums in GM practice.)
    #[arg(long, default_value_t = 0u8)]
    channel: u8,

    /// Program (0..127). 0 = Acoustic Grand Piano in General MIDI.
    #[arg(long, default_value_t = 0u8)]
    program: u8,
}

impl Note {
    fn as_u8(self) -> u8 {
        self.0
    }
}

impl std::str::FromStr for Note {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let s = input.trim();
        if s.is_empty() {
            return Err("empty note".into());
        }

        let mut it = s.chars();

        // Letter
        let letter = it.next().ok_or_else(|| "empty note".to_string())?;
        let base_pc: i32 = match letter.to_ascii_uppercase() {
            'C' => 0,
            'D' => 2,
            'E' => 4,
            'F' => 5,
            'G' => 7,
            'A' => 9,
            'B' => 11,
            _ => return Err(format!("bad note letter: {letter}")),
        };

        // Optional accidental
        let mut pc = base_pc;
        let mut octave_str = it.as_str(); // remaining tail, no allocation

        if let Some(acc) = it.clone().next() {
            match acc {
                '#' | '♯' => {
                    pc += 1;
                    it.next(); // consume it
                    octave_str = it.as_str();
                }
                'b' | 'B' | '♭' => {
                    pc -= 1;
                    it.next(); // consume it
                    octave_str = it.as_str();
                }
                _ => {}
            }
        }

        let octave_str = octave_str.trim();
        if octave_str.is_empty() {
            return Err("missing octave, expected like C#4".into());
        }

        let octave: i32 = octave_str
            .parse()
            .map_err(|_| format!("bad octave: {octave_str}"))?;

        // C4 = 60 mapping (SPN convention); MIDI standardizes Middle C as 60, but octave label conventions vary. [web:109]
        let midi: i32 = (octave + 1) * 12 + pc;

        if !(0..=127).contains(&midi) {
            return Err(format!("note out of MIDI range 0..127: {midi}"));
        }

        Ok(Note(midi as u8))
    }
}

// Generate default output path if none provided
fn default_out_path(seed: u64) -> String {
    let ts = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string(); // formatting via chrono [web:139]
    format!("out/seeded_{ts}_{seed}.mid")
}

fn bpm_to_us_per_quarter(bpm: u32) -> u32 {
    // 60_000_000 microseconds per minute / bpm
    60_000_000u32 / bpm.max(1)
}

fn scale_semitones(s: ScaleOpt) -> &'static [i8] {
    match s {
        ScaleOpt::Major => &[0, 2, 4, 5, 7, 9, 11],
        ScaleOpt::NaturalMinor => &[0, 2, 3, 5, 7, 8, 10],
        ScaleOpt::MinorPentatonic => &[0, 3, 5, 7, 10],
        ScaleOpt::MajorPentatonic => &[0, 2, 4, 7, 9],
    }
}

fn weighted_choice<R: Rng>(rng: &mut R, items: &[(u8, u32)]) -> u8 {
    let total: u32 = items.iter().map(|(_, w)| *w).sum();
    let mut x = rng.gen_range(0..total.max(1));
    for (v, w) in items {
        if x < *w {
            return *v;
        }
        x -= *w;
    }
    items.last().unwrap().0
}

fn event_order_key(kind: &TrackEventKind) -> u8 {
    match kind {
        TrackEventKind::Midi { message, .. } => match message {
            MidiMessage::NoteOff { .. } => 0,
            MidiMessage::NoteOn { .. } => 1,
            _ => 2,
        },
        TrackEventKind::Meta(_) => 3,
        TrackEventKind::SysEx(_) | TrackEventKind::Escape(_) => 4,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse(); // clap derive parsing [web:68]

    // Determine output path
    let out_path = cli
        .out
        .clone()
        .unwrap_or_else(|| default_out_path(cli.seed));

    // Basic argument sanity
    if cli.channel > 15 {
        return Err("channel must be 0..15".into());
    }
    if cli.program > 127 {
        return Err("program must be 0..127".into());
    }

    let mut rng = ChaCha8Rng::seed_from_u64(cli.seed);

    let scale = scale_semitones(cli.scale);
    // Root note in scientific pitch notation (e.g. C4, A3, F#5, Db2)
    let base_note = cli.root.as_u8() as i16;
    // [formerly] Root MIDI note number (60=C4)
    // let base_note = cli.root as i16;

    // 4/4 grid: 16 steps per bar => 1/16 note per step.
    let steps_per_bar = 16u32;
    let step_ticks: u32 = (cli.ppqn as u32) / 4; // 1/16 = quarter/4
    let total_steps: u32 = cli.bars * steps_per_bar;
    let song_len_ticks: u32 = total_steps * step_ticks;

    // Collect events as (absolute_tick, kind)
    let mut abs_events: Vec<(u32, TrackEventKind)> = Vec::new();

    // Tempo at time 0: Set Tempo meta message (µs per quarter note). [web:14][web:91]
    let us_per_qn = bpm_to_us_per_quarter(cli.bpm);
    abs_events.push((
        0,
        TrackEventKind::Meta(MetaMessage::Tempo(us_per_qn.into())),
    ));

    // Program change at time 0 (instrument selection). [web:91]
    abs_events.push((
        0,
        TrackEventKind::Midi {
            channel: cli.channel.into(),
            message: MidiMessage::ProgramChange {
                program: cli.program.into(),
            },
        },
    ));

    // Stochastic melody with light constraints.
    let mut last_degree: i32 = 0;
    for step in 0..total_steps {
        let t0 = step * step_ticks;

        // Note density: 45% notes, 55% rests.
        if rng.gen_range(0..100u32) < 55 {
            continue;
        }

        // Choose a degree index in 0..scale.len()
        // Slightly bias toward "stable-ish" degrees (0 and 2 if they exist).
        let max_deg = (scale.len() as i32).max(1);
        let target = if max_deg >= 3 {
            weighted_choice(&mut rng, &[(0, 30), (1, 15), (2, 30), (3, 15), (4, 10)]) as i32
        } else {
            rng.gen_range(0..max_deg as u32) as i32
        };
        let target = target.clamp(0, max_deg - 1);

        // Encourage stepwise motion 65% of the time.
        let deg = if rng.gen_range(0..100u32) < 65 {
            let delta = match rng.gen_range(0..3u32) {
                0 => -1,
                1 => 0,
                _ => 1,
            };
            (last_degree + delta).clamp(0, max_deg - 1)
        } else {
            target
        };
        last_degree = deg;

        let semis = scale[deg as usize] as i16;

        // Occasionally jump an octave up/down (but keep in MIDI 0..127).
        let octave_shift: i16 = match rng.gen_range(0..100u32) {
            0..=9 => 12,
            10..=14 => -12,
            _ => 0,
        };

        let note_i16 = base_note + semis + octave_shift;
        let note_u8 = note_i16.clamp(0, 127) as u8;

        // Duration in steps (1..4) with weights.
        let dur_steps: u32 =
            weighted_choice(&mut rng, &[(1, 40), (2, 30), (3, 10), (4, 20)]) as u32;

        let t1 = (t0 + dur_steps * step_ticks).min(song_len_ticks);

        // Velocity: accent on quarter-note boundaries.
        let accent: u8 = if step % 4 == 0 { 18 } else { 0 };
        let vel: u8 = (rng.gen_range(55..95) as u16 + accent as u16).min(127) as u8;

        abs_events.push((
            t0,
            TrackEventKind::Midi {
                channel: cli.channel.into(),
                message: MidiMessage::NoteOn {
                    key: note_u8.into(),
                    vel: vel.into(),
                },
            },
        ));

        abs_events.push((
            t1,
            TrackEventKind::Midi {
                channel: cli.channel.into(),
                message: MidiMessage::NoteOff {
                    key: note_u8.into(),
                    vel: 0.into(),
                },
            },
        ));
    }

    // Sort and ensure sensible ordering at identical timestamps.
    abs_events.sort_by(|(ta, ea), (tb, eb)| {
        ta.cmp(tb)
            .then_with(|| event_order_key(ea).cmp(&event_order_key(eb)))
    });

    // Convert abs->delta and build the single track. [web:91]
    let mut track: Vec<TrackEvent> = Vec::new();
    let mut last_tick: u32 = 0;
    for (tick, kind) in abs_events {
        let delta = tick.saturating_sub(last_tick);
        last_tick = tick;
        track.push(TrackEvent {
            delta: delta.into(), 
            kind,
        });
    }

    // End-of-track meta event must be last. [web:91]
    track.push(TrackEvent {
        delta: 0.into(),
        kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
    });

    // Format 0 = SingleTrack in midly. [web:36]
    let header = Header::new(Format::SingleTrack, Timing::Metrical(cli.ppqn.into()));
    let smf = Smf {
        header,
        tracks: vec![track],
    };

    // Write file
    if let Some(parent) = std::path::Path::new(&out_path).parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    smf.save(&out_path)?; // midly save helper
    eprintln!("Wrote {}", out_path);
    Ok(())
}
