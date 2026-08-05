import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
  DialogClose,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { Label } from '@/components/ui/label';
import { Input } from '@/components/ui/input';
import { Separator } from '@/components/ui/separator';
import { Bot, Check, ChevronDown, ChevronRight, Copy, RefreshCw } from 'lucide-react';
import { useEffect, useState } from 'react';
import { getMcpToken, regenerateMcpToken } from '@/tauriApi';

function CopyButton({ text }) {
  const [copied, setCopied] = useState(false);
  return (
    <Button
      variant="ghost"
      size="icon"
      className="size-6 shrink-0"
      title="Copy"
      onClick={() => {
        navigator.clipboard.writeText(text).catch(() => {});
        setCopied(true);
        setTimeout(() => setCopied(false), 1500);
      }}
    >
      {copied ? <Check className="size-3.5 text-green-500" /> : <Copy className="size-3.5" />}
    </Button>
  );
}

function Snippet({ label, hint, text }) {
  return (
    <div className="min-w-0">
      <div className="flex items-center justify-between gap-2">
        <div className="text-xs font-medium">{label}</div>
        <CopyButton text={text} />
      </div>
      {hint && <div className="text-[11px] text-muted-foreground mb-1">{hint}</div>}
      <pre className="max-w-full text-[11px] leading-relaxed bg-muted rounded-md p-2 overflow-x-auto whitespace-pre-wrap break-all">
        {text}
      </pre>
    </div>
  );
}

function mcpSnippets(port) {
  const url = `http://127.0.0.1:${port}/mcp`;
  return [
    {
      label: 'opencode',
      hint: 'opencode.json (project-level or global ~/.config/opencode/). First, in your shell: export LIBREGENE_MCP_TOKEN=<token> — the token shown in this dialog.',
      text: `{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "libregene": {
      "type": "remote",
      "url": "${url}",
      "enabled": true,
      "headers": {
        "Authorization": "Bearer {env:LIBREGENE_MCP_TOKEN}"
      }
    }
  }
}`,
    },
  ];
}

export default function McpGuideDialog({ open, onOpenChange, mcpConfig, onMcpConfigChange }) {
  const [guideOpen, setGuideOpen] = useState(false);
  const [token, setToken] = useState('');
  useEffect(() => {
    if (!open) return;
    getMcpToken()
      .then((r) => setToken(r?.token ?? ''))
      .catch(() => {});
  }, [open]);
  const handleRegenerate = () => {
    regenerateMcpToken()
      .then((r) => setToken(r?.token ?? ''))
      .catch(() => {});
  };
  const enabled = Boolean(mcpConfig?.enabled);
  const port = mcpConfig?.port ?? 8766;
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="grid-cols-1 sm:max-w-lg max-h-[85vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Bot className="size-4 text-muted-foreground" />
            MCP Server
          </DialogTitle>
          <DialogDescription>
            Let an LLM agent operate the open plasmid via the Model Context Protocol (20 tools:
            overview digests, sequence read/edit, feature & primer CRUD, primer design, ORF,
            alignment, and more).
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-1">
          <div className="space-y-2">
            <div className="flex items-center gap-2">
              <Checkbox
                id="mcp-guide-enabled"
                checked={enabled}
                onCheckedChange={(v) => onMcpConfigChange?.({ ...mcpConfig, enabled: Boolean(v) })}
              />
              <Label htmlFor="mcp-guide-enabled" className="cursor-pointer text-sm font-normal">
                Enable MCP server
              </Label>
            </div>
            <p className="text-xs text-muted-foreground leading-relaxed">
              Loopback only (127.0.0.1:{port}) — never exposed to the network.
            </p>
            <div className="flex items-center gap-2">
              <Label className="text-sm text-muted-foreground shrink-0">Port</Label>
              <Input
                className="w-20 h-8 px-2 py-0 text-sm text-right font-mono"
                type="number"
                min="1"
                max="65535"
                value={port}
                disabled={!enabled}
                onChange={(e) => {
                  const v = parseInt(e.target.value, 10);
                  if (!isNaN(v) && v >= 1 && v <= 65535) {
                    onMcpConfigChange?.({ ...mcpConfig, port: v });
                  }
                }}
              />
            </div>
            <div className="space-y-1">
              <div className="flex items-center gap-2">
                <Label className="text-sm text-muted-foreground shrink-0">Access token</Label>
                <code className="flex-1 min-w-0 truncate text-[11px] font-mono bg-muted rounded px-2 py-1 select-all">
                  {token || '…'}
                </code>
                <CopyButton text={token} />
                <Button
                  variant="ghost"
                  size="icon"
                  className="size-6 shrink-0"
                  title="Regenerate token"
                  onClick={handleRegenerate}
                >
                  <RefreshCw className="size-3.5" />
                </Button>
              </div>
              <p className="text-[11px] text-muted-foreground leading-relaxed">
                Required by the MCP server on every request. Regenerating invalidates the old token
                immediately — update your agent client config afterwards.
              </p>
            </div>
          </div>

          <Separator />

          <div>
            <button
              type="button"
              className="flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground transition-colors"
              onClick={() => setGuideOpen(!guideOpen)}
            >
              {guideOpen ? (
                <ChevronDown className="size-3.5" />
              ) : (
                <ChevronRight className="size-3.5" />
              )}
              How to connect your agent
            </button>
            {guideOpen && (
              <div className="space-y-3 mt-3">
                <p className="text-xs text-muted-foreground leading-relaxed">
                  {enabled
                    ? 'The MCP server is running. Add one of the following configs to your agent client:'
                    : 'The MCP server is currently disabled — tick Enable above first.'}
                </p>
                {mcpSnippets(port).map((s) => (
                  <Snippet key={s.label} {...s} />
                ))}
                <p className="text-[11px] text-muted-foreground leading-relaxed">
                  After configuring, restart your agent session and ask it to call{' '}
                  <code>list_projects</code> to verify the connection.
                </p>
              </div>
            )}
          </div>
        </div>

        <DialogFooter>
          <DialogClose asChild>
            <Button variant="outline">Close</Button>
          </DialogClose>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
