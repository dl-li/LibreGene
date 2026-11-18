import { useEffect, useMemo, useRef, useState } from 'react';
import { Info } from 'lucide-react';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import { T, useTranslation } from '@/i18n';
import { getEnzymeDatabase } from '../tauriApi';
import {
  parseSequenceShortcuts,
  replaceSequenceShortcut,
  stripValidSequenceShortcuts,
} from '../sequenceShortcuts';

let enzymeRecordsCache = null;
let enzymeRecordsPromise = null;

function loadEnzymeRecords() {
  if (enzymeRecordsCache) return Promise.resolve(enzymeRecordsCache);
  if (!enzymeRecordsPromise) {
    enzymeRecordsPromise = getEnzymeDatabase()
      .then((records) => {
        enzymeRecordsCache = Array.isArray(records) ? records : [];
        return enzymeRecordsCache;
      })
      .catch(() => {
        enzymeRecordsCache = [];
        return enzymeRecordsCache;
      });
  }
  return enzymeRecordsPromise;
}

export function useSequenceShortcuts(value, enabled = true) {
  const [enzymeRecords, setEnzymeRecords] = useState(enzymeRecordsCache);
  const needsEnzymes = enabled && value.includes('[') && enzymeRecords === null;

  useEffect(() => {
    if (!needsEnzymes) return;
    let cancelled = false;
    loadEnzymeRecords().then((records) => {
      if (!cancelled) setEnzymeRecords(records);
    });
    return () => {
      cancelled = true;
    };
  }, [needsEnzymes]);

  const parsed = useMemo(
    () =>
      enabled ? parseSequenceShortcuts(value, enzymeRecords || []) : { groups: [], tokens: [] },
    [value, enzymeRecords, enabled],
  );
  const plainText = useMemo(
    () => stripValidSequenceShortcuts(value, parsed.groups),
    [value, parsed.groups],
  );

  return {
    ...parsed,
    enabled,
    plainText,
    loading: needsEnzymes,
    hasSyntax: enabled && (value.includes('[') || value.includes(']')),
    hasUnresolved: parsed.groups.some((group) => group.valid),
  };
}

function HighlightedText({ value, tokens, activeToken, setActiveToken, className, scrollRef }) {
  const segments = [];
  let cursor = 0;
  for (const token of tokens) {
    if (token.start < cursor) continue;
    if (token.start > cursor) segments.push(value.slice(cursor, token.start));
    segments.push(
      <span
        key={token.id}
        className={cn(
          'pointer-events-auto rounded-[2px]',
          activeToken === token.id && 'bg-[#4b2e20] text-white',
        )}
        onMouseEnter={() => setActiveToken(token.id)}
        onMouseLeave={() => setActiveToken(null)}
      >
        {value.slice(token.start, token.end)}
      </span>,
    );
    cursor = token.end;
  }
  segments.push(value.slice(cursor));

  return (
    <div
      ref={scrollRef}
      aria-hidden="true"
      className={cn(
        className,
        'pointer-events-none absolute inset-0 z-10 m-0 overflow-hidden border-transparent bg-transparent text-transparent shadow-none ring-0',
      )}
    >
      {segments}
    </div>
  );
}

