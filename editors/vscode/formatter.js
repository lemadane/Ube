const ARGUMENT_LINE = "\uE000";
const MAX_LINE_LENGTH = 80;
const STRUCTURAL_INDENT = 4;
const CALL_ARGUMENT_INDENT = 2;

function isEscaped(source, index) {
  let backslashes = 0;
  for (let cursor = index - 1; cursor >= 0 && source[cursor] === "\\"; cursor -= 1) {
    backslashes += 1;
  }
  return backslashes % 2 === 1;
}

function skipQuotedString(source, index, end) {
  const quote = source[index];
  for (let cursor = index + 1; cursor < end; cursor += 1) {
    if (source[cursor] === quote && (quote === "`" || !isEscaped(source, cursor))) {
      return cursor + 1;
    }
  }
  return end;
}

function skipLineComment(source, index, end) {
  const newline = source.indexOf("\n", index + 2);
  return newline === -1 || newline >= end ? end : newline;
}

function skipBlockComment(source, index, end) {
  const close = source.indexOf("*/", index + 2);
  return close === -1 || close + 2 > end ? end : close + 2;
}

function skipNonCode(source, index, end) {
  if (source[index] === '"' || source[index] === "`" || source[index] === "'") {
    return skipQuotedString(source, index, end);
  }
  if (source[index] === "/" && source[index + 1] === "/") {
    return skipLineComment(source, index, end);
  }
  if (source[index] === "/" && source[index + 1] === "*") {
    return skipBlockComment(source, index, end);
  }
  return index;
}

function findClosingParenthesis(source, openIndex, end) {
  let depth = 1;
  for (let cursor = openIndex + 1; cursor < end; cursor += 1) {
    const skipped = skipNonCode(source, cursor, end);
    if (skipped !== cursor) {
      cursor = skipped - 1;
      continue;
    }
    if (source[cursor] === "(") {
      depth += 1;
    } else if (source[cursor] === ")") {
      depth -= 1;
      if (depth === 0) {
        return cursor;
      }
    }
  }
  return -1;
}

function splitTopLevelArguments(source, start, end) {
  const argumentsList = [];
  let argumentStart = start;
  let parenDepth = 0;
  let bracketDepth = 0;
  let braceDepth = 0;

  for (let cursor = start; cursor < end; cursor += 1) {
    const skipped = skipNonCode(source, cursor, end);
    if (skipped !== cursor) {
      cursor = skipped - 1;
      continue;
    }
    switch (source[cursor]) {
      case "(":
        parenDepth += 1;
        break;
      case ")":
        parenDepth -= 1;
        break;
      case "[":
        bracketDepth += 1;
        break;
      case "]":
        bracketDepth -= 1;
        break;
      case "{":
        braceDepth += 1;
        break;
      case "}":
        braceDepth -= 1;
        break;
      case ",":
        if (parenDepth === 0 && bracketDepth === 0 && braceDepth === 0) {
          argumentsList.push([argumentStart, cursor]);
          argumentStart = cursor + 1;
        }
        break;
      default:
        break;
    }
  }
  argumentsList.push([argumentStart, end]);
  return argumentsList;
}

function needsMultilineArgument(source, start, end) {
  let parenDepth = 0;
  let bracketDepth = 0;
  let braceDepth = 0;

  for (let cursor = start; cursor < end; cursor += 1) {
    const skipped = skipNonCode(source, cursor, end);
    if (skipped !== cursor) {
      cursor = skipped - 1;
      continue;
    }
    if (
      (source[cursor] === "\n" || source.slice(cursor, cursor + 2) === "??") &&
      parenDepth === 0 && bracketDepth === 0 && braceDepth === 0
    ) {
      return true;
    }
    switch (source[cursor]) {
      case "(":
        // A call used as an argument starts on its own continuation line,
        // even when that nested call has only one argument (or none).
        const closeIndex = findClosingParenthesis(source, cursor, end);
        if (closeIndex !== -1 && isCallParenthesis(source, cursor, closeIndex, end)) {
          return true;
        }
        parenDepth += 1;
        break;
      case ")":
        parenDepth -= 1;
        break;
      case "[":
        bracketDepth += 1;
        break;
      case "]":
        bracketDepth -= 1;
        break;
      case "{":
        braceDepth += 1;
        break;
      case "}":
        braceDepth -= 1;
        break;
      default:
        break;
    }
  }
  return false;
}

