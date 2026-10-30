import {
  Bot,
  ChevronDown,
  Clock,
  ExternalLink,
  FilePlus2,
  FolderOpen,
  Info,
  LoaderCircle,
  Puzzle,
  Settings,
  X,
} from 'lucide-react';
import { cn } from '@/lib/utils';
import ScrollingLabel from './ScrollingLabel';
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
} from '@/components/ui/sidebar';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import { getFileIcon } from '../fileIcons';
import { fileNameOf } from '../recentFiles';

export default function ProjectSidebar({
  sidebarWheelRef,
  nativeTitleBar,
  handleOpenFile,
  setNewSeqOpen,
  sidebarScroll,
  setSidebarScroll,
  sidebarContentRef,
  visibleRecent,
  windowInfo,
  recentOpen,
  setRecentOpen,
  handleOpenRecent,
  handleRemoveRecent,
  projects,
  orderedProjects,
  activeId,
  dirtyById,
  fileName,
  dragTabId,
  tabItemRefs,
  handleTabPointerDown,
  suppressTabClickRef,
  handleSwitchProject,
  handleOpenInNewWindow,
  handleCloseProject,
  visiblePlugins,
  handlesRef,
  sidebarTargetId,
  setMcpGuideOpen,
  openSettings,
  setAboutOpen,
}) {
  return (
    <Sidebar
      ref={sidebarWheelRef}
      collapsible="icon"
      variant="sidebar"
      className={`${nativeTitleBar ? '' : 'pt-10 '}transition-[width] duration-300 ease-out`}
    >
      <SidebarHeader className="flex flex-row items-center gap-2.5 overflow-hidden px-2.5 pb-2 pt-1.5">
        <div className="flex size-7 shrink-0 items-center justify-center rounded-lg bg-primary text-primary-foreground shadow-sm">
          <div className="relative size-5">
            <svg viewBox="0 0 24 24" className="absolute inset-0 size-5">
              <circle
                cx="12"
                cy="12"
                r="9"
                fill="none"
                stroke="white"
                strokeOpacity="0.3"
                strokeWidth="2"
              />
            </svg>
            <LoaderCircle className="relative size-5 text-white" />
          </div>
        </div>
        <div className="flex min-w-0 flex-col whitespace-nowrap group-data-[state=collapsed]:hidden">
          <span className="text-[13px] font-semibold leading-tight tracking-tight text-foreground">
            LibreGene
          </span>
          <span className="text-[10px] leading-tight text-muted-foreground">Plasmid Editor</span>
        </div>
      </SidebarHeader>
      <SidebarGroup>
        <SidebarGroupContent>
          <SidebarMenu>
            <SidebarMenuItem>
              <SidebarMenuButton onClick={handleOpenFile} tooltip="Open File">
                <FolderOpen className="size-4" />
                <span>Open File…</span>
              </SidebarMenuButton>
            </SidebarMenuItem>
            <SidebarMenuItem>
              <SidebarMenuButton onClick={() => setNewSeqOpen(true)} tooltip="New File">
                <FilePlus2 className="size-4" />
                <span>New File…</span>
              </SidebarMenuButton>
            </SidebarMenuItem>
          </SidebarMenu>
        </SidebarGroupContent>
      </SidebarGroup>
      <div className="relative flex min-h-0 flex-1 flex-col">
        <div
          className={cn(
            'pointer-events-none absolute inset-x-0 top-0 z-10 h-7 bg-gradient-to-b from-sidebar from-30% to-transparent transition-opacity duration-200',
            sidebarScroll.up ? 'opacity-100' : 'opacity-0',
          )}
        />
        <div
          className={cn(
            'pointer-events-none absolute inset-x-0 bottom-0 z-10 h-7 bg-gradient-to-t from-sidebar from-30% to-transparent transition-opacity duration-200',
            sidebarScroll.down ? 'opacity-100' : 'opacity-0',
          )}
        />
        <SidebarContent
          ref={sidebarContentRef}
          onScroll={(e) => {
            const el = e.currentTarget;
            setSidebarScroll({
              up: el.scrollTop > 1,
              down: el.scrollTop + el.clientHeight < el.scrollHeight - 1,
            });
          }}
        >
          {visibleRecent.length > 0 && (!windowInfo || windowInfo.type === 'main') && (
            <Collapsible open={recentOpen} onOpenChange={setRecentOpen}>
              <SidebarGroup className="pt-0">
                <CollapsibleTrigger asChild>
                  <SidebarGroupLabel className="cursor-pointer select-none hover:text-sidebar-foreground">
                    <Clock className="mr-1.5 size-3" />
                    <span className="uppercase tracking-wider text-[10px] font-semibold">
                      Recent
                    </span>
                    <ChevronDown
                      className={`ml-auto size-3.5 shrink-0 transition-transform duration-200 ${recentOpen ? 'rotate-0' : '-rotate-90'}`}
                    />
                  </SidebarGroupLabel>
                </CollapsibleTrigger>
                <CollapsibleContent>
                  <SidebarGroupContent>
                    <SidebarMenu>
                      {visibleRecent.slice(0, 8).map((p) => {
                        const name = fileNameOf(p);
                        const Icon = getFileIcon(name);
                        return (
                          <SidebarMenuItem key={p}>
                            <SidebarMenuButton
                              onClick={() => handleOpenRecent(p)}
                              tooltip={p}
                              className="flex-1 min-w-0 pr-5"
                            >
                              <Icon className="size-4 shrink-0" />
                              <span className="truncate">{name}</span>
                            </SidebarMenuButton>
                            <button
                              type="button"
                              aria-label="Remove from recent"
                              onClick={(e) => {
                                e.stopPropagation();
                                handleRemoveRecent(p);
                              }}
                              title="Remove from recent"
                              className="absolute right-2 top-1/2 hidden -translate-y-1/2 size-4 items-center justify-center rounded text-muted-foreground hover:bg-sidebar-accent-foreground/10 hover:text-foreground group-hover/menu-item:flex"
                            >
                              <X className="size-3" />
                            </button>
                          </SidebarMenuItem>
                        );
                      })}
                    </SidebarMenu>
                  </SidebarGroupContent>
                </CollapsibleContent>
              </SidebarGroup>
            </Collapsible>
          )}
          {projects.length > 0 && (
            <SidebarGroup className="pt-0">
              <SidebarGroupLabel>
                <span className="uppercase tracking-wider text-[10px] font-semibold">Opened</span>
                <span className="ml-1.5 rounded-full bg-sidebar-accent px-1.5 py-px text-[10px] font-medium tabular-nums text-sidebar-accent-foreground">
                  {projects.length}
                </span>
              </SidebarGroupLabel>
              <SidebarGroupContent>
                <SidebarMenu>
                  {orderedProjects.map((p) => {
                    const isActiveProject = p.id === activeId;
                    const name = fileName(p);
                    const Icon = getFileIcon(name);
                    const isDragging = dragTabId === p.id;
                    return (
                      <SidebarMenuItem
                        key={p.id}
                        ref={(el) => {
                          if (el) tabItemRefs.current.set(p.id, el);
                          else tabItemRefs.current.delete(p.id);
                        }}
                        onPointerDown={(e) => handleTabPointerDown(e, p.id)}
                        onContextMenu={(e) => {
                          if (dragTabId) e.preventDefault();
                        }}
                        className={cn(isDragging && 'z-10 opacity-80 cursor-grabbing')}
                        style={dragTabId ? { touchAction: 'none', userSelect: 'none' } : undefined}
                      >
                        {isActiveProject && (
                          <span className="absolute left-0 top-1/2 h-4 w-0.5 -translate-y-1/2 rounded-full bg-primary" />
                        )}
                        <SidebarMenuButton
                          onClick={() => {
                            if (suppressTabClickRef.current) {
                              suppressTabClickRef.current = false;
                              return;
                            }
                            handleSwitchProject(p.id);
                          }}
                          isActive={isActiveProject}
                          tooltip={name}
                          className={cn(
                            'flex-1 min-w-0 select-none',
                            !windowInfo || windowInfo.type !== 'project' ? 'pr-12' : 'pr-6',
                            isDragging && 'bg-sidebar-accent ring-1 ring-sidebar-ring',
                          )}
                        >
                          <Icon className="size-4 shrink-0" />
                          <ScrollingLabel text={name} />
                        </SidebarMenuButton>
                        {dirtyById[p.id] === true && (
                          <span className="pointer-events-none absolute right-2.5 top-1/2 size-1.5 -translate-y-1/2 rounded-full bg-amber-500 group-hover/menu-item:hidden group-data-[state=collapsed]:hidden" />
                        )}
                        {!windowInfo || windowInfo.type !== 'project' ? (
                          <div
                            className="absolute right-1 top-1/2 hidden -translate-y-1/2 items-center group-hover/menu-item:flex group-data-[state=collapsed]:hidden"
                            onPointerDown={(e) => e.stopPropagation()}
                          >
                            <button
                              className="flex size-5 items-center justify-center rounded text-muted-foreground transition-colors hover:bg-sidebar-accent-foreground/10 hover:text-foreground"
                              onClick={(e) => {
                                e.stopPropagation();
                                handleOpenInNewWindow(p.id);
                              }}
                              title="Open in new window"
                            >
                              <ExternalLink className="size-3" />
                            </button>
                            <button
                              className="flex size-5 items-center justify-center rounded text-muted-foreground transition-colors hover:bg-sidebar-accent-foreground/10 hover:text-foreground"
                              onClick={(e) => {
                                e.stopPropagation();
                                handleCloseProject(p.id);
                              }}
                              title="Close"
                            >
                              <X className="size-3" />
                            </button>
                          </div>
                        ) : (
                          <button
                            className="absolute right-1 top-1/2 hidden size-5 -translate-y-1/2 items-center justify-center rounded text-muted-foreground transition-colors hover:bg-sidebar-accent-foreground/10 hover:text-foreground group-hover/menu-item:flex group-data-[state=collapsed]:hidden"
                            onPointerDown={(e) => e.stopPropagation()}
                            onClick={(e) => {
                              e.stopPropagation();
                              handleCloseProject(p.id);
                            }}
                            title="Close"
                          >
                            <X className="size-3" />
                          </button>
                        )}
                      </SidebarMenuItem>
                    );
                  })}
                </SidebarMenu>
              </SidebarGroupContent>
            </SidebarGroup>
          )}
        </SidebarContent>
      </div>
      <SidebarFooter className="p-0">
        <SidebarGroup>
          <SidebarGroupContent>
            <SidebarMenu>
              {visiblePlugins.flatMap((plugin) =>
                plugin.sidebarItems.map((item) => (
                  <SidebarMenuItem key={`${plugin.id}-${item.dialogKey}`}>
                    <SidebarMenuButton
                      onClick={() =>
                        handlesRef.current[sidebarTargetId]?.openPluginDialog(item.dialogKey)
                      }
                      tooltip={item.tooltip}
                      className="text-muted-foreground hover:text-foreground"
                    >
                      <item.icon className="size-4" />
                      <span>{item.label}</span>
                    </SidebarMenuButton>
                  </SidebarMenuItem>
                )),
              )}
              <SidebarMenuItem>
                <SidebarMenuButton
                  onClick={() => setMcpGuideOpen(true)}
                  tooltip="MCP Server (LLM agent)"
                  className="text-muted-foreground hover:text-foreground"
                >
                  <Bot className="size-4" />
                  <span>MCP Server</span>
                </SidebarMenuButton>
              </SidebarMenuItem>
              <SidebarMenuItem>
                <SidebarMenuButton
                  onClick={() => openSettings('plugins')}
                  tooltip="Manage plugins"
                  className="text-muted-foreground hover:text-foreground"
                >
                  <Puzzle className="size-4" />
                  <span>Plugins</span>
                </SidebarMenuButton>
              </SidebarMenuItem>
              <SidebarMenuItem>
                <SidebarMenuButton
                  onClick={() => openSettings()}
                  tooltip="Settings"
                  className="text-muted-foreground hover:text-foreground"
                >
                  <Settings className="size-4" />
                  <span>Settings</span>
                </SidebarMenuButton>
              </SidebarMenuItem>
              <SidebarMenuItem>
                <SidebarMenuButton
                  onClick={() => setAboutOpen(true)}
                  tooltip="About LibreGene"
                  className="text-muted-foreground hover:text-foreground"
                >
                  <Info className="size-4" />
                  <span>About</span>
                </SidebarMenuButton>
              </SidebarMenuItem>
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarFooter>
    </Sidebar>
  );
}
