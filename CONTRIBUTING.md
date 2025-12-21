[CONTRIBUTING.md]


---

# Contributing

Thanks for your interest in improving **random-midi-gen-util**!


## Code style
- Follow Rust 2021 edition conventions.
- Run `cargo fmt` before commits.
- Ensure all warnings are resolved: `cargo clippy -- -D warnings`.
- Keep logic minimal and prefer small, auditable dependencies.


## Development workflow
1. Fork the repository. [file:172]
2. Create a feature branch:
```bash
git checkout -b feature/your-feature
```

3. Commit changes with clear messages (examples):
feat: add dorian scale option
fix: handle negative octave parsing for --root
docs: improve README examples

4. Push and open a Pull Request against `main`. [file:172]


## Testing
Run:
```bash
cargo build --release
cargo test
```

For manual verification:
- Generate a MIDI file and import it into a DAW/player to confirm it loads and plays correctly. [file:172]


## Documentation
If you add or change CLI options, please update README.md and ensure `--help` output remains clean and accurate. [file:172]


## Communication
Open a GitHub Issue for:
- Bug reports
- Feature requests
- Questions / clarifications

---
