const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { formatUbeDocument } = require("../formatter");

test("splits multiline query chains and keeps simple trailing calls compact", () => {
  const source = "final query = users.query().where(\n              'active',\n              '=',\n              false).orderBy(\n              'id',\n              'asc').limit(\n              10)";
  const expected = "final query = users.query()\n  .where(\n    'active',\n    '=',\n    false)\n  .orderBy(\n    'id',\n    'asc')\n  .limit(10)\n";
  for (const input of [source, "final query = users.query().where('active', '=', false).orderBy('id', 'asc').limit(10)", expected]) {
    assert.equal(formatUbeDocument(input), expected);
  }
  const nested = `outer(\n  ${expected.trimEnd().replace('final query = ', '').replaceAll('\n', '\n  ')})\n`;
  assert.equal(formatUbeDocument(nested), nested);
  const block = `class App {\n    start() {\n${expected.trimEnd().split('\n').map(line => '        ' + line).join('\n')}\n    }\n}\n`;
  assert.equal(formatUbeDocument(block), block);
});

test("splits short call chains consistently", () => {
  for (const [source, expected] of [
    ["final selected = active.select('name').select('score')", "final selected = active.select('name')\n  .select('score')\n"],
    ["users.query().all()", "users.query()\n  .all()\n"],
  ]) {
    assert.equal(formatUbeDocument(source), expected);
    assert.equal(formatUbeDocument(expected), expected);
  }
});

test("preserves chain punctuation in strings and comments", () => {
  for (const source of ["log('users.query().where(a, b)')\n", "// users.query().where(a, b)\n"]) {
    assert.equal(formatUbeDocument(source), source);
  }
});

test("matches the edited cache counter App.ube style", () => {
  const source = fs.readFileSync(
    path.join(__dirname, "../../../examples/cache/counter/App.ube"),
    "utf8"
  );
  assert.equal(formatUbeDocument(source), source);
});

test("formats the starter app using the initial Ube formatting rules", () => {
  const source = `class App {
start() {
final server = Server()
server.get('/', request -> Response.ok('Hello from my Ube server!'));
server.start(8080)
}
}`;
  const expected = `class App {
    start() {
        final server = Server()
        server.get(
          '/',
          request -> Response.ok('Hello from my Ube server!'));
        server.start(8080)
    }
}
`;

  const formatted = formatUbeDocument(source);
  assert.equal(formatted, expected);
  assert.equal(formatUbeDocument(formatted), formatted);
});

test("puts a single null-coalescing argument on its own line", () => {
  const source = "Console.printLine(cache.get('a') ?? 'missing')";
  const expected = "Console.printLine(\n  cache.get('a')\n    ?? 'missing')\n";
  assert.equal(formatUbeDocument(source), expected);
  assert.equal(formatUbeDocument(expected), expected);
});

test("indents null-coalescing continuations in assignments and fallback calls", () => {
  const cases = [
    ["final address = Env.get('UBE_CACHE_ADDRESS') ?? '127.0.0.1:6380'",
      "final address = Env.get('UBE_CACHE_ADDRESS')\n  ?? '127.0.0.1:6380'\n"],
    ["value ??= fallback(first, second)",
      "value\n  ??= fallback(\n    first,\n    second)\n"],
    ["final value = first ?? second ?? third",
      "final value = first\n  ?? second\n  ?? third\n"],
    ["class App {\nstart() {\nfinal value = first // fallback\n ?? second\n}\n}",
      "class App {\n    start() {\n        final value = first // fallback\n          ?? second\n    }\n}\n"],
  ];
  for (const [source, expected] of cases) {
    assert.equal(formatUbeDocument(source), expected);
    assert.equal(formatUbeDocument(expected), expected);
  }
});

test("ignores null-coalescing punctuation in literals and comments", () => {
  for (const source of [
    "Console.printLine('??')\n",
    "Console.printLine(`??`)\n",
    "Console.printLine(value /* ?? */)\n",
  ]) {
    assert.equal(formatUbeDocument(source), source);
  }
});

test("formats nested null-coalescing arguments consistently", () => {
  const source = "Console.printLine(resolve(cache.get('a') ?? 'missing'))";
  const expected = "Console.printLine(\n  resolve(\n    cache.get('a')\n      ?? 'missing'))\n";
  assert.equal(formatUbeDocument(source), expected);
  assert.equal(formatUbeDocument(expected), expected);
});

