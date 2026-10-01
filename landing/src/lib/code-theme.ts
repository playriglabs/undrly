import type { ThemeRegistration } from 'shiki';

// Shiki theme in the landing palette: forest keywords, warm strings, quiet comments.
export const codeTheme: ThemeRegistration = {
  name: 'undrly',
  type: 'dark',
  colors: {
    'editor.background': '#00000000',
    'editor.foreground': '#dfe3dc',
  },
  tokenColors: [
    { scope: ['comment', 'punctuation.definition.comment'], settings: { foreground: '#5f695f' } },
    {
      scope: ['keyword', 'storage', 'storage.type', 'storage.modifier', 'keyword.control'],
      settings: { foreground: '#b6c8a6' },
    },
    {
      scope: ['string', 'string.template', 'punctuation.definition.string'],
      settings: { foreground: '#d3c592' },
    },
    { scope: ['support.type.property-name.json'], settings: { foreground: '#9fc0b4' } },
    {
      scope: ['entity.name.function', 'support.function', 'meta.function-call'],
      settings: { foreground: '#e9ece6' },
    },
    {
      scope: ['variable.other.constant', 'variable.other.env', 'constant'],
      settings: { foreground: '#c6b6d8' },
    },
    {
      scope: ['punctuation.definition.template-expression', 'meta.embedded', 'variable.parameter'],
      settings: { foreground: '#dfe3dc' },
    },
    { scope: ['entity.name.type', 'support.type'], settings: { foreground: '#9fc0b4' } },
  ],
};
