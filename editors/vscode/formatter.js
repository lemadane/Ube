const ARGUMENT_LINE = "\uE000";
const CALL_END_LINE = "\uE001";
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
    if (source[cursor] === quote && !isEscaped(source, cursor)) {
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
  if (source[index] === '"' || source[index] === "`") {
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

function hasTopLevelLineBreak(source, start, end) {
  let parenDepth = 0;
  let bracketDepth = 0;
  let braceDepth = 0;

  for (let cursor = start; cursor < end; cursor += 1) {
    const skipped = skipNonCode(source, cursor, end);
    if (skipped !== cursor) {
      cursor = skipped - 1;
      continue;
    }
    if (source[cursor] === "\n" && parenDepth === 0 && bracketDepth === 0 && braceDepth === 0) {
      return true;
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
      default:
        break;
    }
  }
  return false;
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
    } else if (character === '"' || character === "`") {
      quote = character;
    }
    if (character === "\n" && !quote) {
      result += `${ARGUMENT_LINE}${extraIndent}:`;
      discardIndent = true;
    }
  }
  return result;
}

function formatCallArguments(source, start, end, extraIndent = 0) {
  let result = "";
  let cursor = start;

  while (cursor < end) {
    const skipped = skipNonCode(source, cursor, end);
    if (skipped !== cursor) {
      result += source.slice(cursor, skipped);
      cursor = skipped;
      continue;
    }
    if (source[cursor] !== "(") {
      result += source[cursor];
      cursor += 1;
      continue;
    }

    const closeIndex = findClosingParenthesis(source, cursor, end);
    if (closeIndex === -1) {
      result += source.slice(cursor, end);
      break;
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
        hasTopLevelLineBreak(source, cursor + 1, closeIndex)
      );
    let argumentIndent = extraIndent + (isMultilineCall ? 1 : 0);
    let formattedArguments = argumentRanges.map(([argStart, argEnd]) =>
      formatCallArguments(source, argStart, argEnd, argumentIndent).trim()
    );
    if (
      !isMultilineCall &&
      formattedArguments.some(
        (argument) => argument.includes(ARGUMENT_LINE) || argument.includes(CALL_END_LINE)
      )
    ) {
      isMultilineCall = true;
      argumentIndent = extraIndent + 1;
      formattedArguments = argumentRanges.map(([argStart, argEnd]) =>
        formatCallArguments(source, argStart, argEnd, argumentIndent).trim()
      );
    }
    if (isMultilineCall) {
      result += `(${openingComment}\n`;
      result += formattedArguments
        .map((argument) => markArgumentLines(argument, extraIndent))
        .join(",\n");
      result += `\n${CALL_END_LINE}${extraIndent}:)`;
    } else {
      result += `(${formatCallArguments(source, cursor + 1, closeIndex, extraIndent)})`;
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
      if (line[cursor] === lexicalState.quote && !isEscaped(line, cursor)) {
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
    if (line[cursor] === '"' || line[cursor] === "`") {
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
        countBraces(line, lexicalState);
        return line;
      }

      const callEnds = [...trimmed.matchAll(/\uE001(\d+):/g)];
      const closingIndent = callEnds.length > 0
        ? Math.max(...callEnds.map(([, rawIndent]) => Number(rawIndent))) * CALL_ARGUMENT_INDENT
        : null;
      const argumentMarkers = [...trimmed.matchAll(/\uE000(\d+):/g)];
      const argumentIndent = argumentMarkers.length > 0
        ? Math.max(...argumentMarkers.map(([, rawIndent]) => Number(rawIndent)))
        : null;
      trimmed = trimmed.replace(/[\uE000\uE001]\d+:/g, "");

      const lineDepth = Math.max(0, braceDepth - leadingClosingBraces(trimmed));
      const indentSpaces = closingIndent !== null
        ? braceDepth * STRUCTURAL_INDENT + closingIndent
        : argumentIndent !== null
          ? braceDepth * STRUCTURAL_INDENT + (argumentIndent + 1) * CALL_ARGUMENT_INDENT
          : lineDepth * STRUCTURAL_INDENT;
      const output = `${" ".repeat(indentSpaces)}${trimmed}`;
      braceDepth = Math.max(0, braceDepth + countBraces(trimmed, lexicalState));
      return output;
    })
    .join("\n");
}

function formatUbeDocument(source) {
  const formattedCalls = formatCallArguments(source, 0, source.length);
  return `${indentLines(formattedCalls).trimEnd()}\n`;
}

module.exports = { formatUbeDocument };
