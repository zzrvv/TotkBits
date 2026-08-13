import * as monaco from "monaco-editor";

const languageId = "xlink";

if (!monaco.languages.getLanguages().some(({ id }) => id === languageId)) {
  monaco.languages.register({
    id: languageId,
    aliases: ["Xlink", "ELink"],
    extensions: [".belnk", ".blalnk", ".bfevl", ".xlink"],
  });

  monaco.languages.setLanguageConfiguration(languageId, {
    comments: { lineComment: "//" },
    brackets: [["{", "}"], ["[", "]"], ["(", ")"]],
    autoClosingPairs: [
      { open: "{", close: "}" },
      { open: "[", close: "]" },
      { open: "(", close: ")" },
      { open: '"', close: '"' },
    ],
    surroundingPairs: [
      { open: "{", close: "}" },
      { open: "[", close: "]" },
      { open: "(", close: ")" },
      { open: '"', close: '"' },
    ],
    folding: { markers: { start: /^\s*[^=/{]+\s*\{\s*$/, end: /^\s*\}\s*$/ } },
    indentationRules: {
      increaseIndentPattern: /\{\s*$/,
      decreaseIndentPattern: /^\s*\}/,
    },
  });

  monaco.languages.setMonarchTokensProvider(languageId, {
    keywords: ["true", "false", "null"],
    tokenizer: {
      root: [
        [/\/\/.*$/, "comment"],
        [/^\s*([A-Za-z_][\w.:/-]*)(\s*)(\{)/, ["type.identifier", "", "delimiter.curly"]],
        [/([A-Za-z_][\w.:/-]*)(\s*)(=)/, ["variable.name", "", "operator"]],
        [/"([^"\\]|\\.)*"/, "string"],
        [/-?(?:0[xX][0-9a-fA-F]+|\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)/, "number"],
        [/[A-Za-z_][\w.:/-]*/, { cases: { "@keywords": "keyword", "@default": "string.unquoted" } }],
        [/[{}()[\]]/, "@brackets"],
        [/=/, "operator"],
        [/[;,]/, "delimiter"],
        [/\s+/, "white"],
      ],
    },
  });
}

