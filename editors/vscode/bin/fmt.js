#!/usr/bin/env node
"use strict";

const fs = require("node:fs");
const path = require("node:path");
const { formatUbeDocument } = require("../formatter");

const help = `Usage: ube format [options] [file or directory ...]

Format Ube source using the same formatter as the VS Code extension.
With no paths, read stdin. By default, write formatted source to stdout.
Directories are searched recursively for .ube files (excluding hidden directories,
node_modules, target, and symbolic links).

  -w, --write   Update files in place
  -l, --list    Print paths whose formatting differs
  --check      Print differing paths and exit 1 if formatting is needed
  -h, --help   Show this help
  --           Treat remaining arguments as paths

Exit codes: 0 success, 1 formatting needed with --check, 2 usage or I/O error.
`;

function main(args) {
  let write = false;
  let list = false;
  let check = false;
  let positional = false;
  const paths = [];
  for (const arg of args) {
    if (positional) paths.push(arg);
    else if (arg === "--") positional = true;
    else if (arg === "-w" || arg === "--write") write = true;
    else if (arg === "-l" || arg === "--list") list = true;
    else if (arg === "--check") check = true;
    else if (arg === "-h" || arg === "--help") {
      fs.writeSync(1, help);
      return;
    } else if (arg.startsWith("-")) throw new Error(`unknown option: ${arg}`);
    else paths.push(arg);
  }
  if (write && check) throw new Error("--write and --check cannot be combined");
  if (write && paths.length === 0) throw new Error("--write requires a file or directory");

  let changed = false;
  let failed = false;
  function format(file) {
    const source = fs.readFileSync(file === null ? 0 : file, "utf8");
    const formatted = formatUbeDocument(source);
    if (source !== formatted) {
      changed = true;
      if (list || check) fs.writeSync(1, `${file === null ? "<stdin>" : file}\n`);
      if (write) fs.writeFileSync(file, formatted, "utf8");
    }
    if (!write && !list && !check) fs.writeSync(1, formatted);
  }

  const seen = new Set();
  const excluded = new Set(["node_modules", "target"]);
  function visit(file, explicit = false) {
    try {
      const stat = fs.lstatSync(file);
      if (stat.isSymbolicLink()) {
        if (explicit) throw new Error("symbolic link paths are not supported");
        return;
      }
      const absolute = path.resolve(file);
      if (seen.has(absolute)) return;
      seen.add(absolute);
      if (stat.isDirectory()) {
        for (const name of fs.readdirSync(file).sort()) {
          const child = path.join(file, name);
          const entry = fs.lstatSync(child);
          if (entry.isDirectory() && (name.startsWith(".") || excluded.has(name))) continue;
          if (entry.isDirectory() || name.endsWith(".ube")) visit(child);
        }
      } else if (stat.isFile()) format(file);
      else throw new Error("expected a regular file or directory");
    } catch (error) {
      failed = true;
      fs.writeSync(2, `ube format: ${file}: ${error.message}\n`);
    }
  }
  if (paths.length === 0) format(null);
  else for (const file of paths) visit(file, true);
  process.exitCode = failed ? 2 : check && changed ? 1 : 0;
}

try {
  main(process.argv.slice(2));
} catch (error) {
  fs.writeSync(2, `ube format: ${error.message}\n`);
  process.exitCode = 2;
}