test("formats a method call with nested argument calls", () => {
  const source = `class App {
start() {
service.configure(first(), second('a,b'))
}
}`;
  const expected = `class App {
    start() {
        service.configure(
          first(),
          second('a,b'))
    }
}
`;

  assert.equal(formatUbeDocument(source), expected);
});

test("formats nested multiline calls inside nested blocks", () => {
  const source = `class App {
start() {
if (ready) {
outer(first(one, two), second)
}
}
}`;
  const expected = `class App {
    start() {
        if (ready) {
            outer(
              first(
                one,
                two),
              second)
        }
    }
}
`;

  const formatted = formatUbeDocument(source);
  assert.equal(formatted, expected);
  assert.equal(formatUbeDocument(formatted), formatted);
});

test("formats the commented src/App.ube example without changing its comments", () => {
  const source = `class App { // indentions should be 2 spaces in length only
    start() { // method declaration should be 2x indented from the class declaration
        final server = Server() // statement1 should be 2x indented from the method declaration
        // statement2 should be 2x indented from the method declaration
        // method call with 2 or more arguments should be lowered down per argument and 1x indented from its parent method call
        server.get(
        // argument1 should be 1x indented from the method call
          '/',
          request -> Response.ok('Hello from my Ube server!')) // argument2 should be 1x indented from the method call
        server.start(8080) // statement3 should be 2x indented from the method declaration
  }
}`;
  const expected = `class App { // indentions should be 2 spaces in length only
    start() { // method declaration should be 2x indented from the class declaration
        final server = Server() // statement1 should be 2x indented from the method declaration
        // statement2 should be 2x indented from the method declaration
        // method call with 2 or more arguments should be lowered down per argument and 1x indented from its parent method call
        server.get(
          // argument1 should be 1x indented from the method call
          '/',
          request -> Response.ok('Hello from my Ube server!')) // argument2 should be 1x indented from the method call
        server.start(8080) // statement3 should be 2x indented from the method declaration
    }
}
`;

  const formatted = formatUbeDocument(source);
  assert.equal(formatted, expected);
  assert.equal(formatUbeDocument(formatted), formatted);
});

test("preserves braces and commas inside strings and comments", () => {
  const source = `class App {
start() {
log('},')
// } , remains a comment
}
}`;
  const expected = `class App {
    start() {
        log('},')
        // } , remains a comment
    }
}
`;

  assert.equal(formatUbeDocument(source), expected);
});

test("preserves multiline string contents", () => {
  const source = `class App {
start() {
log(\`first line
  {second, line}
third line\`)
}
}`;
  const expected = `class App {
    start() {
        log(\`first line
  {second, line}
third line\`)
    }
}
`;

  assert.equal(formatUbeDocument(source), expected);
});

test("indents comments within a multiline argument list", () => {
  const source = `class App {
start() {
server.get(
// route with an "unfinished quote
"/",
request -> Response.ok("Hello")
        )
}
}`;
  const expected = `class App {
    start() {
        server.get(
          // route with an "unfinished quote
          "/",
          request -> Response.ok("Hello"))
    }
}
`;

  assert.equal(formatUbeDocument(source), expected);
});

test("keeps multiline closing parentheses after the final argument", () => {
  const source = `class App {
    start() {
        server.get(
          '/',
          request -> Response.ok( // lowering down the argument of a 1 argument method call is optional
            'Hello from my Ube server!'))
    }
}`;
  const expected = `class App {
    start() {
        server.get(
          '/',
          request -> Response.ok( // lowering down the argument of a 1 argument method call is optional
            'Hello from my Ube server!'))
    }
}
`;

  const formatted = formatUbeDocument(source);
  assert.equal(formatted, expected);
  assert.equal(formatUbeDocument(formatted), formatted);
});

test("matches the revised multiline-call example and preserves its comments", () => {
  const source = `class App {
    start() { // method declaration should be 2x indented from the class declaration
        final server = Server() // statement1 should be 2x indented from the method declaration
        // statement2 should be 2x indented from the method declaration
        // method call with 2 or more arguments should be lowered down per argument and 1x indented from its parent method call
        server.get(
          // arguments should be 1x indented from the method call and should be on a new line
          '/',
          request -> Response.ok( // lowering down the argument of a 1 argument method call is optional
            'Hello from my Ube server!')) // closing parentheses stay with the final argument
        server.start(8080) // statement3 should be 2x indented from the method declaration
    }
}`;
  const expected = `class App {
    start() { // method declaration should be 2x indented from the class declaration
        final server = Server() // statement1 should be 2x indented from the method declaration
        // statement2 should be 2x indented from the method declaration
        // method call with 2 or more arguments should be lowered down per argument and 1x indented from its parent method call
        server.get(
          // arguments should be 1x indented from the method call and should be on a new line
          '/',
          request -> Response.ok( // lowering down the argument of a 1 argument method call is optional
            'Hello from my Ube server!')) // closing parentheses stay with the final argument
        server.start(8080) // statement3 should be 2x indented from the method declaration
    }
}
`;

  const formatted = formatUbeDocument(source);
  assert.equal(formatted, expected);
  assert.equal(formatUbeDocument(formatted), formatted);
});

