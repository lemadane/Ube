# Space Invaders in Ube

A playable, single-player arcade game with a 384 × 288 pixel canvas, five rows of invaders, destructible shields, three lives, increasing waves, and a local personal-best score. No packages, CDN assets, or internet connection are needed to play.

## Run

From the repository root:

```sh
cargo build --workspace
./target/debug/ube-compiler run examples/space-invaders/App.ube
```

Open **http://127.0.0.1:8081** and select **Start flight**. Stop the server with Ctrl+C. The port must be available.

- Move with **← / →** or **A / D**.
- Hold **Space** to fire paired shots from the wing cannons. Each shot has independent collisions; the next pair fires once both shots clear and the cooldown expires.
- Press **L** or hold **LASER** for a piercing beam from the center tip. It passes through shields, hits every enemy in its path, and follows the ship for one second. It starts ready and recharges ten seconds after activation; holding the control fires again when ready. The indicator shows its status. Timing uses 30 simulation ticks per second and freezes while paused; slow requests slow these timers too. Clearing a wave stops the beam but keeps its cooldown.
- Press **P** or the pause button to pause/resume.
- Touch devices have left, right, and fire buttons. Hold two buttons to move and shoot.
- Losing focus pauses the game. Clear every invader to begin a faster wave. Losing all lives or allowing the formation to reach your ship ends the game.
- Enemies drop volleys of up to three bombs from distinct occupied columns every 24 ticks, with at most six bombs in flight. The lowest surviving invader in each selected column fires.
- Shields absorb four hits, including your own shots, and regenerate between waves.

## Architecture

The gameplay simulation is written in Ube. The browser needs JavaScript to access Canvas and input events; that adapter renders server snapshots and sends controls, without implementing collision or scoring rules.

| Layer | Files | Responsibility |
| --- | --- | --- |
| Domain | `GameState.ube`, `Invader.ube`, `Shield.ube`, `Bomb.ube`, `Shot.ube` | Explicit, serializable game state |
| Domain services | `GameFactory.ube`, `GameRules.ube` | Initial formation, fixed ticks, movement, projectiles, collisions, scoring, waves |
| Persistence adapter | `GameCodec.ube` | Reconstruct typed domain objects from server-owned session JSON |
| HTTP adapter | `GameController.ube`, `App.ube` | Read controls, load/save session state, return snapshots, register routes |
| Presentation | `GamePage.ube` | Typed USSR page, responsive layout, controls and HUD |
| Browser adapters | `PixelRenderer.ube`, `BrowserClient.ube` | Static script components for pixel sprites, keyboard/pointer input, transport, pause and local best score |

The dependency direction is **HTTP → domain**. Domain code knows nothing about requests, sessions, HTML, or Canvas. Small named methods separate each rule, and all entity mutations use Ube accessors.

```text
Input → POST /game/step → session snapshot → GameRules.advance
                                          ↓
Canvas ← JSON snapshot ← save updated GameState
```

The browser requests up to 30 simulation ticks per second and renders with `requestAnimationFrame`. Requests never overlap in the supplied client; delayed requests slow the simulation instead of producing catch-up bursts. Integer coordinates, disabled canvas smoothing, and CSS pixel scaling keep sprites sharp.

## Ube concurrency

Each reset or step starts an `Async.start` task and awaits its response with
`.await()`. `App` seeds a bounded `Channel<int>(1)` with one ownership token.
The task receives that token before reading session state, runs the simulation,
saves the snapshot, and returns the token in `finally`. Reset and step share
the same gate; errors also release it. The missing-session path resets directly
while holding the token rather than trying to acquire it again.

All sessions share one gate, so only one state update runs at a time. The current
HTTP runtime already dispatches route callbacks serially; awaiting the task keeps
the request alive and blocks that callback until completion. Ordinary method calls
use Ube's blocking task pool. This demonstrates task execution and channel
coordination without increasing HTTP throughput or adding a background simulation
clock. See [Ube concurrency](../../docs/concurrency.md).

## Endpoints and state

- `GET /`: the self-contained HTML page and static browser adapters.
- `POST /game/reset`: start a new game in the current session.
- `POST /game/step?move=left&fire=1`: advance one tick. `move` accepts `left`, `right`, or idle; `fire=1` shoots. Other values are ignored.

Game state stays on the server. The browser never supplies positions, scores, or enemy health. A missing/expired session starts a new game. Ube sessions expire after 30 minutes and disappear when the server stops. The personal best is stored separately in browser local storage, when available.

This is a **local arcade example**, not a competitive multiplayer service. Use one active game tab per browser session: tabs sharing cookies also share game state and each tab can advance or reset it. The channel gate serializes game updates within this server process. Its request-driven clock favors readable Ube rules over high-latency remote play. There are no WebSockets, client-side prediction, accounts, audio, or online leaderboards.

## Verify

```sh
# Syntax and type checking
./target/debug/ube-compiler check examples/space-invaders/App.ube

# Eleven native Ube tests; no server required
./target/debug/ube-compiler test examples/space-invaders/GameTests.ube

# With the game server running (Node.js 18+)
node examples/space-invaders/tests/http-smoke.mjs

# Compiler regressions, including the canvas template support
cargo test -p ube-compiler --test html_e2e
```

The Ube tests cover movement limits, dual wing firing, independent projectile collisions, shield damage, invulnerability, lives, game over, formation descent, enemy bomb volleys, the six-bomb limit, simultaneous collisions, slot reuse, piercing laser damage/duration/cooldown, new waves, and exact state serialization. The HTTP smoke test checks the page, JSON media type, input, independent sessions, persistence, restart, and preservation of every tick across overlapping client requests, bomb persistence/reset, and laser activation/cooldown/reset.

Browser verification: start a flight, hold movement and fire, pause/resume, restart after losing, and try the touch buttons at a narrow viewport. A server interruption produces a reconnect prompt instead of leaving the game apparently running.