function previousToken(source, index) {
  let cursor = index - 1;
  while (cursor >= 0 && /\s/.test(source[cursor])) {
    cursor -= 1;
  }
  const end = cursor + 1;
  while (cursor >= 0 && /[A-Za-z0-9_]/.test(source[cursor])) {
    cursor -= 1;
  }
  return { text: source.slice(cursor + 1, end), start: cursor + 1, end };
}

function nextCodeIndex(source, index, end) {
  let cursor = index;
  while (cursor < end) {
    if (/\s/.test(source[cursor])) {
      cursor += 1;
      continue;
    }
    if (source[cursor] === "/" && source[cursor + 1] === "/") {
      cursor = skipLineComment(source, cursor, end);
      continue;
    }
    if (source[cursor] === "/" && source[cursor + 1] === "*") {
      cursor = skipBlockComment(source, cursor, end);
      continue;
    }
    break;
  }
  return cursor;
}

function isGroupedFunctionParenthesis(source, openIndex, closeIndex, end) {
  const after = nextCodeIndex(source, closeIndex + 1, end);
  return source.slice(after, after + 2) === "->";
}

function isCallParenthesis(source, openIndex, closeIndex, end) {
  const previous = previousToken(source, openIndex);
  if (["Func", "if", "for", "while", "catch", "switch", "match"].includes(previous.text)) {
    return false;
  }
  if (isGroupedFunctionParenthesis(source, openIndex, closeIndex, end)) {
    return false;
  }
  let cursor = openIndex - 1;
  while (cursor >= 0 && /\s/.test(source[cursor])) {
    cursor -= 1;
  }
  return cursor >= 0 && /[A-Za-z0-9_\])>]/.test(source[cursor]);
}

function markArgumentLines(argument, extraIndent) {
  let marked = `${ARGUMENT_LINE}${extraIndent}:${argument}`;
  let result = "";
  let quote = null;
  let inLineComment = false;
  let inBlockComment = false;
  let discardIndent = false;
  for (let cursor = 0; cursor < marked.length; cursor += 1) {
    const character = marked[cursor];
    if (discardIndent && (character === " " || character === "\t")) {
      continue;
    }
    discardIndent = false;
    result += character;
    if (inLineComment) {
      if (character === "\n") {
        inLineComment = false;
      }
    } else if (inBlockComment) {
      if (character === "*" && marked[cursor + 1] === "/") {
        inBlockComment = false;
      }
    } else if (quote) {
      if (character === quote && !isEscaped(marked, cursor)) {
        quote = null;
      }
    } else if (character === "/" && marked[cursor + 1] === "/") {
      inLineComment = true;
    } else if (character === "/" && marked[cursor + 1] === "*") {
      inBlockComment = true;
    } else if (character === '"' || character === "`" || character === "'") {
      quote = character;
    }
    if (character === "\n" && !quote) {
      result += `${ARGUMENT_LINE}${extraIndent}:`;
      discardIndent = true;
    }
  }
  return result;
}

function appendClosingParenthesis(argument) {
  let quote = null;
  let inBlockComment = false;
  let lineComment = -1;
  let lastNewline = -1;
  for (let cursor = 0; cursor < argument.length; cursor += 1) {
    const character = argument[cursor];
    if (character === "\n") {
      lastNewline = cursor;
      lineComment = -1;
      continue;
    }
    if (lineComment !== -1) {
      continue;
    }
    if (inBlockComment) {
      if (character === "*" && argument[cursor + 1] === "/") {
        inBlockComment = false;
        cursor += 1;
      }
    } else if (quote) {
      if (character === quote && (quote === "`" || !isEscaped(argument, cursor))) {
        quote = null;
      }
    } else if (character === "/" && argument[cursor + 1] === "/") {
      lineComment = cursor;
      cursor += 1;
    } else if (character === "/" && argument[cursor + 1] === "*") {
      inBlockComment = true;
      cursor += 1;
    } else if (character === '"' || character === "`" || character === "'") {
      quote = character;
    }
  }
  if (lineComment > lastNewline) {
    let insertion = lineComment;
    while (insertion > lastNewline + 1 && /[ \t]/.test(argument[insertion - 1])) {
      insertion -= 1;
    }
    return `${argument.slice(0, insertion)})${argument.slice(insertion)}`;
  }
  return `${argument})`;
}

