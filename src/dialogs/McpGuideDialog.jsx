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
import { writePlainClipboard } from '../clipboardAnnotations';

function CopyButton({ text }) {
  const [copied, setCopied] = useState(false);
  return (
    <Button
      variant="ghost"
      size="icon"
      className="size-6 shrink-0"
      title="Copy"
      onClick={() => {
        writePlainClipboard(text).catch(() => {});
        setCopied(true);
        setTimeout(() => setCopied(false), 1500);
      }}
    >
      {copied ? <Check className="size-3.5 text-green-500" /> : <Copy className="size-3.5" />}
    </Button>
  );
}

function Snippet({ label, text }) {
  return (
    <div className="min-w-0">
      <div className="flex items-center justify-between gap-2">
        <div className="text-xs font-medium">{label}</div>
        <CopyButton text={text} />
      </div>
      <pre className="max-w-full text-[11px] leading-relaxed bg-muted rounded-md p-2 overflow-x-auto whitespace-pre-wrap break-all">
        {text}
      </pre>
    </div>
  );
}

function agentPrompt(port, token, requireAuth) {
  const authLine = requireAuth
    ? `- Send this header on every request: Authorization: Bearer ${token || '<token from the MCP Server dialog>'}`
    : '- No Authorization header is needed.';
  return `Configure LibreGene as a remote MCP server using your client's official documentation:
- Transport: Streamable HTTP
- URL: http://127.0.0.1:${port}/mcp
${authLine}
- Required Host header: 127.0.0.1:${port}

After saving the configuration, ask the user to restart you or start a new conversation. Then verify the connection by calling list_workspace.`;
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
  const requireAuth = mcpConfig?.requireAuth !== false;
  // Port edits stay local until blur/Enter; committing on every keystroke
  // would restart the backend server per key and can strand it on a
  // half-typed port (<1024 bind failures flip enabled off server-side).
  const [portDraft, setPortDraft] = useState(String(port));
  useEffect(() => {
    setPortDraft(String(port));
  }, [port]);
  const commitPort = () => {
    const v = parseInt(portDraft, 10);
    if (!isNaN(v) && v >= 1 && v <= 65535) {
      if (v !== port) onMcpConfigChange?.({ ...mcpConfig, port: v });
    } else {
      setPortDraft(String(port));
    }
  };
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="grid-cols-1 sm:max-w-lg max-h-[85vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Bot className="size-4 text-muted-foreground" />
            MCP Server
          </DialogTitle>
          <DialogDescription>
            Let an LLM agent read and edit LibreGene projects via the Model Context Protocol.
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
              Available only on this computer (127.0.0.1).
            </p>
            <div className="flex items-center gap-2">
              <Label className="text-sm text-muted-foreground shrink-0">Port</Label>
              <Input
                className="w-20 h-8 px-2 py-0 text-sm text-right font-mono"
                type="number"
                min="1"
                max="65535"
                value={portDraft}
                disabled={!enabled}
                onChange={(e) => setPortDraft(e.target.value)}
                onBlur={commitPort}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') commitPort();
                }}
              />
            </div>
            <div className="space-y-1">
              <div className="flex items-center gap-2">
                <Checkbox
                  id="mcp-guide-require-auth"
                  checked={requireAuth}
                  disabled={!enabled}
                  onCheckedChange={(v) =>
                    onMcpConfigChange?.({ ...mcpConfig, requireAuth: Boolean(v) })
                  }
                />
                <Label
                  htmlFor="mcp-guide-require-auth"
                  className="cursor-pointer text-sm font-normal"
                >
                  Require access token
                </Label>
              </div>
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
                {requireAuth
                  ? 'Required on every request. Regenerating immediately invalidates the old token.'
                  : 'Token verification is off; the Host check remains enabled.'}
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
                    ? 'Copy the prompt below and send it to your agent to configure the connection.'
                    : 'Enable the server above, then copy the prompt below for your agent.'}
                </p>
                <Snippet label="Agent setup prompt" text={agentPrompt(port, token, requireAuth)} />
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