export function SequenceShortcutHint({ align = 'end' }) {
  return (
    <TooltipProvider>
      <Tooltip>
        <TooltipTrigger asChild>
          <button
            type="button"
            className="inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 text-[11px] font-normal normal-case tracking-normal text-muted-foreground outline-none transition-colors hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/40"
          >
            <Info className="size-3" />
            <T>{'Sequence shortcut input'}</T>
          </button>
        </TooltipTrigger>
        <TooltipContent
          side="bottom"
          align={align}
          sideOffset={6}
          className="w-[340px] max-w-[calc(100vw-2rem)] p-3 text-left text-pretty"
        >
          <div className="mb-2.5 flex items-center justify-between gap-3">
            <span className="font-sans text-xs font-semibold">
              <T>{'Sequence shortcut input'}</T>
            </span>
            <span className="text-[10px] uppercase tracking-wider opacity-60">
              <T>{'3 code types'}</T>
            </span>
          </div>

          <div className="space-y-1.5">
            <div className="flex items-start gap-2.5 rounded-md bg-background/10 p-2">
              <code className="min-w-14 rounded bg-background/10 px-1.5 py-0.5 text-center text-[11px] leading-4">
                5N
              </code>
              <div className="min-w-0 font-sans">
                <div className="text-[11px] font-medium leading-4">
                  <T>{'Random bases'}</T>
                </div>
                <div className="mt-0.5 text-[11px] leading-4 opacity-75">
                  <T>{'A number + N sets the random-base length.'}</T>
                </div>
              </div>
            </div>

            <div className="flex items-start gap-2.5 rounded-md bg-background/10 p-2">
              <code className="min-w-14 rounded bg-background/10 px-1.5 py-0.5 text-center text-[11px] leading-4">
                EcoRI
              </code>
              <div className="min-w-0 font-sans">
                <div className="text-[11px] font-medium leading-4">
                  <T>{'Restriction enzyme'}</T>
                </div>
                <div className="mt-0.5 text-[11px] leading-4 opacity-75">
                  <T>{'Case-insensitive enzyme name → recognition sequence.'}</T>
                </div>
              </div>
            </div>

            <div className="flex items-start gap-2.5 rounded-md bg-background/10 p-2">
              <code className="min-w-14 rounded bg-background/10 px-1.5 py-0.5 text-center text-[11px] leading-4">
                HHHHHH
              </code>
              <div className="min-w-0 font-sans">
                <div className="text-[11px] font-medium leading-4">
                  <T>{'Amino acids'}</T>
                </div>
                <div className="mt-0.5 text-[11px] leading-4 opacity-75">
                  <T>{'Random synonymous codons; * → stop codon.'}</T>
                </div>
              </div>
            </div>
          </div>

          <div className="mt-3 border-t border-background/20 pt-2.5">
            <div className="mb-1.5 font-sans text-[10px] font-medium uppercase tracking-wider opacity-60">
              <T>{'Examples'}</T>
            </div>
            <div className="flex flex-wrap gap-1.5">
              <code className="rounded bg-background/10 px-1.5 py-0.5 text-[11px]">
                [5N, EcoRI]
              </code>
              <code className="rounded bg-background/10 px-1.5 py-0.5 text-[11px]">[HHHHHH]</code>
            </div>
            <div className="mt-2 font-sans text-[11px] leading-4 opacity-75">
              <T>{'Separate codes with commas. Click a badge to replace it.'}</T>
            </div>
          </div>
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}

export default function SequenceShortcutInput({
  value,
  onChange,
  shortcuts,
  inputRef,
  multiline = false,
  className,
  inputClassName,
  onScroll,
  showHint = true,
  ...inputProps
}) {
  const { t } = useTranslation();
  const localRef = useRef(null);
  const overlayRef = useRef(null);
  const actualRef = inputRef || localRef;
  const [activeToken, setActiveToken] = useState(null);
  const InputTag = multiline ? 'textarea' : 'input';

  const handleScroll = (event) => {
    if (overlayRef.current) {
      overlayRef.current.scrollTop = event.currentTarget.scrollTop;
      overlayRef.current.scrollLeft = event.currentTarget.scrollLeft;
    }
    onScroll?.(event);
  };

  const replaceToken = (token, replacement) => {
    onChange(replaceSequenceShortcut(value, token, replacement));
    setActiveToken(null);
    requestAnimationFrame(() => actualRef.current?.focus());
  };

  const optionKind = (kind) => {
    if (kind === 'enzyme') return t('Restriction enzyme');
    if (kind === 'aminoAcids') return t('Amino acids');
    return t('Random bases');
  };

  return (
    <div className={cn('min-w-0', className)}>
      <div className="relative">
        <InputTag
          ref={actualRef}
          value={value}
          onChange={(event) => onChange(event.target.value)}
          onScroll={handleScroll}
          className={inputClassName}
          {...inputProps}
        />
        {shortcuts.tokens.length > 0 && (
          <HighlightedText
            value={value}
            tokens={shortcuts.tokens}
            activeToken={activeToken}
            setActiveToken={setActiveToken}
            className={cn(
              inputClassName,
              multiline ? 'whitespace-pre-wrap break-all' : 'whitespace-pre',
            )}
            scrollRef={overlayRef}
          />
        )}
      </div>

      {shortcuts.tokens.length > 0 && (
        <div className="mt-1.5 flex flex-wrap gap-1.5">
          {shortcuts.tokens.flatMap((token) =>
            token.options.map((option) => {
              const label = `${token.text} → ${option.replacement}`;
              return (
                <button
                  key={`${token.id}:${option.kind}`}
                  type="button"
                  title={optionKind(option.kind)}
                  aria-label={label}
                  onMouseEnter={() => setActiveToken(token.id)}
                  onMouseLeave={() => setActiveToken(null)}
                  onFocus={() => setActiveToken(token.id)}
                  onBlur={() => setActiveToken(null)}
                  onMouseDown={(event) => event.preventDefault()}
                  onClick={() => replaceToken(token, option.replacement)}
                  className={cn(
                    'inline-flex max-w-full items-center rounded-md border px-2 py-0.5 font-mono text-[11px] leading-4 transition-colors',
                    activeToken === token.id
                      ? 'border-[#4b2e20] bg-[#4b2e20] text-white'
                      : 'border-amber-200 bg-amber-50 text-amber-950 hover:border-amber-300',
                  )}
                >
                  <span className="truncate">{label}</span>
                </button>
              );
            }),
          )}
        </div>
      )}

      {showHint && shortcuts.enabled && (
        <div className="mt-1.5 flex justify-end">
          <SequenceShortcutHint />
        </div>
      )}
    </div>
  );
}