function formatCallArguments(source, start, end, extraIndent = 0) {
  let result = "";
  let cursor = start;
  let continuationIndent = extraIndent;

  while (cursor < end) {
    const skipped = skipNonCode(source, cursor, end);
    if (skipped !== cursor) {
      result += source.slice(cursor, skipped);
      cursor = skipped;
      continue;
    }
    if (source.slice(cursor, cursor + 2) === "??") {
      result = result.trimEnd();
      result += `\n${ARGUMENT_LINE}${extraIndent}:??`;
      cursor += 2;
      if (source[cursor] === "=") {
        result += "=";
        cursor += 1;
      }
      while (cursor < end && /\s/.test(source[cursor])) {
        cursor += 1;
      }
      result += " ";
      continuationIndent = extraIndent + 1;
      continue;
    }
    if (source[cursor] !== "(") {
      if (source[cursor] === "." && /^\s*$/.test(source.slice(source.lastIndexOf("\n", cursor) + 1, cursor))) {
        result += `${ARGUMENT_LINE}${extraIndent}:`;
        continuationIndent = extraIndent + 1;
      }
      if (source[cursor] === "\n") {
        continuationIndent = extraIndent;
      }
      result += source[cursor];
      cursor += 1;
      continue;
    }

    const closeIndex = findClosingParenthesis(source, cursor, end);
    if (closeIndex === -1) {
      result += source.slice(cursor, end);
      break;
    }

    if (!isCallParenthesis(source, cursor, closeIndex, end)) {
      result += `(${formatCallArguments(source, cursor + 1, closeIndex, continuationIndent)})`;
      cursor = closeIndex + 1;
      continue;
    }

    const argumentRanges = splitTopLevelArguments(source, cursor + 1, closeIndex);
    const openingCommentMatch = source
      .slice(cursor + 1, closeIndex)
      .match(/^[ \t]*(\/\/[^\r\n]*)(?:\r?\n)/);
    const openingComment = openingCommentMatch ? ` ${openingCommentMatch[1]}` : "";
    if (openingCommentMatch && argumentRanges[0][0] === cursor + 1) {
      argumentRanges[0][0] += openingCommentMatch[0].length;
    }
    const isMultiArgumentCall =
      argumentRanges.length > 1 &&
      argumentRanges.every(([argStart, argEnd]) => source.slice(argStart, argEnd).trim().length > 0);
    let isMultilineCall =
      isMultiArgumentCall ||
      (
        !isMultiArgumentCall &&
        argumentRanges[0][0] < argumentRanges[0][1] &&
        (needsMultilineArgument(source, cursor + 1, closeIndex) ||
          closeIndex + 1 - (source.lastIndexOf("\n", cursor) + 1) > MAX_LINE_LENGTH)
      );
    let argumentIndent = continuationIndent + (isMultilineCall ? 1 : 0);
    let formattedArguments = argumentRanges.map(([argStart, argEnd]) =>
      formatCallArguments(source, argStart, argEnd, argumentIndent).trim()
    );
    if (
      !isMultilineCall &&
      formattedArguments.some(
        (argument) => argument.includes(ARGUMENT_LINE)
      )
    ) {
      isMultilineCall = true;
      argumentIndent = continuationIndent + 1;
      formattedArguments = argumentRanges.map(([argStart, argEnd]) =>
        formatCallArguments(source, argStart, argEnd, argumentIndent).trim()
      );
    }
    if (isMultilineCall) {
      result += `(${openingComment}\n`;
      formattedArguments[formattedArguments.length - 1] = appendClosingParenthesis(
        formattedArguments[formattedArguments.length - 1]
      );
      result += formattedArguments
        .map((argument) => markArgumentLines(argument, continuationIndent))
        .join(",\n");
    } else {
      result += `(${formatCallArguments(source, cursor + 1, closeIndex, continuationIndent)})`;
    }
    cursor = closeIndex + 1;
  }

  return result;
}

function countBraces(line, lexicalState) {
  let delta = 0;
  for (let cursor = 0; cursor < line.length; cursor += 1) {
    if (lexicalState.inBlockComment) {
      const close = line.indexOf("*/", cursor);
      if (close === -1) {
        return delta;
      }
      lexicalState.inBlockComment = false;
      cursor = close + 1;
      continue;
    }
    if (lexicalState.quote) {
      if (line[cursor] === lexicalState.quote && (lexicalState.quote === "`" || !isEscaped(line, cursor))) {
        lexicalState.quote = null;
      }
      continue;
    }

    if (line[cursor] === "/" && line[cursor + 1] === "/") {
      break;
    }
    if (line[cursor] === "/" && line[cursor + 1] === "*") {
      lexicalState.inBlockComment = true;
      cursor += 1;
      continue;
    }
    if (line[cursor] === '"' || line[cursor] === "`" || line[cursor] === "'") {
      lexicalState.quote = line[cursor];
      continue;
    }
    if (line[cursor] === "{") {
      delta += 1;
    } else if (line[cursor] === "}") {
      delta -= 1;
    }
  }
  return delta;
}

