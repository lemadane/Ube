const assert = require("node:assert/strict");
const test = require("node:test");
const { formatUbeDocument } = require("../formatter");

test("formats the starter app using the initial Ube formatting rules", () => {
  const source = `class Application {
main() {
final server = server()
server.get("/", request -> response.ok("Hello from my Ube server!"));
server.start(8080)
}
}`;
  const expected = `class Application {
    main() {
        final server = server()
        server.get(
          "/",
          request -> response.ok("Hello from my Ube server!")
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
  const source = `class Application {
main() {
service.configure(first(), second("a,b"))
}
}`;
  const expected = `class Application {
    main() {
        service.configure(
          first(),
          second("a,b")
        )
    }
}
`;

  assert.equal(formatUbeDocument(source), expected);
});

test("formats nested multiline calls inside nested blocks", () => {
  const source = `class Application {
main() {
if (ready) {
outer(first(one, two), second)
}
}
}`;
  const expected = `class Application {
    main() {
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

test("formats the commented src/main.ube example without changing its comments", () => {
  const source = `class Application { // indentions should be 2 spaces in length only
    main() { // method declaration should be 2x indented from the class declaration
        final server = server() // statement1 should be 2x indented from the method declaration
        // statement2 should be 2x indented from the method declaration
        // method call with 2 or more arguments should be lowered down per argument and 1x indented from its parent method call
        server.get(
        // argument1 should be 1x indented from the method call
          "/",
          request -> response.ok("Hello from my Ube server!")) // argument2 should be 1x indented from the method call
        server.start(8080) // statement3 should be 2x indented from the method declaration
  }
}`;
  const expected = `class Application { // indentions should be 2 spaces in length only
    main() { // method declaration should be 2x indented from the class declaration
        final server = server() // statement1 should be 2x indented from the method declaration
        // statement2 should be 2x indented from the method declaration
        // method call with 2 or more arguments should be lowered down per argument and 1x indented from its parent method call
        server.get(
          // argument1 should be 1x indented from the method call
          "/",
          request -> response.ok("Hello from my Ube server!")
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
  const source = `class Application {
main() {
print("},")
// } , remains a comment
}
}`;
  const expected = `class Application {
    main() {
        print("},")
        // } , remains a comment
    }
}
`;

  assert.equal(formatUbeDocument(source), expected);
});

test("preserves multiline string contents", () => {
  const source = `class Application {
main() {
print(\`first line
  {second, line}
third line\`)
}
}`;
  const expected = `class Application {
    main() {
        print(\`first line
  {second, line}
third line\`)
    }
}
`;

  assert.equal(formatUbeDocument(source), expected);
});

test("indents comments within a multiline argument list", () => {
  const source = `class Application {
main() {
server.get(
// route with an "unfinished quote
"/",
request -> response.ok("Hello")
        )
}
}`;
  const expected = `class Application {
    main() {
        server.get(
          // route with an "unfinished quote
          "/",
          request -> response.ok("Hello")
        )
    }
}
`;

  assert.equal(formatUbeDocument(source), expected);
});

test("keeps explicitly multiline single-argument calls and indents both closing calls", () => {
  const source = `class Application {
    main() {
        server.get(
          "/",
          request -> response.ok( // lowering down the argument of a 1 argument method call is optional
            "Hello from my Ube server!"
        )
        )
    }
}`;
  const expected = `class Application {
    main() {
        server.get(
          "/",
          request -> response.ok( // lowering down the argument of a 1 argument method call is optional
            "Hello from my Ube server!"
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
  const source = `class Application {
    main() { // method declaration should be 2x indented from the class declaration
        final server = server() // statement1 should be 2x indented from the method declaration
        // statement2 should be 2x indented from the method declaration
        // method call with 2 or more arguments should be lowered down per argument and 1x indented from its parent method call
        server.get(
          // arguments should be 1x indented from the method call and should be on a new line
          "/",
          request -> response.ok( // lowering down the argument of a 1 argument method call is optional
            "Hello from my Ube server!" // this is a forced lowered down argument, it should be 1x indented from the method call
        ) // the closing parenthesis of a single argument (that is forced lowered down method call should be on a new line and 1x indented from the method call
        ) // the closing parenthesis of a multiple argument method call should be on a new line and 1x indented from the method call
        server.start(8080) // statement3 should be 2x indented from the method declaration
    }
}`;
  const expected = `class Application {
    main() { // method declaration should be 2x indented from the class declaration
        final server = server() // statement1 should be 2x indented from the method declaration
        // statement2 should be 2x indented from the method declaration
        // method call with 2 or more arguments should be lowered down per argument and 1x indented from its parent method call
        server.get(
          // arguments should be 1x indented from the method call and should be on a new line
          "/",
          request -> response.ok( // lowering down the argument of a 1 argument method call is optional
            "Hello from my Ube server!" // this is a forced lowered down argument, it should be 1x indented from the method call
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
  const source = `class Application {
    main() {
        server.get("/", request -> response.ok("Hello"));
    }
}
`;

  const formatted = formatUbeDocument(source);
  assert.equal(formatUbeDocument(formatted), formatted);
});
