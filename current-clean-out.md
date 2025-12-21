PS D:\\code\\random-midi-gen-util\\v0.0.1> cargo check

&nbsp;   Checking midi\_seed\_gen v0.1.0 (D:\\code\\random-midi-gen-util\\v0.0.1)

&nbsp;   Finished `dev` profile \[unoptimized + debuginfo] target(s) in 0.39s

PS D:\\code\\random-midi-gen-util\\v0.0.1> cargo clippy -- -D warnings

&nbsp;   Checking midi\_seed\_gen v0.1.0 (D:\\code\\random-midi-gen-util\\v0.0.1)

&nbsp;   Finished `dev` profile \[unoptimized + debuginfo] target(s) in 0.36s

PS D:\\code\\random-midi-gen-util\\v0.0.1> cargo check

&nbsp;   Checking midi\_seed\_gen v0.1.0 (D:\\code\\random-midi-gen-util\\v0.0.1)

&nbsp;   Finished `dev` profile \[unoptimized + debuginfo] target(s) in 0.30s

PS D:\\code\\random-midi-gen-util\\v0.0.1> cargo build --release

&nbsp;  Compiling midi\_seed\_gen v0.1.0 (D:\\code\\random-midi-gen-util\\v0.0.1)

&nbsp;   Finished `release` profile \[optimized] target(s) in 1.99s

PS D:\\code\\random-midi-gen-util\\v0.0.1> cargo run -- --seed 1990754 --bpm 140 --bars 16 --root c4 --scale natural-minor --program 80

&nbsp;  Compiling midi\_seed\_gen v0.1.0 (D:\\code\\random-midi-gen-util\\v0.0.1)

&nbsp;   Finished `dev` profile \[unoptimized + debuginfo] target(s) in 1.10s

&nbsp;    Running `target\\debug\\midi\_seed\_gen.exe --seed 1990754 --bpm 140 --bars 16 --root c4 --scale natural-minor --program 80`

Wrote out/seeded\_20251221\_084459\_1990754.mid

PS D:\\code\\random-midi-gen-util\\v0.0.1>

