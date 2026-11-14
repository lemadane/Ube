// Run against the local server: node examples/space-invaders/tests/http-smoke.mjs
import assert from 'node:assert/strict';

const origin = process.env.GAME_URL || 'http://127.0.0.1:8081';

class GameSession {
    cookie = '';

    async post(path) {
        const response = await fetch(origin + path, {
            method: 'POST', headers: { Cookie: this.cookie },
            signal: AbortSignal.timeout(5000)
        });
        assert.equal(response.status, 200);
        assert.match(response.headers.get('content-type'), /application\/json/);
        const cookie = response.headers.get('set-cookie');
        if (cookie) this.cookie = cookie.split(';')[0];
        return response.json();
    }
}

const page = await fetch(origin);
assert.equal(page.status, 200);
assert.match(page.headers.get('content-type'), /text\/html/);
assert.match(await page.text(), /<canvas width="384" height="288"/);

const first = new GameSession();
const second = new GameSession();
let state = await first.post('/game/reset');
assert.equal(state.invaders.length, 40);
assert.equal(state.lives, 3);
assert.equal(state.playerX, 192);
state = await first.post('/game/step?move=right&fire=1');
assert.equal(state.playerX, 196);
assert.equal(state.shotX, 196);
assert.equal(state.shotY, 240);
assert.equal(state.tick, 1);

// A new browser session starts independently, including when /step is its first request.
state = await second.post('/game/step');
assert.equal(state.playerX, 192);
assert.equal(state.tick, 0);
state = await first.post('/game/step?move=left');
assert.equal(state.playerX, 192);
assert.equal(state.tick, 2);
assert.equal(state.shotY, 232);
state = await first.post('/game/reset');
assert.equal(state.tick, 0);
assert.equal(state.score, 0);
assert.equal(state.shotY, -1);
assert.equal(state.invaders.filter(enemy => enemy.alive).length, 40);
console.log('PASS: HTML, JSON, movement, firing, session persistence/isolation, reset');