test("is idempotent", () => {
  const source = `class App {
    start() {
        server.get('/', request -> Response.ok('Hello'));
    }
}
`;

  const formatted = formatUbeDocument(source);
  assert.equal(formatUbeDocument(formatted), formatted);
});

test("ignores punctuation inside single-quoted arguments", () => {
  const source = "class App {\nstart() {\nlog('hello { (, world')\nlog('done')\n}\n}\n";
  const expected = "class App {\n    start() {\n        log('hello { (, world')\n        log('done')\n    }\n}\n";
  assert.equal(formatUbeDocument(source), expected);
  assert.equal(formatUbeDocument(expected), expected);
});

test("tracks braces after a text block and preserves its contents", () => {
  const source = 'class App {\nstart() {\nlog("\n    hello { world\n    ")}\nnext() {\nlog("x")\n}\n}\n';
  const expected = 'class App {\n    start() {\n        log("\n    hello { world\n    ")\n    }\n    next() {\n        log("x")\n    }\n}\n';
  assert.equal(formatUbeDocument(source), expected);
  assert.equal(formatUbeDocument(expected), expected);
});

test("raw text blocks close after a backslash and preserve JSON content", () => {
  const source = [
    "class App {",
    "start() {",
    "log(`C:\\tmp\\`, '{\"ok\":true}')",
    "}",
    "}"
  ].join("\n");
  const expected = [
    "class App {",
    "    start() {",
    "        log(",
    "          `C:\\tmp\\`,",
    "          '{\"ok\":true}')",
    "    }",
    "}",
    ""
  ].join("\n");
  assert.equal(formatUbeDocument(source), expected);
  assert.equal(formatUbeDocument(expected), expected);
});

test("formats declaration parameters while preserving nested Func signatures", () => {
  const source = `class User(int id, string name) {
bool validate(Func(int, string) -> bool operation, int value) {
return operation(value, name)
}
}`;
  const expected = `class User(
  int id,
  string name) {
    bool validate(
      (Func(int, string) -> bool) operation,
      int value) {
        return operation(
          value,
          name)
    }
}
`;

  const formatted = formatUbeDocument(source);
  assert.equal(formatted, expected);
  assert.equal(formatUbeDocument(formatted), formatted);
});

test("preserves grouped Func locals and closure parameter lists", () => {
  const source = `class App {
start() {
(Func(int, string) -> bool) operation = (int id, string name) -> id > 0
final pending = Async.start(() -> operation(1, 'one'))
final values = Async.startAll([() -> 1, () -> 2])
Console.printLine(pending.await(), values.await().len())
}
}`;
  const expected = `class App {
    start() {
        (Func(int, string) -> bool) operation = (int id, string name) -> id > 0
        final pending = Async.start(
          () -> operation(
            1,
            'one'))
        final values = Async.startAll([() -> 1, () -> 2])
        Console.printLine(
          pending.await(),
          values.await()
            .len())
    }
}
`;

  const formatted = formatUbeDocument(source);
  assert.equal(formatted, expected);
  assert.equal(formatUbeDocument(formatted), formatted);
});

test("does not treat control-flow conditions as calls", () => {
  const source = `class App {
start() {
if (ready(1, 2) and allowed(3, 4)) {
log('yes')
}
}
}`;
  const expected = `class App {
    start() {
        if (ready(
          1,
          2)
          and allowed(
          3,
          4)) {
            log('yes')
        }
    }
}
`;

  const formatted = formatUbeDocument(source);
  assert.equal(formatted, expected);
  assert.equal(formatUbeDocument(formatted), formatted);
});

test("formats abstract trait method parameters", () => {
  const source = `trait Service {
string load(int id, string name)
notify(int id, string name)
}`;
  const expected = `trait Service {
    string load(
      int id,
      string name)
    notify(
      int id,
      string name)
}
`;

  const formatted = formatUbeDocument(source);
  assert.equal(formatted, expected);
  assert.equal(formatUbeDocument(formatted), formatted);
});

