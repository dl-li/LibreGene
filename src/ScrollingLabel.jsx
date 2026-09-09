import { useRef, useState, useCallback } from 'react';
import { cn } from '@/lib/utils';

export default function ScrollingLabel({ text, className }) {
  const containerRef = useRef(null);
  const textRef = useRef(null);
  const [style, setStyle] = useState({});

  const handleEnter = useCallback(() => {
    const container = containerRef.current;
    const inner = textRef.current;
    if (!container || !inner) return;
    const overflow = inner.scrollWidth - container.clientWidth;
    if (overflow <= 0) return;
    setStyle({
      transform: `translateX(${-overflow}px)`,
      transition: `transform ${Math.max(0.8, overflow / 60)}s linear 0.3s`,
    });
  }, []);

  const handleLeave = useCallback(() => {
    setStyle({ transform: 'translateX(0)', transition: 'transform 0.25s ease-out' });
  }, []);

  return (
    <span
      ref={containerRef}
      className={cn('min-w-0 truncate', className)}
      onMouseEnter={handleEnter}
      onMouseLeave={handleLeave}
    >
      <span ref={textRef} className="inline-block will-change-transform" style={style}>
        {text}
      </span>
    </span>
  );
}
