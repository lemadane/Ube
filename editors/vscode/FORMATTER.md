# Initial Ube formatter specification

The formatter follows the starter program's formatting needs:

- Use four spaces for each structural indentation level (two two-space increments).
- Indent a method declaration one level inside its class, and statements one level inside their method or nested block.
- Keep calls with zero or one argument on one line unless the source explicitly breaks the argument across lines.
- Put every argument of a call with two or more arguments on its own line, indented two spaces inside the call.
- Put multiline-call closing parentheses on their own lines, aligned with the beginning of the line where the call starts.
- Apply indentation consistently to comment-only lines.
- Preserve string literals, comments, and source tokens. One-line strings and characters use single quotes; text blocks use raw backticks. Preserve their whitespace, backslashes, and line endings exactly. Double quotes are reserved for JSON content.
- Preserve `field()` getters, `field(value)` setters, and private `this._field` access. Formatting does not add constructors to zero-initialized declarations or evaluate constructor defaults.
- Preserve the newline after a bare `return`; moving a following expression onto that line changes its meaning.
- Ube prohibits statement semicolons. The formatter preserves invalid source tokens for compiler diagnostics and never inserts semicolons.
- Formatting is deterministic and idempotent.

For example:

```ube
class App {
    start() {
        final server = Server()
        server.get(
          '/',
          request -> Response.ok(
            'Hello from my Ube server!'
          )
        )
        server.start(8080)
    }
}
```

This formatting contract does not define the complete language grammar. Future versions can extend it to alignment, line wrapping, and additional syntax constructs.