test("wraps calls exceeding 80 columns including structural indentation", () => {
  for (const width of [79, 80, 81]) {
    const argument = `'${"x".repeat(width - 29)}'`;
    const line = `        Console.printLine(${argument})`;
    assert.equal(line.length, width);
    const source = `class App {\n    start() {\n${line}\n    }\n}\n`;
    const expected = width > 80
      ? source.replace(line, `        Console.printLine(\n          ${argument})`)
      : source;
    assert.equal(formatUbeDocument(source), expected);
    assert.equal(formatUbeDocument(expected), expected);
    assert.equal(formatUbeDocument(source.replace(/^ +/gm, "")), expected);
  }
});

test("wraps the persistent usage message at 80 columns", () => {
  const source = "class App {\n    start() {\n        if (args.len() != 2) {\n            Console.printLine('Usage: persistent <database-path> <set|get|delete>')\n        }\n    }\n}\n";
  const expected = source.replace("Console.printLine('", "Console.printLine(\n              '");
  assert.equal(formatUbeDocument(source), expected);
  assert.equal(formatUbeDocument(expected), expected);
});

test("keeps the persistent cache example formatted", () => {
  const source = fs.readFileSync(
    path.join(__dirname, "../../../examples/cache/persistent/App.ube"),
    "utf8"
  );
  assert.equal(formatUbeDocument(source), source);
});

test("moves single nested calls onto a line at each nesting level", () => {
  for (const [source, expected] of [
    ["Console.printLine(cache.delete('greeting'))", "Console.printLine(\n  cache.delete('greeting'))\n"],
    ["outer(middle(inner()))", "outer(\n  middle(\n    inner()))\n"],
    ["Console.printLine(cache.get('??'))", "Console.printLine(\n  cache.get('??'))\n"],
    ["outer('inner()')", "outer('inner()')\n"],
    ["outer(value /* inner() */)", "outer(value /* inner() */)\n"],
  ]) {
    assert.equal(formatUbeDocument(source), expected);
    assert.equal(formatUbeDocument(expected), expected);
  }
});

test("groups function return types, nested signatures, and generic results idempotently", () => {
  const source = `Func(int) -> int makeAdder(int amount) {
return (int value) -> value + amount
}
Func(Func(int) -> int) -> Func() -> Async<int?> factory()
// Func(int) -> int comment
final text = 'Func(int) -> int'
`;
  const expected = `(Func(int) -> int) makeAdder(int amount) {
    return (int value) -> value + amount
}
(Func((Func(int) -> int)) -> (Func() -> Async<int?>)) factory()
// Func(int) -> int comment
final text = 'Func(int) -> int'
`;
  assert.equal(formatUbeDocument(source), expected);
  assert.equal(formatUbeDocument(expected), expected);
});

test("groups single function parameters and preserves grouped static return types", () => {
  for (const [source, expected] of [
    ['int apply(Func(int) -> int operation)', 'int apply((Func(int) -> int) operation)'],
    ['static Func(int) -> int factory()', 'static (Func(int) -> int) factory()'],
    ['Func() -> int callback = () -> 1', '(Func() -> int) callback = () -> 1'],
    ['Func(int) -> broken<', 'Func(int) -> broken<'],
  ]) {
    assert.equal(formatUbeDocument(source), expected + '\n');
    assert.equal(formatUbeDocument(expected), expected + '\n');
  }
});

test("splits if conditions and expands all branches", () => {
  const source = "if (value != 'true' and value != 'false') { throw ApiError(400, '${field} must be a boolean.') }";
  const expected = `if (value != 'true'
  and value != 'false') {
    throw ApiError(
      400,
      '\${field} must be a boolean.')
}
`;
  assert.equal(formatUbeDocument(source), expected);
  assert.equal(formatUbeDocument(expected), expected);
  const branches = `if (a
  and (b
  or c)) {
    yes()
} else if (d
  or e) {
    no()
} else {
    other()
}
`;
  assert.equal(formatUbeDocument("if (a and (b or c)) { yes() } else if (d or e) { no() } else { other() }"), branches);
  assert.equal(formatUbeDocument(branches), branches);
});

