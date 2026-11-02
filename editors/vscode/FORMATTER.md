# Initial Ube formatter specification

The formatter follows the starter program's formatting needs:

- Use four spaces for each structural indentation level (two two-space increments).
- Indent a method declaration one level inside its class, and statements one level inside their method or nested block.
- Keep calls with zero or one argument on one line unless the source explicitly breaks the argument across lines.
- Put every argument of a call with two or more arguments on its own line, indented two spaces inside the call.
- Put multiline-call closing parentheses on their own lines, aligned with the beginning of the line where the call starts.
- Apply indentation consistently to comment-only lines.
- Preserve string literals, comments, semicolons, and source tokens.
- Do not add or remove statement semicolons.
- Formatting is deterministic and idempotent.

For example:

```ube
class Application {
    main() {
        final server = server()
        server.get(
          "/",
          request -> response.ok(
            "Hello from my Ube server!"
          )
        );
        server.start(8080)
    }
}
```

This formatting contract does not define the complete language grammar. Future versions can extend it to alignment, line wrapping, and additional syntax constructs.
