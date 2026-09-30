// sRGB → relative luminance (WCAG 2.1)
const _hexToRgb = (h) => [
  parseInt(h.slice(1, 3), 16) / 255,
  parseInt(h.slice(3, 5), 16) / 255,
  parseInt(h.slice(5, 7), 16) / 255,
];
const _linearize = (c) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
const relLuminance = (r, g, b) =>
  0.2126 * _linearize(r) + 0.7152 * _linearize(g) + 0.0722 * _linearize(b);
const _rgbToHsl = (r, g, b) => {
  const M = Math.max(r, g, b),
    m = Math.min(r, g, b),
    d = M - m,
    l = (M + m) / 2;
  if (!d) return [0, 0, l];
  const s = l > 0.5 ? d / (2 - M - m) : d / (M + m);
  let h;
  if (M === r) h = ((g - b) / d + (g < b ? 6 : 0)) / 6;
  else if (M === g) h = ((b - r) / d + 2) / 6;
  else h = ((r - g) / d + 4) / 6;
  return [h, s, l];
};
const _hslToRgb = (h, s, l) => {
  if (!s) return [l, l, l];
  const q = l < 0.5 ? l * (1 + s) : l + s - l * s,
    p = 2 * l - q;
  const hue2rgb = (t) => {
    if (t < 0) t++;
    if (t > 1) t--;
    if (t < 1 / 6) return p + (q - p) * 6 * t;
    if (t < 1 / 2) return q;
    if (t < 2 / 3) return p + (q - p) * (2 / 3 - t) * 6;
    return p;
  };
  return [hue2rgb(h + 1 / 3), hue2rgb(h), hue2rgb(h - 1 / 3)];
};
const _rgbToHex = (r, g, b) =>
  '#' +
  [r, g, b]
    .map((c) =>
      Math.round(c * 255)
        .toString(16)
        .padStart(2, '0'),
    )
    .join('');

export const ensureReadableColor = (hex, bgHex = '#fdfbf7') => {
  const [r, g, b] = _hexToRgb(hex);
  const [br, bg, bb] = _hexToRgb(bgHex);
  const bgLum = relLuminance(br, bg, bb);
  const lum = relLuminance(r, g, b);
  const MIN_CONTRAST = 3.0; // WCAG non-text/UI-component minimum
  if ((bgLum + 0.05) / (lum + 0.05) >= MIN_CONTRAST) return hex;
  const [h, s, l] = _rgbToHsl(r, g, b);
  // Darken (keeping hue/saturation) until the contrast target is met.
  let newL = l;
  while (newL > 0.1) {
    newL = Math.max(0.1, newL - 0.02);
    const [nr, ng, nb] = _hslToRgb(h, s, newL);
    if ((bgLum + 0.05) / (relLuminance(nr, ng, nb) + 0.05) >= MIN_CONTRAST)
      return _rgbToHex(nr, ng, nb);
  }
  return _rgbToHex(..._hslToRgb(h, s, 0.1));
};

// Nudge lightness so two abutting same-colored bars stay distinguishable.
// Input colors are the readability-mapped ones (normFeatures); the shift is
// applied on top of that mapping.
export const shiftAbutLightness = (hex) => {
  if (!/^#[0-9a-fA-F]{6}$/.test(hex)) return hex;
  const [r, g, b] = _hexToRgb(hex);
  const [h, s, l] = _rgbToHsl(r, g, b);
  const nl = l <= 0.7 ? Math.min(1, l + 0.1) : Math.max(0, l - 0.1);
  return _rgbToHex(..._hslToRgb(h, s, nl));
};