test("preserves condition comments, literals, calls and nested indentation", () => {
  for (const source of [
    "if (a // first condition\n and b) { go() }",
    "if (check(a and b, c) or ready()) { go() }",
    "if (a /* and ignored */ and b) { go() }",
    "class App {\n    start() {\n        if (a and b) { go() }\n    }\n}",
    "server.get('/', (Request request) -> { if (a and b) { return Response.ok('yes') } else { return Response.ok('no') } })",
  ]) {
    const result = formatUbeDocument(source);
    assert.equal(formatUbeDocument(result), result);
  }
  const source = "// if (a and b) { go() }\nlog('if (a and b) { go() }')\n";
  assert.equal(formatUbeDocument(source), source);
  assert.equal(formatUbeDocument("if (value == 'and or') { go() }"),
    "if (value == 'and or') {\n    go()\n}\n");
});

test("expands finally cleanup blocks and remains idempotent", () => {
  for (const [body, expectedBody] of [
    ['users.close() db.close()', '    users.close()\n    db.close()'],
    ['users.close()', '    users.close()'],
    ['', ''],
    ['users.close().flush() db.close()', '    users.close()\n      .flush()\n    db.close()'],
    ["log('finally { ignored() }') db.close()", "    log('finally { ignored() }')\n    db.close()"],
    ['users.close() // cleanup\ndb.close()', '    users.close() // cleanup\n    db.close()'],
  ]) {
    const source = `try {\n    work()\n} finally { ${body} }`;
    const expected = `try {\n    work()\n} finally {\n${expectedBody ? expectedBody + '\n' : ''}}\n`;
    assert.equal(formatUbeDocument(source), expected);
    assert.equal(formatUbeDocument(expected), expected);
  }
});

test("expands try calls and return statements without changing nested arguments", () => {
  const source = `string quote(string value) {
    final params = SQLParams.create()
    try { params.addString(value) return this.scalar(
          'SELECT json_quote(?)',
          params) }
    finally {
        params.close()
    }
}`;
  const expected = `string quote(string value) {
    final params = SQLParams.create()
    try {
        params.addString(value)
        return this.scalar(
          'SELECT json_quote(?)',
          params)
    }
    finally {
        params.close()
    }
}
`;
  assert.equal(formatUbeDocument(source), expected);
  assert.equal(formatUbeDocument(expected), expected);
  const untouched = "// try { first() return second() }\nlog('try { first() return second() }')\n";
  assert.equal(formatUbeDocument(untouched), untouched);
});

test("does not expand finally text inside comments or literals", () => {
  const source = "// finally { users.close() db.close() }\nlog('finally { users.close() db.close() }')\n";
  assert.equal(formatUbeDocument(source), source);
});

test("aligns Java-style lambda closing braces with their parameter lines", () => {
  const source = `class App {
    start() {
        server.post(
          '/rotate',
          (Request request) -> {
              request.session()
                .regenerate()
              return Response.ok('Session ID replaced')
              })
    }
}
`;
  const expected = source.replace("              })", "          })");
  assert.equal(formatUbeDocument(source), expected);
  assert.equal(formatUbeDocument(expected), expected);
});

test("dedents nested blocks inside a lambda argument without changing literals", () => {
  const source = `server.get(
  '/',
  (Request request) -> {
      if (request.hasBearerToken()) {
          // A brace in a string does not close the block.
          return Response.ok('}')
          }
      return Response.ok('guest')
      })
`;
  const expected = source
    .replace("          }\n", "      }\n")
    .replace("      })\n", "  })\n");
  assert.equal(formatUbeDocument(source), expected);
  assert.equal(formatUbeDocument(expected), expected);
});


test("separates imports and expands the Profile method", () => {
  const source = "import UserLookup\nclass Profile(UserLookup users) {\n    string title(int id) { return this._users.name(id) }\n}\n";
  const expected = fs.readFileSync(path.join(__dirname, "../../../examples/testing/profile_test_types/Profile.ube"), "utf8");
  assert.equal(formatUbeDocument(source), expected);
  assert.equal(formatUbeDocument(expected), expected);
});

test("keeps import groups together and ignores imports inside literals and comments", () => {
  const source = "import One\nimport Two // dependency\ntrait Lookup {\n    string name() { return 'import Three' }\n}\n";
  const expected = "import One\nimport Two // dependency\n\ntrait Lookup {\n    string name() {\n        return 'import Three'\n    }\n}\n";
  assert.equal(formatUbeDocument(source), expected);
  assert.equal(formatUbeDocument(expected), expected);
  const literal = "final text = `\nimport One\nclass Example {}\n`\n// import Two\n";
  assert.equal(formatUbeDocument(literal), literal);
});
