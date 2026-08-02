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
import { Bot, Check, Copy } from 'lucide-react';
import { useState } from 'react';

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
      hint: 'opencode.json（项目或全局 ~/.config/opencode/）',
      text: `{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "libregene": {
      "type": "remote",
      "url": "${url}",
      "enabled": true
    }
  }
}`,
    },
    {
      label: 'Claude Code',
      hint: '在终端运行一次即可',
      text: `claude mcp add --transport http libregene ${url}`,
    },
    {
      label: 'Kimi CLI',
      hint: '在终端运行一次即可',
      text: `kimi mcp add --transport http libregene ${url}`,
    },
    {
      label: '其他客户端（mcpServers JSON）',
      hint: '适用于 Cursor / Windsurf 等使用 mcpServers 配置的客户端',
      text: `{
  "mcpServers": {
    "libregene": {
      "url": "${url}"
    }
  }
}`,
    },
  ];
}

export default function McpGuideDialog({ open, onOpenChange, mcpConfig, onMcpConfigChange }) {
  const enabled = Boolean(mcpConfig?.enabled);
  const port = mcpConfig?.port ?? 8766;
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg max-h-[85vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Bot className="size-4 text-muted-foreground" />
            MCP Server
          </DialogTitle>
          <DialogDescription>
            让 LLM Agent 通过 Model Context Protocol 直接操作当前打开的质粒（28
            个工具：概览/序列读取/feature 编辑/碱基编辑/引物设计/ORF/比对等）。
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
          </div>

          <Separator />

          <div className="space-y-3">
            <p className="text-xs text-muted-foreground leading-relaxed">
              {enabled
                ? 'MCP 服务已在运行。在你的 Agent 客户端中添加以下任一配置：'
                : 'MCP 服务当前已停用，请先勾选上方 Enable。'}
            </p>
            {mcpSnippets(port).map((s) => (
              <Snippet key={s.label} {...s} />
            ))}
            <p className="text-[11px] text-muted-foreground leading-relaxed">
              配置完成后重启 Agent 会话，让它调用 <code>list_projects</code> 验证连接。
            </p>
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
