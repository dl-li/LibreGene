import { describe, expect, it } from 'vitest';
import { renderToString } from 'react-dom/server';
import { AboutContent } from '../dialogs/AboutDialog';
import UpdaterPanel from '../components/UpdaterPanel';
import { I18nProvider } from '../i18n';

describe('About dialog', () => {
  it('shows the feedback email and current version', () => {
    const html = renderToString(
      <I18nProvider>
        <AboutContent updater={{ currentVersion: '0.9.4' }} />
      </I18nProvider>,
    );
    expect(html).toContain('libregene@donglin.li');
    expect(html).toContain('v0.9.4');
  });

  it('renders update availability in the update panel', () => {
    const html = renderToString(
      <I18nProvider>
        <UpdaterPanel
          updater={{
            status: 'available',
            autoCheck: true,
            update: { version: '0.9.5', body: 'Bug fixes' },
          }}
        />
      </I18nProvider>,
    );
    expect(html).toContain('Version 0.9.5 is available');
    expect(html).toContain('Bug fixes');
    expect(html).toContain('Download &amp; install');
  });
});
