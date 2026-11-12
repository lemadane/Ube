# Initial Ube formatter specification

The formatter follows the starter program's formatting needs:

- Use four spaces for each structural indentation level (two two-space increments).
- Put each additional condition in an `if` or `else if` on a new line, beginning with `and` or `or` and indented two spaces beyond the `if` line. Preserve grouping and expressions inside call arguments.
- Expand braced `if`, `else if`, and `else` bodies onto separate lines, with closing braces on their own lines. The language already requires braces around these bodies.
- Separate imports from the following declaration with a blank line.
- Expand class and trait method bodies onto separate lines, with the closing brace on its own line.
- Always expand `try` and `finally` bodies onto separate lines, with each consecutive call and following `return` on its own indented line and the closing brace on its own line.
- Indent a method declaration one level inside its class, and statements one level inside their method or nested block.
- Keep calls with zero or one argument on one line unless the call would exceed 80 columns (including indentation), the source explicitly breaks the argument across lines, the argument contains a nested call, or the argument is a null-coalescing expression (`??` or `??=`). Put that expression on its own argument line, with the closing parenthesis immediately after it.
- Put null-coalescing operators (`??` and `??=`) on the next line, indented two spaces beyond the expression’s starting indentation.
- Put a nested call on a new argument line, indented once (two spaces) inside its enclosing call, recursively. For example, `Console.printLine(cache.delete('greeting'))` becomes `Console.printLine(\n  cache.delete('greeting'))`.
- Put every argument of a call with two or more arguments on its own line, indented two spaces inside the call.
- For all call chains, put each method after the first call on its own line, indented two spaces beyond the base expression. Indent its arguments another two spaces. Keep simple chained calls such as `.limit(10)` compact.
- Keep each multiline call's closing parenthesis immediately after its final argument. Nested calls therefore end with adjacent parentheses such as `value))`; structural closing braces remain on their own lines.
- Indent Java-style lambda bodies four spaces beyond their parameter line. Align their closing `}` (including `})`) with the parameter line; do not keep the body indentation on the closing line.
- Apply multiline parameter layout to class constructors and method declarations. Preserve `Func(A, B) -> T` signatures, grouped function types, closure parameters, and control-flow parentheses as language constructs rather than independent calls.
- Wrap function types in parentheses: `Func(int) -> int makeAdder(int amount)` becomes `(Func(int) -> int) makeAdder(int amount)`. Apply this to parameters, locals, and nested function types too; preserve existing grouping.
- Apply indentation consistently to comment-only lines.
- Preserve string literals, comments, and source tokens. One-line strings and characters use single quotes; interpolated text blocks use backticks. Preserve their whitespace, backslashes, and line endings exactly. Double quotes are reserved for JSON content.
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
          (Request request) -> Response.ok(
            'Hello from my Ube server!'))
        server.start(8080)
    }
}
```

This formatting contract does not define the complete language grammar. Future versions can extend it to alignment and additional syntax constructs.
