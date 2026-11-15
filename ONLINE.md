# CrashDrive Online

## Test multiplayer

### Server
```bash
./crashdrive-server
# or: cargo run -p crashdrive-server --release
```
Port: **30814**

### Client (two windows)
```bash
cd client
cargo run --release
```
1. Start race
2. Press **F5** to connect
3. Status bottom-left: `Online: ... | remotes=N`
4. Remote players show as **real car models** (same as local)

### Notes
- Soft-body physics stays local (BeamMP-style)
- Remote cars are rigid visual copies posed from network state
- F5 toggles connect/disconnect
