const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawn } = require("node:child_process");
const test = require("node:test");
const { formatUbeDocument } = require("../formatter");

const cli = path.join(__dirname, "../bin/fmt.js");
const source = "class App { start() { log('hello', 'world') } }";
const formatted = formatUbeDocument(source);
function run(args = [], options = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn("node", [cli, ...args], { cwd: options.cwd });
    let stdout = "";
    let stderr = "";
    const timer = setTimeout(() => { child.kill(); reject(new Error("CLI timed out")); }, 10000);
    child.stdout.setEncoding("utf8").on("data", chunk => { stdout += chunk; });
    child.stderr.setEncoding("utf8").on("data", chunk => { stderr += chunk; });
    child.on("error", error => { clearTimeout(timer); reject(error); });
    child.on("close", status => { clearTimeout(timer); resolve({ status, stdout, stderr }); });
    child.stdin.on("error", error => { if (error.code !== "EPIPE") reject(error); });
    child.stdin.end(options.input || "");
  });
}
function fixture(t) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "ube-fmt-"));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  return dir;
}

test("stdin output matches the editor formatter; check mode signals differences", async () => {
  const output = (await run([], { input: source }));
  assert.equal(output.status, 0);
  assert.equal(output.stdout, formatted);
  assert.equal((await run(["--check"], { input: source })).status, 1);
  assert.equal((await run(["--check"], { input: formatted })).status, 0);
});

test("file output and list/check do not write; write mode updates only once", async (t) => {
  const cwd = fixture(t);
  const file = path.join(cwd, "App.ube");
  fs.writeFileSync(file, source);
  assert.equal((await run(["App.ube"], { cwd })).stdout, formatted);
  assert.equal((await run(["-l", "App.ube"], { cwd })).stdout, "App.ube\n");
  assert.equal((await run(["--check", "App.ube"], { cwd })).status, 1);
  assert.equal(fs.readFileSync(file, "utf8"), source);
  const write = (await run(["-w", "-l", "App.ube"], { cwd }));
  assert.equal(write.status, 0);
  assert.equal(write.stdout, "App.ube\n");
  assert.equal(fs.readFileSync(file, "utf8"), formatted);
  const before = fs.statSync(file).mtimeMs;
  assert.equal((await run(["-w", "App.ube"], { cwd })).status, 0);
  assert.equal(fs.statSync(file).mtimeMs, before);
  assert.equal((await run(["--check", "App.ube"], { cwd })).status, 0);
});

test("recursive traversal skips generated directories, non-Ube files and symlinks", async (t) => {
  const cwd = fixture(t);
  for (const dir of ["src", ".build", ".git", "target", "node_modules"]) {
    fs.mkdirSync(path.join(cwd, dir));
    fs.writeFileSync(path.join(cwd, dir, "App.ube"), source);
  }
  fs.writeFileSync(path.join(cwd, "notes.txt"), source);
  fs.symlinkSync(path.join(cwd, "src/App.ube"), path.join(cwd, "link.ube"));
  const result = (await run(["-w", "-l", ".", "src"], { cwd }));
  assert.equal(result.status, 0);
  assert.equal(result.stdout, `src${path.sep}App.ube\n`);
  for (const file of [".build/App.ube", ".git/App.ube", "target/App.ube", "node_modules/App.ube", "notes.txt"]) {
    assert.equal(fs.readFileSync(path.join(cwd, file), "utf8"), source);
  }
  assert.equal((await run(["-w", "link.ube"], { cwd })).status, 2);
});

test("usage and I/O errors return 2, while remaining files are processed", async (t) => {
  const cwd = fixture(t);
  fs.writeFileSync(path.join(cwd, "-file.ube"), source);
  for (const args of [["--unknown"], ["-w"], ["-w", "--check", "-file.ube"]]) {
    assert.equal((await run(args, { cwd, input: source })).status, 2);
  }
  const result = (await run(["--check", "missing.ube", "--", "-file.ube"], { cwd }));
  assert.equal(result.status, 2);
  assert.match(result.stderr, /missing\.ube/);
  assert.equal(result.stdout, "-file.ube\n");
  assert.equal((await run(["--help"], { cwd })).status, 0);
});