function leadingClosingBraces(line) {
  let count = 0;
  for (const character of line.trimStart()) {
    if (character !== "}") {
      break;
    }
    count += 1;
  }
  return count;
}

function indentLines(source) {
  let braceDepth = 0;
  const lexicalState = { quote: null, inBlockComment: false };
  return source
    .split(/\r?\n/)
    .map((line) => {
      let trimmed = line.trim();
      if (!trimmed) {
        return "";
      }

      if (lexicalState.quote) {
        braceDepth = Math.max(0, braceDepth + countBraces(line, lexicalState));
        return line;
      }

      const argumentMarkers = [...trimmed.matchAll(/\uE000(\d+):/g)];
      const argumentIndent = argumentMarkers.length > 0
        ? Math.max(...argumentMarkers.map(([, rawIndent]) => Number(rawIndent)))
        : null;
      trimmed = trimmed.replace(/\uE000\d+:/g, "");

      const lineDepth = Math.max(0, braceDepth - leadingClosingBraces(trimmed));
      const indentSpaces = argumentIndent !== null
        ? lineDepth * STRUCTURAL_INDENT + (argumentIndent + 1) * CALL_ARGUMENT_INDENT
        : lineDepth * STRUCTURAL_INDENT;
      const output = `${" ".repeat(indentSpaces)}${trimmed}`;
      braceDepth = Math.max(0, braceDepth + countBraces(trimmed, lexicalState));
      return output;
    })
    .join("\n");
}

function groupFunctionTypes(source) {
  const tokens = [];
  for (let cursor = 0; cursor < source.length;) {
    const skipped = skipNonCode(source, cursor, source.length);
    if (skipped !== cursor) {
      cursor = skipped;
      continue;
    }
    if (/\s/.test(source[cursor])) {
      cursor += 1;
      continue;
    }
    const text = source.slice(cursor).match(/^[A-Za-z_][A-Za-z0-9_]*|^->/)?.[0] || source[cursor];
    tokens.push({ text, start: cursor, end: cursor + text.length });
    cursor += text.length;
  }
  const at = (index) => tokens[index]?.text;
  function typeEnd(index) {
    if (at(index) === "(") {
      const end = typeEnd(index + 1);
      return end !== null && at(end) === ")" ? end + 1 : null;
    }
    if (at(index) === "Func") {
      if (at(index + 1) !== "(") return null;
      let end = index + 2;
      if (at(end) !== ")") {
        while (true) {
          end = typeEnd(end);
          if (end === null) return null;
          if (at(end) !== ",") break;
          end += 1;
        }
      }
      if (at(end) !== ")" || at(end + 1) !== "->") return null;
      return typeEnd(end + 2);
    }
    if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(at(index) || "")) return null;
    let end = index + 1;
    if (at(end) === "<") {
      end = typeEnd(end + 1);
      if (end === null || at(end) !== ">") return null;
      end += 1;
    }
    return at(end) === "?" ? end + 1 : end;
  }
  const insertions = new Map();
  function insert(position, text) {
    insertions.set(position, (insertions.get(position) || "") + text);
  }
  for (let index = 0; index < tokens.length; index += 1) {
    if (at(index) !== "Func") continue;
    const end = typeEnd(index);
    if (end === null) continue;
    const beforeOpen = at(index - 2) || "";
    const followsName = /^[A-Za-z0-9_]+$/.test(beforeOpen) &&
      !["static", "async", "return", "yield", "final", "var"].includes(beforeOpen);
    const grouped = at(index - 1) === "(" && at(end) === ")" &&
      !followsName && ![")", "]"].includes(beforeOpen);
    if (!grouped) {
      insert(tokens[index].start, "(");
      insert(tokens[end - 1].end, ")");
    }
  }
  let result = source;
  for (const [position, text] of [...insertions].sort((a, b) => b[0] - a[0])) {
    result = result.slice(0, position) + text + result.slice(position);
  }
  return result;
}

