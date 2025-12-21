[SECURITY.md]

---

# Security Policy


## Supported versions
| Version   | Supported  |
|--------:|:---------:|
| 0.1.x            | ✅ Active    |


## Reporting a vulnerability
If you discover a potential vulnerability:
1. **Do not** open a public GitHub issue immediately. 
2. Use GitHub’s **“Report a vulnerability”** feature if enabled, or open a private contact channel with the maintainer(s) security@whispr.dev. 

Response target: within **72 hours**. 

## Security scope
This repository is a local CLI tool that generates `.mid` files and writes them to disk. [file:170]
Security concerns are primarily:
- Malicious / unexpected input handling (CLI parsing, path handling). [file:170]
- Denial-of-service style issues (e.g., huge values for `--bars`/`--ppqn` leading to memory/time blowups). [file:170]

## Known limitations
- The project does not attempt to provide sandboxing guarantees; it runs with the permissions of the user executing it. [file:170]

## Verification
Users are encouraged to inspect, audit, and rebuild from source before use in sensitive environments. [file:170]

​