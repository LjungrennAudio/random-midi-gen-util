use clap::{Parser, ValueEnum};
use midly::{Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind};
use rand_chacha::ChaCha8Rng;
use std::fs;
use std::error::Error;

// GUI imports
use macroquad::prelude::*;
use midir::{MidiOutput, MidiOutputConnection};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

// Import rand traits explicitly to avoid macroquad conflict
use ::rand::{Rng, SeedableRng};

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

    /// Scale / mode
    #[arg(long, value_enum, default_value_t = ScaleOpt::MinorPentatonic)]
    scale: ScaleOpt,

    /// MIDI channel (0..15)
    #[arg(long, default_value_t = 0u8)]
    channel: u8,

    /// Program (0..127). 0 = Acoustic Grand Piano in General MIDI.
    #[arg(long, default_value_t = 0u8)]
    program: u8,

    /// Launch GUI piano roll viewer
    #[arg(long, default_value_t = false)]
    gui: bool,
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

        let mut pc = base_pc;
        let mut octave_str = it.as_str();

        if let Some(acc) = it.clone().next() {
            match acc {
                '#' | '♯' => {
                    pc += 1;
                    it.next();
                    octave_str = it.as_str();
                }
                'b' | 'B' | '♭' => {
                    pc -= 1;
                    it.next();
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

        let midi: i32 = (octave + 1) * 12 + pc;

        if !(0..=127).contains(&midi) {
            return Err(format!("note out of MIDI range 0..127: {midi}"));
        }

        Ok(Note(midi as u8))
    }
}

#[derive(Clone, Debug)]
struct MidiNote {
    pitch: u8,
    start_tick: u32,
    end_tick: u32,
    velocity: u8,
}

struct MidiSequence {
    notes: Vec<MidiNote>,
    bpm: u32,
    ppqn: u16,
    total_ticks: u32,
}

fn default_out_path(seed: u64) -> String {
    let ts = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
    format!("out/seeded_{ts}_{seed}.mid")
}

fn bpm_to_us_per_quarter(bpm: u32) -> u32 {
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

fn generate_sequence(cli: &Cli) -> Result<MidiSequence, Box<dyn Error>> {
    let mut rng = ChaCha8Rng::seed_from_u64(cli.seed);
    let scale = scale_semitones(cli.scale);
    let base_note = cli.root.as_u8() as i16;

    let steps_per_bar = 16u32;
    let step_ticks: u32 = (cli.ppqn as u32) / 4;
    let total_steps: u32 = cli.bars * steps_per_bar;
    let song_len_ticks: u32 = total_steps * step_ticks;

    let mut notes = Vec::new();
    let mut last_degree: i32 = 0;

    for step in 0..total_steps {
        let t0 = step * step_ticks;

        if rng.gen_range(0..100u32) < 55 {
            continue;
        }

        let max_deg = (scale.len() as i32).max(1);
        let target = if max_deg >= 3 {
            weighted_choice(&mut rng, &[(0, 30), (1, 15), (2, 30), (3, 15), (4, 10)]) as i32
        } else {
            rng.gen_range(0..max_deg as u32) as i32
        };
        let target = target.clamp(0, max_deg - 1);

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
        let octave_shift: i16 = match rng.gen_range(0..100u32) {
            0..=9 => 12,
            10..=14 => -12,
            _ => 0,
        };

        let note_i16 = base_note + semis + octave_shift;
        let note_u8 = note_i16.clamp(0, 127) as u8;

        let dur_steps: u32 =
            weighted_choice(&mut rng, &[(1, 40), (2, 30), (3, 10), (4, 20)]) as u32;

        let t1 = (t0 + dur_steps * step_ticks).min(song_len_ticks);

        let accent: u8 = if step % 4 == 0 { 18 } else { 0 };
        let vel: u8 = (rng.gen_range(55..95) as u16 + accent as u16).min(127) as u8;

        notes.push(MidiNote {
            pitch: note_u8,
            start_tick: t0,
            end_tick: t1,
            velocity: vel,
        });
    }

    Ok(MidiSequence {
        notes,
        bpm: cli.bpm,
        ppqn: cli.ppqn,
        total_ticks: song_len_ticks,
    })
}

fn save_sequence(seq: &MidiSequence, cli: &Cli, out_path: &str) -> Result<(), Box<dyn Error>> {
    let mut abs_events: Vec<(u32, TrackEventKind)> = Vec::new();

    let us_per_qn = bpm_to_us_per_quarter(seq.bpm);
    abs_events.push((
        0,
        TrackEventKind::Meta(MetaMessage::Tempo(us_per_qn.into())),
    ));

    abs_events.push((
        0,
        TrackEventKind::Midi {
            channel: cli.channel.into(),
            message: MidiMessage::ProgramChange {
                program: cli.program.into(),
            },
        },
    ));

    for note in &seq.notes {
        abs_events.push((
            note.start_tick,
            TrackEventKind::Midi {
                channel: cli.channel.into(),
                message: MidiMessage::NoteOn {
                    key: note.pitch.into(),
                    vel: note.velocity.into(),
                },
            },
        ));

        abs_events.push((
            note.end_tick,
            TrackEventKind::Midi {
                channel: cli.channel.into(),
                message: MidiMessage::NoteOff {
                    key: note.pitch.into(),
                    vel: 0.into(),
                },
            },
        ));
    }

    abs_events.sort_by(|(ta, ea), (tb, eb)| {
        ta.cmp(tb)
            .then_with(|| event_order_key(ea).cmp(&event_order_key(eb)))
    });

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

    track.push(TrackEvent {
        delta: 0.into(),
        kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
    });

    let header = Header::new(Format::SingleTrack, Timing::Metrical(seq.ppqn.into()));
    let smf = Smf {
        header,
        tracks: vec![track],
    };

    if let Some(parent) = std::path::Path::new(out_path).parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    smf.save(out_path)?;
    Ok(())
}

// ============================================================================
// GUI MODE
// ============================================================================

struct PlaybackState {
    playing: bool,
    current_tick: u32,
}

fn setup_midi_output() -> Result<MidiOutputConnection, Box<dyn Error>> {
    let midi_out = MidiOutput::new("MIDI Seed Gen")?;
    let out_ports = midi_out.ports();
    
    if out_ports.is_empty() {
        return Err("No MIDI output ports available".into());
    }
    
    // Use first available port
    let out_port = &out_ports[0];
    let port_name = midi_out.port_name(out_port).unwrap_or_else(|_| "Unknown".to_string());
    println!("Connected to MIDI output: {}", port_name);
    
    let conn = midi_out.connect(out_port, "midi-gen-output")?;
    Ok(conn)
}

fn spawn_playback_thread(
    seq: MidiSequence,
    channel: u8,
    state: Arc<Mutex<PlaybackState>>,
) {
    thread::spawn(move || {
        let mut midi_out = match setup_midi_output() {
            Ok(m) => m,
            Err(e) => {
                eprintln!("Failed to setup MIDI: {}", e);
                return;
            }
        };

        loop {
            let (playing, current_tick) = {
                let s = state.lock().unwrap();
                (s.playing, s.current_tick)
            };

            if !playing {
                thread::sleep(Duration::from_millis(50));
                continue;
            }

            // Play notes that start at current tick
            for note in &seq.notes {
                if note.start_tick == current_tick {
                    let note_on = [0x90 | channel, note.pitch, note.velocity];
                    midi_out.send(&note_on).ok();
                }
                if note.end_tick == current_tick {
                    let note_off = [0x80 | channel, note.pitch, 0];
                    midi_out.send(&note_off).ok();
                }
            }

            // Advance tick
            {
                let mut s = state.lock().unwrap();
                s.current_tick += 1;
                if s.current_tick >= seq.total_ticks {
                    s.current_tick = 0;
                }
            }

            // Calculate sleep duration based on BPM and PPQN
            let microseconds_per_tick = (bpm_to_us_per_quarter(seq.bpm) as f64) / (seq.ppqn as f64);
            let sleep_duration = Duration::from_micros(microseconds_per_tick as u64);
            thread::sleep(sleep_duration);
        }
    });
}

async fn run_gui(mut cli: Cli, mut seq: MidiSequence) {
    let state = Arc::new(Mutex::new(PlaybackState {
        playing: false,
        current_tick: 0,
    }));

    spawn_playback_thread(seq.clone(), cli.channel, Arc::clone(&state));

    loop {
        clear_background(Color::from_rgba(15, 15, 20, 255));

        // Calculate dimensions
        let panel_height = 100.0;
        let piano_roll_y = panel_height;
        let piano_roll_height = screen_height() - panel_height;

        // Find pitch range
        let min_pitch = seq.notes.iter().map(|n| n.pitch).min().unwrap_or(60) - 2;
        let max_pitch = seq.notes.iter().map(|n| n.pitch).max().unwrap_or(72) + 2;
        let pitch_range = (max_pitch - min_pitch + 1) as f32;

        // Time scaling
        let time_scale = (screen_width() - 100.0) / seq.total_ticks as f32;

        // ===== CONTROL PANEL =====
        draw_rectangle(0.0, 0.0, screen_width(), panel_height, Color::from_rgba(25, 25, 30, 255));

        // Title
        draw_text(
            &format!("MIDI SEED GENERATOR - Seed: 0x{:X}", cli.seed),
            20.0,
            30.0,
            24.0,
            WHITE,
        );
        draw_text(
            &format!("BPM: {} | Scale: {:?} | Root: {}", seq.bpm, cli.scale, cli.root.as_u8()),
            20.0,
            55.0,
            18.0,
            LIGHTGRAY,
        );

        // Buttons
        let play_btn_x = 20.0;
        let play_btn_y = 70.0;
        let btn_w = 100.0;
        let btn_h = 25.0;

        let (playing, current_tick) = {
            let s = state.lock().unwrap();
            (s.playing, s.current_tick)
        };

        // Play/Stop button
        let play_color = if playing {
            Color::from_rgba(255, 60, 60, 255)
        } else {
            Color::from_rgba(0, 255, 128, 255)
        };
        draw_rectangle(play_btn_x, play_btn_y, btn_w, btn_h, play_color);
        let play_text = if playing { "STOP" } else { "PLAY" };
        draw_text(play_text, play_btn_x + 25.0, play_btn_y + 18.0, 20.0, BLACK);

        if is_mouse_button_pressed(MouseButton::Left) {
            let (mx, my) = mouse_position();
            if mx >= play_btn_x && mx <= play_btn_x + btn_w && my >= play_btn_y && my <= play_btn_y + btn_h {
                let mut s = state.lock().unwrap();
                s.playing = !s.playing;
                if s.playing {
                    s.current_tick = 0;
                }
            }
        }

        // Regenerate button
        let regen_btn_x = play_btn_x + btn_w + 10.0;
        draw_rectangle(regen_btn_x, play_btn_y, btn_w + 20.0, btn_h, Color::from_rgba(60, 150, 255, 255));
        draw_text("REGENERATE", regen_btn_x + 10.0, play_btn_y + 18.0, 18.0, BLACK);

        if is_mouse_button_pressed(MouseButton::Left) {
            let (mx, my) = mouse_position();
            if mx >= regen_btn_x && mx <= regen_btn_x + btn_w + 20.0 && my >= play_btn_y && my <= play_btn_y + btn_h {
                cli.seed = ::rand::random();
                seq = generate_sequence(&cli).unwrap();
                let mut s = state.lock().unwrap();
                s.playing = false;
                s.current_tick = 0;
            }
        }

        // ===== PIANO ROLL =====
        // Draw background
        draw_rectangle(0.0, piano_roll_y, screen_width(), piano_roll_height, Color::from_rgba(20, 20, 25, 255));

        // Draw piano keys (left side)
        let key_width = 80.0;
        for pitch in min_pitch..=max_pitch {
            let y = piano_roll_y + ((max_pitch - pitch) as f32 / pitch_range) * piano_roll_height;
            let row_height = piano_roll_height / pitch_range;

            // White/black key coloring
            let note_class = pitch % 12;
            let is_black = matches!(note_class, 1 | 3 | 6 | 8 | 10);
            let key_color = if is_black {
                Color::from_rgba(30, 30, 35, 255)
            } else {
                Color::from_rgba(45, 45, 50, 255)
            };

            draw_rectangle(0.0, y, key_width, row_height, key_color);
            draw_line(0.0, y, screen_width(), y, 1.0, Color::from_rgba(40, 40, 45, 255));

            // Note name
            let note_name = note_to_string(pitch);
            draw_text(&note_name, 10.0, y + row_height / 2.0 + 5.0, 16.0, LIGHTGRAY);
        }

        // Draw time grid
        let quarters = (seq.total_ticks / seq.ppqn as u32) as usize;
        for q in 0..=quarters {
            let x = key_width + (q as f32 * seq.ppqn as f32 * time_scale);
            let color = if q % 4 == 0 {
                Color::from_rgba(80, 80, 90, 255)
            } else {
                Color::from_rgba(50, 50, 55, 255)
            };
            draw_line(x, piano_roll_y, x, screen_height(), 1.0, color);
        }

        // Draw notes
        for note in &seq.notes {
            let y = piano_roll_y + ((max_pitch - note.pitch) as f32 / pitch_range) * piano_roll_height;
            let row_height = piano_roll_height / pitch_range;
            let x = key_width + (note.start_tick as f32 * time_scale);
            let width = ((note.end_tick - note.start_tick) as f32 * time_scale).max(2.0);

            // Velocity to opacity
            let alpha = (note.velocity as f32 / 127.0 * 0.6 + 0.4) as u8;
            
            let note_color = Color::from_rgba(0, 180, 255, alpha.saturating_mul(255));
            draw_rectangle(x, y + 2.0, width, row_height - 4.0, note_color);
            draw_rectangle_lines(x, y + 2.0, width, row_height - 4.0, 1.0, Color::from_rgba(100, 200, 255, 200));
        }

        // Draw playhead
        if playing {
            let playhead_x = key_width + (current_tick as f32 * time_scale);
            draw_line(playhead_x, piano_roll_y, playhead_x, screen_height(), 2.0, Color::from_rgba(255, 60, 60, 255));
        }

        next_frame().await
    }
}

fn note_to_string(pitch: u8) -> String {
    let note_names = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    let octave = (pitch / 12) as i32 - 1;
    let note = note_names[(pitch % 12) as usize];
    format!("{}{}", note, octave)
}

// ============================================================================
// MAIN
// ============================================================================

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    let seq = generate_sequence(&cli)?;

    if cli.gui {
        // Launch GUI
        let window_conf = Conf {
            window_title: "MIDI Seed Generator - Piano Roll".to_owned(),
            window_width: 1400,
            window_height: 700,
            ..Default::default()
        };
        
        macroquad::Window::new(window_conf, async move {
            run_gui(cli, seq).await;
        });
        
        Ok(())
    } else {
        // CLI mode - just save file
        let out_path = cli
            .out
            .clone()
            .unwrap_or_else(|| default_out_path(cli.seed));

        save_sequence(&seq, &cli, &out_path)?;
        eprintln!("Wrote {}", out_path);
        Ok(())
    }
}
