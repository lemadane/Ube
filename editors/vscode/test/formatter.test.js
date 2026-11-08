const assert = require("node:assert/strict");
const test = require("node:test");
const { formatUbeDocument } = require("../formatter");

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
          request -> Response.ok('Hello from my Ube server!')
        );
        server.start(8080)
    }
}
`;

  const formatted = formatUbeDocument(source);
  assert.equal(formatted, expected);
  assert.equal(formatUbeDocument(formatted), formatted);
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
          second('a,b')
        )
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
                two
              ),
              second
            )
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
          request -> Response.ok('Hello from my Ube server!')
        ) // argument2 should be 1x indented from the method call
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
          request -> Response.ok("Hello")
        )
    }
}
`;

  assert.equal(formatUbeDocument(source), expected);
});

test("keeps explicitly multiline single-argument calls and indents both closing calls", () => {
  const source = `class App {
    start() {
        server.get(
          '/',
          request -> Response.ok( // lowering down the argument of a 1 argument method call is optional
            'Hello from my Ube server!'
        )
        )
    }
}`;
  const expected = `class App {
    start() {
        server.get(
          '/',
          request -> Response.ok( // lowering down the argument of a 1 argument method call is optional
            'Hello from my Ube server!'
          )
        )
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
            'Hello from my Ube server!' // this is a forced lowered down argument, it should be 1x indented from the method call
        ) // the closing parenthesis of a single argument (that is forced lowered down method call should be on a new line and 1x indented from the method call
        ) // the closing parenthesis of a multiple argument method call should be on a new line and 1x indented from the method call
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
            'Hello from my Ube server!' // this is a forced lowered down argument, it should be 1x indented from the method call
          ) // the closing parenthesis of a single argument (that is forced lowered down method call should be on a new line and 1x indented from the method call
        ) // the closing parenthesis of a multiple argument method call should be on a new line and 1x indented from the method call
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
  const expected = 'class App {\n    start() {\n        log("\n    hello { world\n    ")}\n    next() {\n        log("x")\n    }\n}\n';
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
    "          '{\"ok\":true}'",
    "        )",
    "    }",
    "}",
    ""
  ].join("\n");
  assert.equal(formatUbeDocument(source), expected);
  assert.equal(formatUbeDocument(expected), expected);
});
