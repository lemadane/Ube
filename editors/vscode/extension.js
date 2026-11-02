const vscode = require("vscode");
const { formatUbeDocument } = require("./formatter");

function activate(context) {
  const provider = vscode.languages.registerDocumentFormattingEditProvider("ube", {
    provideDocumentFormattingEdits(document) {
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
