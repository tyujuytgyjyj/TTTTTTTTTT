# CrashNG.drive — Dedicated Server & Console Guide

## 1. Starting the Headless Dedicated Server
Double-click `run_server.bat` or run:
```bash
cargo run --release -p crashdrive-server
```
The server starts in **headless mode**, listens on UDP port `30814` (configurable in `server.properties`), writes timestamped events to `server.log`, and opens an interactive **Minecraft-style terminal console** reading from `stdin`.

## 2. Server-Authoritative Architecture
- **Time & Weather**: Controlled strictly by the server (`initial-time`, `do-daylight-cycle`, `weather`). Clients cannot locally alter time or weather; `EnvSync` packets keep all clients locked to the server's clock and road surface conditions.
- **Map Transfer & Security Verification**: When a player joins (or when `map set <name>` is run), the server sends a `MapTransfer` packet containing the map JSON and its 64-bit FNV-1a hash (`fnv1a_64`). Both server and client enforce size limits (`<= 512 KB`), object count limits (`<= 2500`), path-traversal sanitization, and hash verification.
- **Authorized World Building & Persistence**: Players cannot open or apply changes in the World Editor (`F10`) online unless granted permission by the server owner via `op <player>` or `build allow <player>`. When an authorized builder modifies the map, the server validates the edit, saves it persistently to `server/maps/<map>.json` and `scenes/level.json`, and broadcasts the updated map immediately to all connected players.

## 3. Console Commands (`stdin`)
| Command | Description |
| :--- | :--- |
| `op <player>` / `deop <player>` | Grant or revoke full Operator (admin + time + world build) privileges |
| `build allow <player>` / `build deny <player>` | Grant or revoke World Editor (`F10`) building permission |
| `time set <day\|night\|sunset\|sunrise\|0..24>` | Set the authoritative server time for all players |
| `time cycle <on\|off>` | Enable or pause the 24-minute real-time day/night cycle |
| `weather set <clear\|wet\|rain\|fog>` | Set the authoritative weather and road friction coefficient |
| `map list` / `map set <name>` | List available maps or switch active map and sync to all clients |
| `kick <player> [reason]` | Disconnect a player immediately |
| `ban <player>` / `unban <player>` | Ban or unban a player by username (saved to `banned-players.txt`) |
| `whitelist <on\|off\|add\|remove\|list>` | Manage server whitelist (`whitelist.txt`) |
| `say <message>` | Broadcast a `[SERVER]` announcement to all players in chat |
| `list` | List connected players, IDs, IP addresses, and `op`/`build` flags |
| `save` | Force-save active world map and ACL files to disk |
| `stop` | Save world & configuration and shut down cleanly |
