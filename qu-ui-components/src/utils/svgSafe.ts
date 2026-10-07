import DOMPurify from 'dompurify';

/**
 * Sanitises SVG markup from a file the user opened or dropped. The result
 * is safe to inject as inner HTML: scripts, `foreignObject` (an escape
 * hatch into arbitrary HTML), event-handler attributes, `javascript:` URLs
 * and external `<use>`/`<image>` references are removed.
 *
 * Returns '' when nothing renderable remains.
 */
export function sanitizeSvg(source: string): string {
  const clean = DOMPurify.sanitize(source, {
    USE_PROFILES: { svg: true, svgFilters: true },
    FORBID_TAGS: ['script', 'foreignObject', 'iframe', 'embed', 'object'],
    FORBID_ATTR: ['onload', 'onclick', 'onerror', 'onmouseover', 'onfocus'],
    // Keep the root <svg> element itself.
    WHOLE_DOCUMENT: false,
    ADD_ATTR: ['viewBox'],
  });
  return typeof clean === 'string' && /<svg[\s>]/i.test(clean) ? clean : '';
}
