import { describe, expect, it } from 'vitest';
import { renderToString } from 'react-dom/server';
import ProjectSidebar from '../components/ProjectSidebar';
import { SidebarProvider } from '../components/ui/sidebar';
import { I18nProvider } from '../i18n';

function renderSidebar(updateAvailable) {
  return renderToString(
    <I18nProvider>
      <SidebarProvider>
        <ProjectSidebar
          nativeTitleBar={false}
          sidebarScroll={{ up: false, down: false }}
          visibleRecent={[]}
          projects={[]}
          orderedProjects={[]}
          dirtyById={{}}
          visiblePlugins={[]}
          handlesRef={{ current: {} }}
          updateAvailable={updateAvailable}
          openSettings={() => {}}
          setMcpGuideOpen={() => {}}
          setAboutOpen={() => {}}
        />
      </SidebarProvider>
    </I18nProvider>,
  );
}

describe('sidebar update notification', () => {
  it('shows the settings badge only when an update is available', () => {
    expect(renderSidebar(true)).toContain('bg-red-500');
    expect(renderSidebar(false)).not.toContain('bg-red-500');
  });
});
