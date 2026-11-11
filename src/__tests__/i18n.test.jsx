import { afterEach, describe, expect, it } from 'vitest';
import { renderToString } from 'react-dom/server';
import { I18nProvider, T, translate } from '../i18n';
import { localizeSessionText } from '../i18n/sessionText';
import { agentPrompt } from '../dialogs/McpGuideDialog';

const originalLocalStorage = Object.getOwnPropertyDescriptor(globalThis, 'localStorage');

afterEach(() => {
  if (originalLocalStorage) {
    Object.defineProperty(globalThis, 'localStorage', originalLocalStorage);
  } else {
    delete globalThis.localStorage;
  }
});

function renderWithStoredLanguage(language) {
  Object.defineProperty(globalThis, 'localStorage', {
    configurable: true,
    value: {
      getItem: (key) => (key === 'language' ? language : null),
      setItem: () => {},
    },
  });
  return renderToString(
    <I18nProvider>
      <T>Open File</T>
    </I18nProvider>,
  );
}

describe('i18n', () => {
  it('renders both stored language choices', () => {
    expect(renderWithStoredLanguage('zh')).toBe('打开文件');
    expect(renderWithStoredLanguage('en')).toBe('Open File');
  });

  it('interpolates localized values', () => {
    expect(translate('zh', 'Add {count} feature(s)', { count: 3 })).toBe('添加 3 个特征');
    expect(translate('en', 'Add {count} feature(s)', { count: 3 })).toBe('Add 3 feature(s)');
  });

  it('localizes session summaries without changing names or coordinates', () => {
    const t = (text, values) => translate('zh', text, values);
    expect(localizeSessionText(t, 'Added feature ‘GFP’ (CDS, 2–6)')).toBe(
      '添加特征“GFP”（CDS，2–6）',
    );
    expect(localizeSessionText(t, 'Changed topology to linear')).toBe('拓扑改为线性');
  });

  it('preserves the original English MCP prompt and translates it in Chinese', () => {
    const english = agentPrompt(8766, 'secret', true, (text, values) =>
      translate('en', text, values),
    );
    expect(english)
      .toBe(`Configure LibreGene as a remote MCP server using your client's official documentation:
- Transport: Streamable HTTP
- URL: http://127.0.0.1:8766/mcp
- Send this header on every request: Authorization: Bearer secret
- Required Host header: 127.0.0.1:8766

After saving the configuration, ask the user to restart you or start a new conversation. Then verify the connection by calling list_workspace.`);

    const chinese = agentPrompt(8766, 'secret', true, (text, values) =>
      translate('zh', text, values),
    );
    expect(chinese).toBe(`请按照客户端官方文档，将 LibreGene 配置为远程 MCP 服务器：
- Transport: Streamable HTTP
- URL: http://127.0.0.1:8766/mcp
- 每次请求都发送此标头：Authorization: Bearer secret
- 必需的 Host 标头：127.0.0.1:8766

保存配置后，请让用户重启你或开始新对话。然后调用 list_workspace 验证连接。`);
  });
});
