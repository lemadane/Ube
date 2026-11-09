const vscode = require("vscode");

function activate(context) {
  const provider = vscode.languages.registerDocumentFormattingEditProvider("ube", {
    provideDocumentFormattingEdits(document) {
      // Pick up local formatter updates without restarting the extension host.
      const formatterPath = require.resolve("./formatter");
      delete require.cache[formatterPath];
      const { formatUbeDocument } = require(formatterPath);
      const source = document.getText();
      const formatted = formatUbeDocument(source);
      if (formatted === source) {
        return [];
      }

      const lastLine = document.lineAt(document.lineCount - 1);
      const range = new vscode.Range(
        0,
        0,
        document.lineCount - 1,
        lastLine.text.length
      );
      return [vscode.TextEdit.replace(range, formatted)];
    },
  });
  context.subscriptions.push(provider);
}

function deactivate() {}

module.exports = { activate, deactivate };