function expandDeclarationBlocks(source) {
  const stack = [];
  const breaks = new Set();
  let previous = "";
  let pendingType = false;
  for (let cursor = 0; cursor < source.length;) {
    if (/\s/.test(source[cursor])) {
      cursor += 1;
      continue;
    }
    const skipped = skipNonCode(source, cursor, source.length);
    if (skipped !== cursor) {
      if (source[cursor] !== "/") previous = "literal";
      cursor = skipped;
      continue;
    }
    const token = source.slice(cursor).match(/^[A-Za-z_][A-Za-z0-9_]*/)?.[0] || source[cursor];
    const frame = stack[stack.length - 1];
    if (["class", "trait"].includes(token)) pendingType = true;
    if (frame?.finally && previous === ")" &&
        /^[A-Za-z_]/.test(token) &&
        !["and", "or", "is", "as", "in"].includes(token)) {
      breaks.add(cursor);
    }
    if (["{", "(", "["].includes(token)) {
      const isFinally = token === "{" && previous === "finally";
      const isMethod = token === "{" && frame?.type && previous === ")";
      const isType = token === "{" && pendingType;
      if (token === "{") pendingType = false;
      stack.push({ token, finally: isFinally, method: isMethod, type: isType });
      if (isFinally || (isMethod && !/^[ \t]*\/\//.test(source.slice(cursor + 1)))) {
        breaks.add(cursor + 1);
      }
    } else if (["}", ")", "]"].includes(token)) {
      if (token === "}" && (frame?.finally || frame?.method)) breaks.add(cursor);
      stack.pop();
    } else if (token === ";" && frame?.finally) {
      breaks.add(cursor + 1);
    }
    previous = token;
    cursor += token.length;
  }
  let result = source;
  for (const position of [...breaks].sort((a, b) => b - a)) {
    // Keep existing line breaks and comments intact; indentation is normalized later.
    if (!/[\r\n]/.test(result.slice(position).match(/^\s*/)[0]) &&
        !/[\r\n]/.test(source.slice(0, position).match(/\s*$/)[0])) {
      result = result.slice(0, position) + "\n" + result.slice(position);
    }
  }
  return result;
}

function separateImports(source) {
  let result = "";
  for (let cursor = 0; cursor < source.length;) {
    const skipped = skipNonCode(source, cursor, source.length);
    if (skipped !== cursor) {
      result += source.slice(cursor, skipped);
      cursor = skipped;
      continue;
    }
    const importLine = source.slice(cursor).match(/^import\b[^\r\n]*\r?\n/);
    if (importLine && /^[ \t]*$/.test(source.slice(source.lastIndexOf("\n", cursor - 1) + 1, cursor))) {
      result += importLine[0];
      cursor += importLine[0].length;
      if (cursor < source.length && !/^[ \t]*(?:\r?\n|import\b)/.test(source.slice(cursor))) {
        result += "\n";
      }
      continue;
    }
    result += source[cursor++];
  }
  return result;
}

function formatCallChains(source) {
  const edits = new Map();
  for (let cursor = 0; cursor < source.length; cursor += 1) {
    const skipped = skipNonCode(source, cursor, source.length);
    if (skipped !== cursor) {
      cursor = skipped - 1;
      continue;
    }
    if (source[cursor] !== "(") continue;
    const close = findClosingParenthesis(source, cursor, source.length);
    if (close === -1 || !isCallParenthesis(source, cursor, close, source.length)) continue;
    const calls = [{ open: cursor, close }];
    let previousClose = close;
    while (true) {
      const suffix = source.slice(previousClose + 1).match(/^\s*\.\s*[A-Za-z_][A-Za-z0-9_]*\s*\(/);
      if (!suffix) break;
      const dot = previousClose + 1 + suffix[0].indexOf(".");
      const open = previousClose + suffix[0].length;
      const nextClose = findClosingParenthesis(source, open, source.length);
      if (nextClose === -1) break;
      calls.push({ open, close: nextClose, dot, previousClose });
      previousClose = nextClose;
    }
    if (calls.length < 2) continue;
    for (const call of calls.slice(1)) {
      edits.set(call.previousClose + 1, { end: call.dot, text: "\n" });
      const argument = source.slice(call.open + 1, call.close).trim();
      if (splitTopLevelArguments(argument, 0, argument.length).length === 1 &&
          !argument.includes("\n") && !argument.includes("//") &&
          !needsMultilineArgument(argument, 0, argument.length)) {
        edits.set(call.open + 1, { end: call.close, text: argument });
      }
    }
  }
  let result = source;
  for (const [start, { end, text }] of [...edits].sort((a, b) => b[0] - a[0])) {
    result = result.slice(0, start) + text + result.slice(end);
  }
  return result;
}

function formatUbeDocument(source) {
  // Normalize structural indentation before measuring call widths.
  const normalized = indentLines(formatCallChains(groupFunctionTypes(expandDeclarationBlocks(separateImports(source)))));
  const formattedCalls = formatCallArguments(normalized, 0, normalized.length);
  return `${indentLines(formattedCalls).trimEnd()}\n`;
}

module.exports = { formatUbeDocument };
